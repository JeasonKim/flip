mod agent;
mod commands;
mod credential;
mod current_account;
mod file_ops;
mod import;
mod profile;
mod quota;
mod session;
mod tray_panel;

use crate::tray_panel::{
    calculate_macos_popup_origin, resolve_tray_panel_visibility_change,
    should_hide_tray_panel_after_focus_change, PopupFrameSize, TrayAnchorFrame,
    TrayPanelVisibilityChange, WorkAreaFrame,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

/// 托盘图标的选中态：仅由左键点击切换，用它决定面板是否应该显示。
static TRAY_ICON_SELECTED: AtomicBool = AtomicBool::new(false);
const TRAY_ICON_ID: &str = "main-tray";

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

            // macOS：设置窗口层级和集合行为，确保全屏空间下面板不被系统隐藏
            #[cfg(target_os = "macos")]
            setup_macos_panel(&popup);

            register_popup_focus_behavior(app.handle(), &popup);

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

            let _tray = TrayIconBuilder::with_id(TRAY_ICON_ID)
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    let app = tray.app_handle();
                    tauri_plugin_positioner::on_tray_event(app, &event);

                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            position: click_pos,
                            rect: tray_rect,
                            ..
                        } => {
                            let Some(popup) = app.get_webview_window("popup") else {
                                log::warn!(
                                    "[tray-panel] popup window missing when toggling tray selection"
                                );
                                sync_tray_icon_selection(tray, false);
                                return;
                            };

                            match resolve_tray_panel_visibility_change(
                                TRAY_ICON_SELECTED.load(Ordering::SeqCst),
                            ) {
                                TrayPanelVisibilityChange::Show => {
                                    if show_popup(&popup, click_pos, tray_rect) {
                                        sync_tray_icon_selection(tray, true);
                                        TRAY_ICON_SELECTED.store(true, Ordering::SeqCst);
                                    } else {
                                        sync_tray_icon_selection(tray, false);
                                    }
                                }
                                TrayPanelVisibilityChange::Hide => {
                                    if hide_popup(&popup) {
                                        sync_tray_icon_selection(tray, false);
                                        TRAY_ICON_SELECTED.store(false, Ordering::SeqCst);
                                    }
                                }
                            }
                        }
                        _ => {}
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
            commands::reconcile_live_profiles,
            commands::flip_account,
            commands::capture_current,
            commands::dismiss_account,
            commands::sync_credentials,
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

fn show_popup(
    popup: &tauri::WebviewWindow,
    click_pos: tauri::PhysicalPosition<f64>,
    tray_rect: tauri::Rect,
) -> bool {
    #[cfg(target_os = "macos")]
    let _ = click_pos;
    #[cfg(not(target_os = "macos"))]
    let _ = tray_rect;

    // macOS：菜单栏在顶部，弹窗向下展开
    #[cfg(target_os = "macos")]
    {
        // 面板显示期间，禁用失焦自动隐藏，这样菜单栏会保持显示
        if let Err(err) = set_hides_on_deactivate(popup, false) {
            log::warn!("[tray-panel] failed to disable hidesOnDeactivate: {err}");
        }

        let popup_size = match popup.outer_size() {
            Ok(size) => PopupFrameSize::from(size),
            Err(err) => {
                log::warn!("[tray-panel] failed to read popup size before showing on macOS: {err}");
                return false;
            }
        };
        let tray_anchor = TrayAnchorFrame::from(tray_rect);
        let work_area = popup
            .monitor_from_point(
                f64::from(tray_anchor.x + (tray_anchor.width / 2)),
                f64::from(tray_anchor.y + (tray_anchor.height / 2)),
            )
            .ok()
            .flatten()
            .map(|monitor| WorkAreaFrame::from(monitor.work_area()));
        let popup_origin = calculate_macos_popup_origin(tray_anchor, popup_size, work_area);

        if let Err(err) = popup.set_position(tauri::Position::Physical(popup_origin.into())) {
            log::warn!("[tray-panel] failed to position popup below tray icon on macOS: {err}");
        }
    }

    // Windows：任务栏在底部，弹窗向上展开到点击位置上方
    #[cfg(not(target_os = "macos"))]
    {
        if let Ok(size) = popup.outer_size() {
            let x = click_pos.x as i32 - (size.width as i32 / 2);
            let y = click_pos.y as i32 - size.height as i32;
            if let Err(err) = popup.set_position(tauri::Position::Physical(
                tauri::PhysicalPosition::new(x, y),
            )) {
                log::warn!(
                    "[tray-panel] failed to position popup above tray icon x={x} y={y}: {err}"
                );
            }
        } else {
            log::warn!("[tray-panel] failed to read popup size before showing");
        }
    }

    if let Err(err) = popup.show() {
        log::warn!("[tray-panel] failed to show popup after selecting tray icon: {err}");
        return false;
    }
    if let Err(err) = popup.emit("flip-popup-shown", ()) {
        log::warn!("[tray-panel] failed to emit popup shown event: {err}");
    }

    // Windows：保持“任务栏图标被选中 → 面板显示”的简单切换语义，不主动抢焦点。
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = popup.set_focus() {
            log::warn!("[tray-panel] failed to focus popup after showing: {err}");
        }
    }

    true
}

