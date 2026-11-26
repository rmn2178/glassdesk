//! Human-readable names for apps or shell items, used in dock labels.

use std::{ffi::c_void, path::Path};

use windows::{
    Win32::{
        Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        System::Com::CoTaskMemFree,
        UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY},
    },
    core::{HSTRING, PCWSTR, w},
};

/// Display name: the executable description (for example, "Visual Studio Code"),
/// otherwise the shell name (for example, "Recycle Bin"), or finally the file
/// name without its extension.
///
/// Requires COM to be initialized on the calling thread.
#[must_use]
pub fn display_name(parsing_name: &str) -> String {
    file_description(parsing_name)
        .or_else(|| shell_name(parsing_name))
        .map(|name| name.trim().trim_end_matches(".exe").to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            Path::new(parsing_name).file_stem().map_or_else(
                || parsing_name.to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            )
        })
}

fn file_description(path: &str) -> Option<String> {
    let path = HSTRING::from(path);
    // SAFETY: The buffer has the size requested by the API, and pointers
    // returned by VerQueryValueW refer to it while it remains alive.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;

        let mut value: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let found = VerQueryValueW(
            data.as_ptr().cast(),
            w!("\\VarFileInfo\\Translation"),
            &mut value,
            &mut len,
        );
        if !found.as_bool() || len < 4 {
            return None;
        }
        // Each translation is a pair of 16-bit language and code-page values.
        let pair = std::slice::from_raw_parts(value.cast::<u16>(), 2);
        let key = HSTRING::from(format!(
            "\\StringFileInfo\\{:04x}{:04x}\\FileDescription",
            pair[0], pair[1]
        ));

        let found = VerQueryValueW(data.as_ptr().cast(), &key, &mut value, &mut len);
        if !found.as_bool() || len == 0 {
            return None;
        }
        PCWSTR(value.cast()).to_string().ok()
    }
}

fn shell_name(parsing_name: &str) -> Option<String> {
    // SAFETY: The shell-returned string is copied and then freed with
    // the allocator that created it.
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(&HSTRING::from(parsing_name), None).ok()?;
        let raw = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let name = raw.to_string().ok();
        CoTaskMemFree(Some(raw.as_ptr().cast()));
        name
    }
}
