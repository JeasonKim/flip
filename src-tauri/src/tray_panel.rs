#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPanelVisibilityChange {
    Show,
    Hide,
}

const MACOS_TRAY_PANEL_TOP_GAP: i32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayAnchorFrame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupFrameSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkAreaFrame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupOrigin {
    pub x: i32,
    pub y: i32,
}

pub fn resolve_tray_panel_visibility_change(is_tray_selected: bool) -> TrayPanelVisibilityChange {
    if is_tray_selected {
        TrayPanelVisibilityChange::Hide
    } else {
        TrayPanelVisibilityChange::Show
    }
}

pub fn should_hide_tray_panel_after_focus_change(
    is_tray_selected: bool,
    is_popup_focused: bool,
) -> bool {
    is_tray_selected && !is_popup_focused
}

pub fn calculate_macos_popup_origin(
    tray_anchor: TrayAnchorFrame,
    popup_size: PopupFrameSize,
    work_area: Option<WorkAreaFrame>,
) -> PopupOrigin {
    let centered_x = tray_anchor.x + (tray_anchor.width / 2) - (popup_size.width / 2);
    let below_tray_y = tray_anchor.y + tray_anchor.height + MACOS_TRAY_PANEL_TOP_GAP;

    if let Some(work_area) = work_area {
        let max_x = work_area.x + (work_area.width - popup_size.width).max(0);
        let max_y = work_area.y + (work_area.height - popup_size.height).max(0);

        return PopupOrigin {
            x: centered_x.clamp(work_area.x, max_x),
            y: below_tray_y.clamp(work_area.y, max_y),
        };
    }

    PopupOrigin {
        x: centered_x.max(0),
        y: below_tray_y.max(0),
    }
}

impl From<tauri::Rect> for TrayAnchorFrame {
    fn from(rect: tauri::Rect) -> Self {
        let position = rect.position.to_physical::<i32>(1.0);
        let size = rect.size.to_physical::<i32>(1.0);

        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }
}

impl From<tauri::PhysicalSize<u32>> for PopupFrameSize {
    fn from(size: tauri::PhysicalSize<u32>) -> Self {
        Self {
            width: size.width as i32,
            height: size.height as i32,
        }
    }
}

impl From<&tauri::PhysicalRect<i32, u32>> for WorkAreaFrame {
    fn from(work_area: &tauri::PhysicalRect<i32, u32>) -> Self {
        Self {
            x: work_area.position.x,
            y: work_area.position.y,
            width: work_area.size.width as i32,
            height: work_area.size.height as i32,
        }
    }
}

impl From<PopupOrigin> for tauri::PhysicalPosition<i32> {
    fn from(origin: PopupOrigin) -> Self {
        tauri::PhysicalPosition::new(origin.x, origin.y)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        calculate_macos_popup_origin, resolve_tray_panel_visibility_change,
        should_hide_tray_panel_after_focus_change, PopupFrameSize, PopupOrigin, TrayAnchorFrame,
        TrayPanelVisibilityChange, WorkAreaFrame,
    };

    #[test]
    fn 未选中托盘图标时应该展示面板() {
        assert_eq!(
            resolve_tray_panel_visibility_change(false),
            TrayPanelVisibilityChange::Show
        );
    }

    #[test]
    fn 已选中托盘图标时应该隐藏面板() {
        assert_eq!(
            resolve_tray_panel_visibility_change(true),
            TrayPanelVisibilityChange::Hide
        );
    }

    #[test]
    fn 面板失焦且托盘已选中时应该收起() {
        assert!(should_hide_tray_panel_after_focus_change(true, false));
    }

    #[test]
    fn 面板仍聚焦时不应该收起() {
        assert!(!should_hide_tray_panel_after_focus_change(true, true));
    }

    #[test]
    fn 托盘未选中时失焦不应该触发收起() {
        assert!(!should_hide_tray_panel_after_focus_change(false, false));
    }

    #[test]
    fn macos_面板应该展示在托盘图标下方并留出间距() {
        assert_eq!(
            calculate_macos_popup_origin(
                TrayAnchorFrame {
                    x: 1000,
                    y: 0,
                    width: 24,
                    height: 24,
                },
                PopupFrameSize {
                    width: 360,
                    height: 500,
                },
                None,
            ),
            PopupOrigin { x: 832, y: 32 }
        );
    }

    #[test]
    fn macos_面板超出工作区时应该约束回屏幕内() {
        assert_eq!(
            calculate_macos_popup_origin(
                TrayAnchorFrame {
                    x: 1410,
                    y: 0,
                    width: 24,
                    height: 24,
                },
                PopupFrameSize {
                    width: 360,
                    height: 500,
                },
                Some(WorkAreaFrame {
                    x: 0,
                    y: 24,
                    width: 1440,
                    height: 876,
                }),
            ),
            PopupOrigin { x: 1080, y: 32 }
        );
    }
}