fn hide_popup(popup: &tauri::WebviewWindow) -> bool {
    // 恢复失焦自动隐藏（下次显示面板前会再次禁用）
    #[cfg(target_os = "macos")]
    if let Err(err) = set_hides_on_deactivate(popup, true) {
        log::warn!("[tray-panel] failed to re-enable hidesOnDeactivate: {err}");
    }

    if let Err(err) = popup.hide() {
        log::warn!("[tray-panel] failed to hide popup after unselecting tray icon: {err}");
        return false;
    }

    true
}

fn sync_tray_icon_selection<R: tauri::Runtime>(tray: &tauri::tray::TrayIcon<R>, is_selected: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc::sel;
        use objc::sel_impl;

        if let Err(err) = tray.with_inner_tray_icon(move |inner| {
            if let Some(status_item) = inner.ns_status_item() {
                unsafe {
                    let status_item_ptr =
                        (&*status_item) as *const _ as *mut objc::runtime::Object;
                    let button: *mut objc::runtime::Object =
                        objc::msg_send![status_item_ptr, button];
                    if button.is_null() {
                        log::warn!(
                            "[tray-panel] tray status button missing when syncing selected={is_selected}"
                        );
                        return;
                    }
                    let _: () = objc::msg_send![button, highlight: is_selected];
                }
            } else {
                log::warn!(
                    "[tray-panel] tray status item missing when syncing selected={is_selected}"
                );
            }
        }) {
            log::warn!("[tray-panel] failed to sync tray icon selected={is_selected}: {err}");
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (tray, is_selected);
}

fn register_popup_focus_behavior(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    let app_handle = app.clone();
    let popup_window = popup.clone();

    popup.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(is_popup_focused) = event {
            if !should_hide_tray_panel_after_focus_change(
                TRAY_ICON_SELECTED.load(Ordering::SeqCst),
                *is_popup_focused,
            ) {
                return;
            }

            TRAY_ICON_SELECTED.store(false, Ordering::SeqCst);

            if !hide_popup(&popup_window) {
                return;
            }

            let Some(tray) = app_handle.tray_by_id(TRAY_ICON_ID) else {
                log::warn!("[tray-panel] tray icon missing when resetting popup focus state");
                return;
            };

            sync_tray_icon_selection(&tray, false);
        }
    });
}

/// macOS：配置窗口层级和集合行为，让面板在全屏空间中正常显示
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn setup_macos_panel(window: &tauri::WebviewWindow) {
    use cocoa::appkit::{NSWindow, NSWindowCollectionBehavior};
    use objc::sel;
    use objc::sel_impl;

    let _ = window.set_visible_on_all_workspaces(true);

    let ns_window = match window.ns_window() {
        Ok(ptr) => ptr as cocoa::base::id,
        Err(_) => return,
    };
    unsafe {
        // 窗口层级：101（popUpMenu），高于全屏 app 和菜单栏
        ns_window.setLevel_(101);

        // 面板失去焦点后应跟随系统菜单栏产品的默认行为自动收起
        let yes: cocoa::base::BOOL = cocoa::base::YES;
        let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: yes];

        // 集合行为：
        // - canJoinAllSpaces(1<<0): 所有桌面空间可见
        // - stationary(1<<4): 切换空间时不跟随移动
        // - ignoresCycle(1<<6): 不参与 Cmd+Tab 切换
        // - fullScreenAuxiliary(1<<8): 允许在全屏空间中显示
        let behavior = NSWindowCollectionBehavior::from_bits_truncate(
            (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8),
        );
        ns_window.setCollectionBehavior_(behavior);
    }
}

/// macOS：设置窗口失焦后是否自动隐藏（面板显示期间禁用，让菜单栏保持可见）
#[cfg(target_os = "macos")]
fn set_hides_on_deactivate(window: &tauri::WebviewWindow, hide: bool) -> Result<(), String> {
    use objc::sel;
    use objc::sel_impl;

    let ns_window = window
        .ns_window()
        .map_err(|e| format!("failed to get ns_window: {e}"))?
        as cocoa::base::id;

    unsafe {
        let flag: cocoa::base::BOOL = if hide {
            cocoa::base::YES
        } else {
            cocoa::base::NO
        };
        let _: () = objc::msg_send![ns_window, setHidesOnDeactivate: flag];
    }
    Ok(())
}
