//! The shortcuts folder's menu: the folder's contents as a native Windows
//! menu, with a submenu for each folder inside, like a Windows 10 taskbar
//! toolbar. Each submenu is listed only when it opens, so nesting costs
//! nothing; names and icons come from the shell, as in Explorer.

use std::{cell::RefCell, ffi::c_void, ptr};

use nosih_shell::{
    folder::{self, FolderItem},
    icons::IconImage,
    system,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
            HBITMAP,
        },
        UI::WindowsAndMessaging::{
            CreatePopupMenu, DestroyMenu, GetForegroundWindow, GetMenuItemCount, HMENU,
            InsertMenuItemW, MENUINFO, MENUITEMINFOW, MFS_DISABLED, MIIM_BITMAP, MIIM_ID,
            MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, MIM_MAXHEIGHT, PostMessageW,
            SetForegroundWindow, SetMenuInfo, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
            WM_NULL,
        },
    },
    core::{HSTRING, PWSTR},
};

use crate::{menu::Placement, strings::text};

/// Items listed per folder; past this, a huge folder would make its menu
/// slow to open and too long to use.
const MAX_ITEMS: usize = 500;
/// Longest name shown; longer ones end in "…".
const NAME_CHARS: usize = 64;

/// The menu being shown.
#[derive(Default)]
struct Open {
    /// Submenus not listed yet, with their folder.
    pending: Vec<(isize, String)>,
    /// What each item opens, by command id minus one.
    commands: Vec<String>,
    /// The items' icons, freed when the menu closes.
    bitmaps: Vec<HBITMAP>,
    /// Tallest each list may be, in pixels (see [`room`]); 0 for no limit.
    max_height: u32,
}

