// 骨架阶段允许未使用代码，后续任务会逐步接入
#![allow(dead_code)]

mod commands;
mod error;
mod models;
mod platform;
mod services;

use services::hotkey::{HotkeyAction, HotkeyManager};
use services::window::OverlayManager;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.emit("overlay://show", "launcher");
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: dirs::data_local_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("com.quickwin.app")
                            .join("logs"),
                        file_name: Some("quickwin".to_string()),
                    },
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::app_ping,
            commands::autostart_enable,
            commands::autostart_disable,
            commands::autostart_is_enabled,
            commands::overlay_show,
            commands::overlay_hide,
            commands::overlay_resize,
            commands::hotkey_status,
            commands::hotkey_rebind,
            commands::clipboard::clipboard_list,
            commands::clipboard::clipboard_get,
            commands::clipboard::clipboard_set_pinned,
            commands::clipboard::clipboard_delete,
            commands::clipboard::clipboard_clear,
            commands::clipboard::clipboard_activate,
            commands::clipboard::clipboard_set_restore_after_paste,
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::settings::settings_reset,
            commands::settings::settings_set_paused,
        ])
        .manage(OverlayManager::new())
        .manage(HotkeyManager::new())
        .manage(services::paste::PasteOptions::new())
        .setup(|app| {
            log::info!("QuickWin starting");
            services::tray::setup_tray(app.handle())?;

            // 数据库路径必须最先就绪：启动注册热键/自启对账都要读 settings
            let db_path = match services::storage::DbPath::resolve() {
                Ok(db_path) => {
                    let path = db_path.as_path().to_path_buf();
                    app.manage(db_path);
                    path
                }
                Err(e) => {
                    log::error!("failed to resolve database path: {}", e);
                    return Ok(());
                }
            };

            // 读取存储设置（非法/损坏时内部回退默认；DB 打不开也不阻断启动）
            let settings = match services::storage::ClipboardStorage::open_readonly_at(&db_path) {
                Ok(storage) => storage.load_settings(),
                Err(e) => {
                    log::warn!("failed to load settings at startup, using defaults: {}", e);
                    crate::models::Settings::default()
                }
            };

            // 启动热键：按存储设置注册，单个失败回退默认键；不中断启动
            let hotkey_manager = app.state::<HotkeyManager>();
            let startup_failed =
                register_startup_hotkeys(app.handle(), &hotkey_manager, &settings)?;
            for accelerator in &startup_failed {
                let _ = app.emit(
                    "hotkey://failed",
                    serde_json::json!({ "accelerator": accelerator }),
                );
            }

            // 开机自启对账：以 settings 为准应用一次（外部禁用后重启会自动校正）
            let autolaunch = app.autolaunch();
            let reconcile = if settings.autostart {
                autolaunch.enable()
            } else {
                autolaunch.disable()
            };
            if let Err(e) = reconcile {
                log::warn!("autostart reconcile failed: {}", e);
            }

            // 粘贴还原开关：启动时同步到进程内选项
            if let Some(options) = app.try_state::<services::paste::PasteOptions>() {
                options.set_restore_after_paste(settings.restore_clipboard_after_paste);
            }

            // 粘贴抑制状态：监听线程与 clipboard_activate 共享
            let suppression = services::clipboard::PasteSuppression::new();
            let flags = services::clipboard::RuntimeFlags::new();
            app.manage(suppression.handle());
            app.manage(flags.clone());

            // 启动剪贴板监听；monitor 必须交给 AppState 持有，否则线程会被立即回收
            match services::clipboard::ClipboardMonitor::start(
                app.handle().clone(),
                suppression,
                flags,
            ) {
                Ok(monitor) => {
                    app.manage(monitor);
                }
                Err(e) => {
                    log::error!("clipboard monitor failed to start: {}", e);
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        // 启动期不可恢复错误（事件循环建立失败），按 AGENTS.md §2 #3 允许 panic
        .expect("error while running tauri application");
}

/// 启动时按存储设置注册热键
///
/// 每个动作先注册设置中的键；失败（被占用/无效）则回退该动作的默认键。
/// 返回最终仍注册失败的键列表（正常为空）。
fn register_startup_hotkeys(
    app: &tauri::AppHandle,
    manager: &HotkeyManager,
    settings: &crate::models::Settings,
) -> Result<Vec<String>, crate::error::AppError> {
    let bindings = [
        (
            HotkeyAction::Launcher,
            settings.launcher_hotkey.as_str(),
            "Alt+Space",
        ),
        (
            HotkeyAction::Clipboard,
            settings.clipboard_hotkey.as_str(),
            "Alt+V",
        ),
        (
            HotkeyAction::Actions,
            settings.actions_hotkey.as_str(),
            "Alt+X",
        ),
    ];

    let mut failed = Vec::new();
    for (action, configured, default) in bindings {
        match manager.rebind(app, action, configured) {
            Ok(true) => {}
            _ => {
                // 配置键注册失败：尝试默认键，并记录配置键（供前端提示改绑）
                if !manager.rebind(app, action, default)? {
                    failed.push(default.to_string());
                }
                failed.push(configured.to_string());
            }
        }
    }

    Ok(failed)
}
