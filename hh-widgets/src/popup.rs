use crate::widget::Area;

/// Popup placement strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Anchor {
    #[default]
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

/// Popup rendering options.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PopupOptions {
    pub anchor: Anchor,
    pub clear_background: bool,
}

impl Default for PopupOptions {
    fn default() -> Self {
        Self {
            anchor: Anchor::TopLeft,
            clear_background: true,
        }
    }
}

/// Relative offset from anchor point to popup origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Offset {
    pub dx: i16,
    pub dy: i16,
}

/// Generic popup geometry request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PopupRequest {
    pub anchor_x: u16,
    pub anchor_y: u16,
    pub width: u16,
    pub height: u16,
    pub options: PopupOptions,
    pub offset: Offset,
}

impl Default for PopupRequest {
    fn default() -> Self {
        Self {
            anchor_x: 0,
            anchor_y: 0,
            width: 1,
            height: 1,
            options: PopupOptions::default(),
            offset: Offset::default(),
        }
    }
}

/// Compute popup geometry from an anchor point and request.
pub fn popup_from_request(request: PopupRequest, viewport: Area) -> Area {
    let mut x = i32::from(request.anchor_x);
    let mut y = i32::from(request.anchor_y);

    let width = request.width;
    let height = request.height;

    match request.options.anchor {
        Anchor::TopLeft => {}
        Anchor::TopRight => {
            x -= i32::from(width.saturating_sub(1));
        }
        Anchor::BottomLeft => {
            y -= i32::from(height.saturating_sub(1));
        }
        Anchor::BottomRight => {
            x -= i32::from(width.saturating_sub(1));
            y -= i32::from(height.saturating_sub(1));
        }
    }

    x += i32::from(request.offset.dx);
    y += i32::from(request.offset.dy);

    let desired = Area {
        x: if x < 0 {
            0
        } else {
            u16::try_from(x).unwrap_or(u16::MAX)
        },
        y: if y < 0 {
            0
        } else {
            u16::try_from(y).unwrap_or(u16::MAX)
        },
        width,
        height,
    };

    clamp_popup(desired, viewport)
}

/// Compute a clamped popup rectangle for the given viewport.
pub fn clamp_popup(mut desired: Area, viewport: Area) -> Area {
    if desired.width > viewport.width {
        desired.width = viewport.width;
    }
    if desired.height > viewport.height {
        desired.height = viewport.height;
    }

    let max_x = viewport
        .x
        .saturating_add(viewport.width.saturating_sub(desired.width));
    let max_y = viewport
        .y
        .saturating_add(viewport.height.saturating_sub(desired.height));
    desired.x = desired.x.clamp(viewport.x, max_x);
    desired.y = desired.y.clamp(viewport.y, max_y);
    desired
}

#[cfg(test)]
mod tests {
    use super::{Anchor, Offset, PopupOptions, PopupRequest, clamp_popup, popup_from_request};
    use crate::widget::Area;

    #[test]
    fn clamp_popup_keeps_area_inside_viewport() {
        let viewport = Area {
            x: 10,
            y: 5,
            width: 20,
            height: 8,
        };
        let desired = Area {
            x: 40,
            y: 40,
            width: 30,
            height: 20,
        };

        let clamped = clamp_popup(desired, viewport);
        assert_eq!(clamped.width, 20);
        assert_eq!(clamped.height, 8);
        assert!(clamped.x >= viewport.x);
        assert!(clamped.y >= viewport.y);
        assert!(clamped.x + clamped.width <= viewport.x + viewport.width);
        assert!(clamped.y + clamped.height <= viewport.y + viewport.height);
    }

    #[test]
    fn popup_from_request_handles_edges_and_small_terminals() {
        let viewport = Area {
            x: 0,
            y: 0,
            width: 12,
            height: 4,
        };

        let req = PopupRequest {
            anchor_x: 11,
            anchor_y: 3,
            width: 20,
            height: 10,
            options: PopupOptions {
                anchor: Anchor::BottomRight,
                clear_background: true,
            },
            offset: Offset::default(),
        };

        let popup = popup_from_request(req, viewport);
        assert_eq!(popup.x, 0);
        assert_eq!(popup.y, 0);
        assert_eq!(popup.width, 12);
        assert_eq!(popup.height, 4);
    }

    #[test]
    fn popup_from_request_applies_offsets() {
        let viewport = Area {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };

        let req = PopupRequest {
            anchor_x: 10,
            anchor_y: 6,
            width: 10,
            height: 3,
            options: PopupOptions {
                anchor: Anchor::TopLeft,
                clear_background: false,
            },
            offset: Offset { dx: 2, dy: -1 },
        };

        let popup = popup_from_request(req, viewport);
        assert_eq!(popup.x, 12);
        assert_eq!(popup.y, 5);
        assert_eq!(popup.width, 10);
        assert_eq!(popup.height, 3);
    }
}
