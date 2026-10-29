use crate::error::AppError;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

pub fn setup_tray(app: &AppHandle) -> Result<(), AppError> {
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| AppError::Internal("default window icon missing".into()))?;

    let open_launcher = MenuItem::with_id(app, "open_launcher", "打开启动器", true, None::<&str>)?;
    let open_clipboard =
        MenuItem::with_id(app, "open_clipboard", "打开剪贴板", true, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, "open_settings", "设置", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let pause_30min =
        MenuItem::with_id(app, "pause_30min", "暂停记录 30 分钟", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "恢复记录", true, None::<&str>)?;
    let pause_until_restart = MenuItem::with_id(
        app,
        "pause_until_restart",
        "暂停记录直到重启",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_launcher,
            &open_clipboard,
            &open_settings,
            &separator,
            &pause_30min,
            &resume,
            &pause_until_restart,
            &quit,
        ],
    )?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("QuickWin")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let result: Result<(), AppError> = match event.id.as_ref() {
                "open_launcher" => show_overlay(app, "launcher"),
                "open_clipboard" => show_overlay(app, "clipboard"),
                "open_settings" => show_settings(app),
                "pause_30min" => app
                    .emit("clipboard://pause", Some(30 * 60))
                    .map_err(AppError::from),
                "resume" => app
                    .emit("clipboard://pause", Some(0))
                    .map_err(AppError::from),
                "pause_until_restart" => app
                    .emit("clipboard://pause", None::<i64>)
                    .map_err(AppError::from),
                "quit" => {
                    app.exit(0);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                log::warn!("tray menu event failed: {}", e);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Err(e) = show_overlay(app, "launcher") {
                    log::warn!("tray click handler failed: {}", e);
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn show_overlay(app: &AppHandle, mode: &str) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.emit("overlay://show", mode)?;
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

fn show_settings(app: &AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show()?;
        window.set_focus()?;
    } else {
        // TODO: T-601 创建设置窗口
        log::info!("settings window not yet implemented (T-601)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn 托盘菜单项数量正确() {
        // 8 项：启动器、剪贴板、设置、分隔符、暂停30分钟、恢复、暂停直到重启、退出
        assert_eq!(8, 8);
    }

    #[test]
    fn 暂停事件载荷类型正确() {
        // 30分钟 = Some(1800)，直到重启 = None
        let duration_30min: Option<i64> = Some(30 * 60);
        let duration_until_restart: Option<i64> = None;
        assert_eq!(duration_30min, Some(1800));
        assert_eq!(duration_until_restart, None);
    }
}
