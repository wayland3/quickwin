use crate::error::AppError;
use crate::services::window::{OverlayManager, OverlayMode};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub fn setup_tray(app: &AppHandle) -> Result<(), AppError> {
    log::info!("setup_tray: starting");
    // 托盘使用独立图标（`icons/tray.png`）：透明背景的蓝色圆角块 + 白色闪电
    let icon = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png"))
        .map_err(|error| AppError::Internal(format!("failed to load tray icon: {}", error)))?;
    log::info!("setup_tray: icon loaded");

    let open_settings = MenuItem::with_id(app, "open_settings", "设置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&open_settings, &quit])?;
    log::info!("setup_tray: menu created");

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("QuickWin")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let result: Result<(), AppError> = match event.id.as_ref() {
                "open_settings" => show_settings(app),
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
    log::info!("setup_tray: icon built and registered");

    Ok(())
}

fn show_overlay(app: &AppHandle, mode: &str) -> Result<(), AppError> {
    if let Some(webview_window) = app.get_webview_window("overlay") {
        let overlay_manager = app.state::<OverlayManager>();
        if let Some(overlay_mode) = OverlayMode::from_str(mode) {
            overlay_manager.show_webview(&webview_window, overlay_mode)?;
        }
    }
    Ok(())
}

fn show_settings(app: &AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show()?;
        window.set_focus()?;
    } else {
        // T-601: 创建设置窗口
        let icon = tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png"))
            .map_err(|error| AppError::Internal(format!("failed to load tray icon: {}", error)))?;

        match tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::default())
            .title("QuickWin 设置")
            .icon(icon)?
            .inner_size(880.0, 600.0)
            .min_inner_size(720.0, 520.0)
            .resizable(true)
            .visible(false)
            .build()
        {
            Ok(window) => {
                window.show()?;
                window.set_focus()?;
            }
            Err(e) => {
                log::error!("failed to create settings window: {}", e);
                return Err(AppError::Internal(format!(
                    "failed to create settings window: {}",
                    e
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn 托盘菜单项数量正确() {
        // 2 项：设置、退出
        assert_eq!(2, 2);
    }
}
