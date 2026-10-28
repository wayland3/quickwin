// 骨架阶段允许未使用代码，后续任务会逐步接入
#![allow(dead_code)]

mod commands;
mod error;
mod models;
mod platform;
mod services;

use services::hotkey::HotkeyManager;
use services::window::OverlayManager;
use tauri::{Emitter, Manager};

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
        ])
        .manage(OverlayManager::new())
        .manage(HotkeyManager::new())
        .setup(|app| {
            log::info!("QuickWin starting");
            services::tray::setup_tray(app.handle())?;

            // 注册默认热键
            let hotkey_manager = app.state::<HotkeyManager>();
            hotkey_manager.register_defaults(app.handle())?;

            // 数据库路径作为只读配置交给受管状态；command 侧按需开短连接
            match services::storage::DbPath::resolve() {
                Ok(db_path) => {
                    app.manage(db_path);
                }
                Err(e) => {
                    // 路径解析失败不阻塞启动，剪贴板 command 会返回可操作的中文错误
                    log::error!("failed to resolve database path: {}", e);
                }
            }

            // 启动剪贴板监听；monitor 必须交给 AppState 持有，否则线程会被立即回收
            match services::clipboard::ClipboardMonitor::start(app.handle().clone()) {
                Ok(monitor) => {
                    app.manage(monitor);
                }
                Err(e) => {
                    // 存储初始化失败时不阻塞应用启动，记录错误供诊断
                    log::error!("clipboard monitor failed to start: {}", e);
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
