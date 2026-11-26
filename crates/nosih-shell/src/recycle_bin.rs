use std::mem::size_of;

use windows::Win32::{
    Foundation::HWND,
    UI::Shell::{
        Common::ITEMIDLIST, FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, SHCNE_CREATE, SHCNE_DELETE,
        SHCNE_RENAMEITEM, SHCNE_UPDATEDIR, SHCNE_UPDATEIMAGE, SHCNE_UPDATEITEM,
        SHCNRF_InterruptLevel, SHCNRF_ShellLevel, SHChangeNotifyDeregister, SHChangeNotifyEntry,
        SHChangeNotifyRegister, SHEmptyRecycleBinW, SHGetKnownFolderIDList, SHQUERYRBINFO,
        SHQueryRecycleBinW,
    },
};

pub const PARSING_NAME: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";

/// The two icon variants the dock asks the icon cache for. Their names are
/// part of the cache keys, so they stay as they were.
pub const FULL: &str = "llena";
pub const EMPTY: &str = "vacia";

/// Where Windows keeps the bin's icon for one state, as `(file, index)`.
///
/// The icon of the bin as a shell item is whatever Explorer shows *now*, and
/// right after the bin is emptied or filled Explorer can still show the
/// previous state. Asked for the "empty" variant at that moment, the dock
/// got the full icon and cached it (issue #15). Windows names both icons in
/// `DefaultIcon` (`Empty` and `Full`), where Explorer itself reads them: the
/// per-user copy first, which holds an icon the user picked in Desktop icon
/// settings, then the system default, then the stock icons.
#[must_use]
pub fn icon_location(full: bool) -> Option<(String, i32)> {
    use windows::{
        Win32::{
            System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
            UI::Shell::{
                SHGSI_ICONLOCATION, SHGetStockIconInfo, SHSTOCKICONINFO, SIID_RECYCLER,
                SIID_RECYCLERFULL,
            },
        },
        core::{PCWSTR, w},
    };

    let value = if full { w!("Full") } else { w!("Empty") };
    let keys: [(_, PCWSTR); 2] = [
        (
            HKEY_CURRENT_USER,
            w!(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\CLSID\{645FF040-5081-101B-9F08-00AA002F954E}\DefaultIcon"
            ),
        ),
        (
            HKEY_CLASSES_ROOT,
            w!(r"CLSID\{645FF040-5081-101B-9F08-00AA002F954E}\DefaultIcon"),
        ),
    ];
    for (root, key) in keys {
        let mut buffer = [0u16; 520];
        let mut bytes = size_of_val(&buffer) as u32;
        // SAFETY: `buffer` and `bytes` describe a valid buffer. `RRF_RT_REG_SZ`
        // also accepts `REG_EXPAND_SZ` and expands `%SystemRoot%`.
        let read = unsafe {
            RegGetValueW(
                root,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if read.is_ok() {
            let len = (bytes as usize / 2).saturating_sub(1);
            if let Some(location) = parse_icon_location(&String::from_utf16_lossy(&buffer[..len])) {
                return Some(location);
            }
        }
    }

    let mut stock = SHSTOCKICONINFO {
        cbSize: size_of::<SHSTOCKICONINFO>() as u32,
        ..Default::default()
    };
    let id = if full {
        SIID_RECYCLERFULL
    } else {
        SIID_RECYCLER
    };
    // SAFETY: `stock` has its size set; only the location is asked for.
    unsafe { SHGetStockIconInfo(id, SHGSI_ICONLOCATION, &mut stock) }.ok()?;
    let len = stock
        .szPath
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(stock.szPath.len());
    let path = String::from_utf16_lossy(&stock.szPath[..len]);
    (!path.is_empty()).then_some((path, stock.iIcon))
}

/// `C:\Windows\System32\imageres.dll,-55` → (`C:\...\imageres.dll`, -55). A
/// location without an index is the file's first icon.
fn parse_icon_location(text: &str) -> Option<(String, i32)> {
    let text = text.trim();
    let (path, index) = match text.rsplit_once(',') {
        Some((path, index)) => (path, index.trim().parse().ok()?),
        None => (text, 0),
    };
    let path = path.trim().trim_matches('"');
    (!path.is_empty()).then(|| (path.to_owned(), index))
}

#[must_use]
pub fn has_items() -> bool {
    let mut info = SHQUERYRBINFO {
        cbSize: size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: The structure has its size initialized; `None` means all drives.
    unsafe { SHQueryRecycleBinW(None, &mut info) }.is_ok() && info.i64NumItems > 0
}

/// Empties the recycle bin using the native confirmation and progress UI.
pub fn empty(owner: HWND) {
    // SAFETY: `owner` is an optional owner window; cancellation is handled by the shell.
    unsafe {
        let _ = SHEmptyRecycleBinW(Some(owner), None, 0);
    }
}

pub struct BinWatch(u32);

impl BinWatch {
    /// Sends `message` to `hwnd` whenever the recycle bin contents change.
    /// Notifications arrive in bursts, so receivers should coalesce them.
    #[must_use]
    pub fn register(hwnd: HWND, message: u32) -> Option<Self> {
        // SAFETY: The identifier list is valid for this registration call, which copies it.
        unsafe {
            let pidl: *mut ITEMIDLIST =
                SHGetKnownFolderIDList(&FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT.0 as u32, None)
                    .ok()?;
            let entry = SHChangeNotifyEntry {
                pidl,
                fRecursive: true.into(),
            };
            let events = SHCNE_CREATE
                | SHCNE_DELETE
                | SHCNE_RENAMEITEM
                | SHCNE_UPDATEDIR
                | SHCNE_UPDATEITEM
                | SHCNE_UPDATEIMAGE;
            let id = SHChangeNotifyRegister(
                hwnd,
                SHCNRF_ShellLevel | SHCNRF_InterruptLevel,
                events.0 as i32,
                message,
                1,
                &entry,
            );
            windows::Win32::System::Com::CoTaskMemFree(Some(pidl.cast()));
            (id != 0).then_some(Self(id))
        }
    }
}

impl Drop for BinWatch {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid registration returned by SHChangeNotifyRegister.
        unsafe {
            let _ = SHChangeNotifyDeregister(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_icon_location_should_split_file_and_resource_id() {
        assert_eq!(
            parse_icon_location(r"C:\Windows\System32\imageres.dll,-55"),
            Some((r"C:\Windows\System32\imageres.dll".to_owned(), -55))
        );
        assert_eq!(
            parse_icon_location(r#""C:\Icons\bin, full.ico" , 2"#),
            Some((r"C:\Icons\bin, full.ico".to_owned(), 2))
        );
        assert_eq!(
            parse_icon_location(r"C:\Icons\bin.ico"),
            Some((r"C:\Icons\bin.ico".to_owned(), 0))
        );
        assert_eq!(parse_icon_location(" , 3"), None);
    }

    /// Reads this PC's two bin icons: they must exist and differ.
    #[test]
    fn empty_and_full_icons_should_be_different_images() -> Result<(), crate::icons::IconError> {
        let (Some(empty), Some(full)) = (icon_location(false), icon_location(true)) else {
            panic!("Windows names no recycle bin icons");
        };
        assert_ne!(empty, full);
        let empty = crate::icons::load_icon_at(&empty.0, empty.1, 64)?;
        let full = crate::icons::load_icon_at(&full.0, full.1, 64)?;
        assert_eq!((empty.width, full.width), (64, 64));
        assert_ne!(empty.bgra, full.bgra);
        Ok(())
    }
}
