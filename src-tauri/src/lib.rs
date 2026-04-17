mod agent;
mod commands;
mod credential;
mod file_ops;
mod import;
mod profile;
mod quota;
mod session;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
#[cfg(target_os = "macos")]
use tauri_plugin_positioner::WindowExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .setup(|app| {
            // macOS：仅在菜单栏托管，不显示 Dock 图标
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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

            // macOS：全屏应用上方也能显示弹窗
            #[cfg(target_os = "macos")]
            let _ = popup.set_visible_on_all_workspaces(true);

            // 失焦自动隐藏弹窗
            let popup_clone = popup.clone();
            popup.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = popup_clone.hide();
                }
            });

            // 托盘右键菜单
            let menu = Menu::with_items(
                app,
                &[
                    &MenuItem::with_id(app, "sessions", "会话", true, None::<&str>)?,
                    &MenuItem::with_id(app, "config", "配置文件", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?,
                ],
            )?;

            // 程序化创建托盘图标（附带右键菜单 + 平台定位）
            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("failed to load tray icon");

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    let app = tray.app_handle();
                    tauri_plugin_positioner::on_tray_event(app, &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect: _rect,
                        ..
                    } = event
                    {
                        let Some(popup) = app.get_webview_window("popup") else {
                            return;
                        };
                        let visible = popup.is_visible().unwrap_or(false);
                        if visible {
                            let _ = popup.hide();
                            return;
                        }

                        // macOS：菜单栏在顶部，弹窗向下展开
                        #[cfg(target_os = "macos")]
                        {
                            let _ = popup.move_window(
                                tauri_plugin_positioner::Position::TrayBottomCenter,
                            );
                        }

                        // Windows：任务栏在底部，弹窗向上展开到托盘图标上方
                        #[cfg(not(target_os = "macos"))]
                        {
                            if let Ok(size) = popup.outer_size() {
                                let x = _rect.position.x as i32
                                    + (_rect.size.width as i32 / 2)
                                    - (size.width as i32 / 2);
                                let y = _rect.position.y as i32 - size.height as i32;
                                let _ = popup.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(x, y),
                                ));
                            }
                        }

                        let _ = popup.show();
                        let _ = popup.set_focus();
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "sessions" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = crate::commands::open_session_window(app).await;
                        });
                    }
                    "config" => {
                        tauri::async_runtime::spawn(async {
                            let _ = crate::commands::reveal_config_file().await;
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::flip_account,
            commands::capture_current,
            commands::dismiss_account,
            commands::detect_unsaved,
            commands::fetch_quota,
            commands::read_model_info,
            commands::rename_account,
            commands::scan_sessions,
            commands::load_session_messages,
            commands::resume_session,
            commands::open_session_window,
            commands::purge_sessions,
            commands::import_from_ccswitch,
            commands::enroll_api_account,
            commands::reveal_config_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Flip");
}
