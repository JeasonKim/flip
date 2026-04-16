mod commands;

use tauri::Manager;
use tauri_plugin_positioner::WindowExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .setup(|app| {
            // 托盘弹窗：无装饰、置顶、隐藏
            let popup = tauri::WebviewWindowBuilder::new(
                app,
                "popup",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Flip")
            .inner_size(360.0, 500.0)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()?;

            // 失焦自动隐藏弹窗
            let popup_clone = popup.clone();
            popup.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = popup_clone.hide();
                }
            });

            Ok(())
        })
        .on_tray_icon_event(|tray_icon, event| {
            tauri_plugin_positioner::on_tray_event(tray_icon.app_handle(), &event);

            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                let app = tray_icon.app_handle();
                if let Some(popup) = app.get_webview_window("popup") {
                    let visible = popup.is_visible().unwrap_or(false);
                    if visible {
                        let _ = popup.hide();
                    } else {
                        let _ = popup.move_window(
                            tauri_plugin_positioner::Position::TrayBottomCenter,
                        );
                        let _ = popup.show();
                        let _ = popup.set_focus();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::flip_account,
            commands::capture_current,
            commands::dismiss_account,
            commands::detect_unsaved,
            commands::fetch_quota,
            commands::rename_account,
            commands::scan_sessions,
            commands::load_session_messages,
            commands::list_session_projects,
            commands::open_session_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Flip");
}
