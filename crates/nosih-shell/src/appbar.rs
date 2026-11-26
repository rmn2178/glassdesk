//! Registering the dock as an *AppBar* reserves a strip along a screen edge
//! (bottom, left or right for the dock, top for the menu bar) so maximized
//! windows end beside it, and notifies when an app enters or exits
//! full-screen mode.

use std::mem::size_of;

use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::Shell::{
        ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
        APPBARDATA, SHAppBarMessage,
    },
};

pub const ABN_FULLSCREENAPP: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The strip `thickness` pixels deep along `edge` of `screen`.
fn strip(edge: Edge, screen: RECT, thickness: i32) -> RECT {
    match edge {
        Edge::Top => RECT {
            bottom: screen.top + thickness,
            ..screen
        },
        Edge::Bottom => RECT {
            top: screen.bottom - thickness,
            ..screen
        },
        Edge::Left => RECT {
            right: screen.left + thickness,
            ..screen
        },
        Edge::Right => RECT {
            left: screen.right - thickness,
            ..screen
        },
    }
}

/// `rect` given back its depth from the edge: another bar on the same edge
/// (the Windows taskbar before it hides) pushes it inward.
fn keep_thickness(edge: Edge, mut rect: RECT, thickness: i32) -> RECT {
    match edge {
        Edge::Top => rect.bottom = rect.top + thickness,
        Edge::Bottom => rect.top = rect.bottom - thickness,
        Edge::Left => rect.right = rect.left + thickness,
        Edge::Right => rect.left = rect.right - thickness,
    }
    rect
}

/// The system granted the strip: right edge, right depth. Along the edge it
/// may be shorter (a side bar ends at the menu bar on top), which is fine;
/// demanding the full length would retry, and notify, forever.
fn granted(edge: Edge, got: RECT, wanted: RECT) -> bool {
    match edge {
        Edge::Top | Edge::Bottom => got.top == wanted.top && got.bottom == wanted.bottom,
        Edge::Left | Edge::Right => got.left == wanted.left && got.right == wanted.right,
    }
}

pub struct AppBar {
    hwnd: HWND,
    reserved: Option<RECT>,
}

impl AppBar {
    /// Registers `hwnd` as an AppBar. Notifications are sent to that window
    /// using `callback`, with the notification code in `wParam`.
    #[must_use]
    pub fn register(hwnd: HWND, callback: u32) -> Option<Self> {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: callback,
            ..Default::default()
        };
        // SAFETY: `data` is a valid APPBARDATA with its size initialized.
        let registered = unsafe { SHAppBarMessage(ABM_NEW, &mut data) } != 0;
        registered.then_some(Self {
            hwnd,
            reserved: None,
        })
    }

    /// Reserves a strip `thickness` pixels deep along `edge` of monitor
    /// `screen`. Calls the system only when the strip changes because each
    /// reservation triggers a settings-change notification; repeating it would
    /// create a loop.
    pub fn reserve(&mut self, edge: Edge, screen: RECT, thickness: i32) {
        let wanted = strip(edge, screen, thickness);
        if self.reserved == Some(wanted) {
            return;
        }

        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uEdge: match edge {
                Edge::Top => ABE_TOP,
                Edge::Bottom => ABE_BOTTOM,
                Edge::Left => ABE_LEFT,
                Edge::Right => ABE_RIGHT,
            },
            rc: wanted,
            ..Default::default()
        };
        // SAFETY: `data` has its size initialized and remains valid for both synchronous calls.
        unsafe {
            SHAppBarMessage(ABM_QUERYPOS, &mut data);
            // Another bar on the same edge (the Windows taskbar, before it is
            // hidden) pushes the strip inward, so preserve its thickness.
            data.rc = keep_thickness(edge, data.rc, thickness);
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        // Treat the reservation as complete only if the system granted the
        // requested area; otherwise, retry on the next call.
        self.reserved = granted(edge, data.rc, wanted).then_some(wanted);
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            ..Default::default()
        };
        // SAFETY: `data` has its size initialized and remains valid for the synchronous call.
        unsafe { SHAppBarMessage(ABM_REMOVE, &mut data) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn each_edge_reserves_its_own_strip() {
        assert_eq!(strip(Edge::Bottom, SCREEN, 90).top, 990);
        assert_eq!(strip(Edge::Top, SCREEN, 35).bottom, 35);
        let left = strip(Edge::Left, SCREEN, 90);
        assert_eq!(
            (left.left, left.right, left.top, left.bottom),
            (0, 90, 0, 1080)
        );
        let right = strip(Edge::Right, SCREEN, 90);
        assert_eq!((right.left, right.right), (1830, 1920));
    }

    #[test]
    fn a_side_strip_shortened_by_the_menu_bar_still_counts_as_granted() {
        let wanted = strip(Edge::Left, SCREEN, 90);
        let below_bar = RECT { top: 35, ..wanted };
        assert!(granted(Edge::Left, below_bar, wanted));
        let pushed = RECT {
            left: 48,
            right: 138,
            ..wanted
        };
        assert!(!granted(Edge::Left, pushed, wanted));
        assert_eq!(keep_thickness(Edge::Left, pushed, 90).right, 138);
        assert_eq!(keep_thickness(Edge::Right, wanted, 60).left, 30);
    }
}