impl Drop for Open {
    fn drop(&mut self) {
        for bitmap in &self.bitmaps {
            // SAFETY: Our own bitmaps; the menu that drew them is destroyed.
            unsafe {
                let _ = DeleteObject((*bitmap).into());
            }
        }
    }
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// Shows the contents of `folder` as a menu on the `placement` side of
/// `anchor`, and returns the path of the item chosen (`None` if
/// dismissed). Folders inside are submenus and are never returned.
///
/// Runs a modal loop that keeps dispatching messages to the dock, whose
/// window procedure hands `WM_INITMENUPOPUP` to [`init_popup`]. The caller
/// must not hold the app state meanwhile.
pub fn show(
    owner: HWND,
    folder: &str,
    anchor: POINT,
    dark: bool,
    placement: Placement,
) -> Option<String> {
    crate::menu::apply_theme(dark);
    // SAFETY: The menu is destroyed below, with its submenus.
    let root = unsafe { CreatePopupMenu() }.ok()?;
    let max_height = system::monitor_at(anchor).map_or(0, |monitor| {
        let work = RECT {
            left: monitor.work_left,
            top: monitor.work_top,
            right: monitor.work_right,
            bottom: monitor.work_bottom,
        };
        room(placement, anchor, work)
    });
    OPEN.with_borrow_mut(|open| {
        *open = Some(Open {
            pending: Vec::new(),
            commands: Vec::new(),
            bitmaps: Vec::new(),
            max_height,
        });
    });
    limit_height(root);
    fill(root, folder);

    // SAFETY: Calls on our own menu and window; as for the dock's native
    // menu, the owner must be in front for a click outside to close it.
    let chosen = unsafe {
        let previous = GetForegroundWindow();
        let _ = SetForegroundWindow(owner);
        let chosen = TrackPopupMenuEx(
            root,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | placement.native_flags()).0,
            anchor.x,
            anchor.y,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(root);
        if chosen.0 == 0 {
            // Dismissing gives the focus back, as the dock's own menu does.
            let _ = SetForegroundWindow(previous);
        }
        chosen.0
    };
    let open = OPEN.with_borrow_mut(Option::take)?;
    usize::try_from(chosen)
        .ok()?
        .checked_sub(1)
        .and_then(|index| open.commands.get(index).cloned())
}

/// Lists a folder's submenu as it opens. `false` for any other menu (the
/// dock's native menu), which is left alone.
pub fn init_popup(menu: HMENU) -> bool {
    let folder = OPEN.with_borrow_mut(|open| {
        let open = open.as_mut()?;
        let index = open
            .pending
            .iter()
            .position(|(pending, _)| *pending == menu.0 as isize)?;
        Some(open.pending.swap_remove(index).1)
    });
    let Some(folder) = folder else {
        return false;
    };
    fill(menu, &folder);
    true
}

fn fill(menu: HMENU, folder: &str) {
    // Listing asks the shell for names and icons, which may dispatch
    // messages: the menu state is not borrowed meanwhile.
    match folder::list(folder) {
        Err(_) => add_note(menu, text().folder_unavailable),
        Ok(items) if items.is_empty() => add_note(menu, text().folder_empty),
        Ok(items) => {
            for item in items.into_iter().take(MAX_ITEMS) {
                add_item(menu, item);
            }
        }
    }
}

fn add_item(menu: HMENU, item: FolderItem) {
    let label = HSTRING::from(label_of(&item.name));
    let mut info = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_STRING | MIIM_ID,
        dwTypeData: PWSTR(label.as_ptr().cast_mut()),
        ..Default::default()
    };
    if let Some(bitmap) = item.icon.as_ref().and_then(bitmap_of) {
        info.fMask |= MIIM_BITMAP;
        info.hbmpItem = bitmap;
        OPEN.with_borrow_mut(|open| {
            if let Some(open) = open {
                open.bitmaps.push(bitmap);
            }
        });
    }
    if item.is_folder {
        // SAFETY: Destroyed with the menu it is attached to below.
        let Ok(submenu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        limit_height(submenu);
        info.fMask |= MIIM_SUBMENU;
        info.hSubMenu = submenu;
        OPEN.with_borrow_mut(|open| {
            if let Some(open) = open {
                open.pending.push((submenu.0 as isize, item.path));
            }
        });
    } else {
        info.wID = OPEN.with_borrow_mut(|open| {
            open.as_mut().map_or(0, |open| {
                open.commands.push(item.path);
                open.commands.len() as u32
            })
        });
    }
    append(menu, &info);
}

/// How tall a list may be: the space between the anchor and the far edge of
/// the work area, which already leaves out the menu bar and the dock.
///
/// Windows lets a long menu grow to the whole screen and gives it scroll
/// arrows at both ends, but the dock and the menu bar stay above it: they
/// covered those arrows and the last items could not be reached (issue #56).
/// Kept within this room, the arrows show and scroll the list on hover, as
/// in any Windows menu.
fn room(placement: Placement, anchor: POINT, work: RECT) -> u32 {
    let height = match placement {
        Placement::Above => anchor.y - work.top,
        Placement::Below => work.bottom - anchor.y,
        Placement::Left | Placement::Right => work.bottom - work.top,
    };
    u32::try_from(height).unwrap_or(0)
}

/// Applies [`room`] to `menu`, the root list or a folder's submenu.
fn limit_height(menu: HMENU) {
    let max_height = OPEN.with_borrow(|open| open.as_ref().map_or(0, |open| open.max_height));
    if max_height == 0 {
        return;
    }
    let info = MENUINFO {
        cbSize: size_of::<MENUINFO>() as u32,
        fMask: MIM_MAXHEIGHT,
        cyMax: max_height,
        ..Default::default()
    };
    // SAFETY: Our own menu; `info` lives for the call.
    unsafe {
        let _ = SetMenuInfo(menu, &info);
    }
}

/// A line that cannot be chosen: the folder is empty or unavailable.
fn add_note(menu: HMENU, note: &str) {
    let label = HSTRING::from(note);
    append(
        menu,
        &MENUITEMINFOW {
            cbSize: size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_STRING | MIIM_STATE,
            fState: MFS_DISABLED,
            dwTypeData: PWSTR(label.as_ptr().cast_mut()),
            ..Default::default()
        },
    );
}

fn append(menu: HMENU, info: &MENUITEMINFOW) {
    // SAFETY: `info` and the strings it points to live for the call, which
    // copies them into the menu.
    unsafe {
        let end = GetMenuItemCount(Some(menu)).max(0) as u32;
        let _ = InsertMenuItemW(menu, end, true, info);
    }
}

/// The name as a menu shows it: `&` is the access key marker, so it is
/// doubled, and a very long name is shortened.
fn label_of(name: &str) -> String {
    let shown: String = if name.chars().count() > NAME_CHARS {
        let mut short: String = name.chars().take(NAME_CHARS - 1).collect();
        short.push('…');
        short
    } else {
        name.to_owned()
    };
    shown.replace('&', "&&")
}

/// A menu draws a 32-bit top-down bitmap with premultiplied alpha blended,
/// which is what the icon already is.
fn bitmap_of(icon: &IconImage) -> Option<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: icon.width as i32,
            // A negative height specifies top-down rows.
            biHeight: -(icon.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = ptr::null_mut();
    // SAFETY: The section is created for exactly these dimensions; the copy
    // writes `width * height * 4` bytes, the size of both buffers.
    unsafe {
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let size = icon.width as usize * icon.height as usize * 4;
        if bits.is_null() || icon.bgra.len() != size {
            let _ = DeleteObject(bitmap.into());
            return None;
        }
        ptr::copy_nonoverlapping(icon.bgra.as_ptr(), bits.cast::<u8>(), size);
        Some(bitmap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_fit_between_the_menu_bar_and_the_dock() {
        let work = RECT {
            left: 0,
            top: 35,
            right: 1920,
            bottom: 987,
        };
        let anchor = |x, y| POINT { x, y };
        // Above an icon of a bottom dock: up to the menu bar.
        assert_eq!(room(Placement::Above, anchor(960, 980), work), 945);
        assert_eq!(room(Placement::Below, anchor(960, 100), work), 887);
        // Beside an icon of a side dock: the whole height between them.
        assert_eq!(room(Placement::Right, anchor(90, 500), work), 952);
        assert_eq!(room(Placement::Left, anchor(1830, 500), work), 952);
        // An anchor outside the work area leaves no room: no limit is set.
        assert_eq!(room(Placement::Above, anchor(960, 20), work), 0);
    }

    #[test]
    fn label_of_should_double_ampersands() {
        assert_eq!(label_of("Tom & Jerry"), "Tom && Jerry");
    }

    #[test]
    fn label_of_should_shorten_very_long_names() {
        let label = label_of(&"x".repeat(NAME_CHARS + 10));
        assert_eq!(label.chars().count(), NAME_CHARS);
        assert!(label.ends_with('…'));
    }
}
