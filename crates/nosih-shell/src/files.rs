//! Shell file operations with the standard Windows progress, confirmation,
//! and undo behavior.

use thiserror::Error;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::Shell::{
            FO_COPY, FO_DELETE, FO_MOVE, FOF_ALLOWUNDO, SHFILEOPSTRUCTW, SHFileOperationW,
        },
    },
    core::PCWSTR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Cancelled,
}

#[derive(Debug, Error)]
#[error("la operación de archivos del shell falló (código {0})")]
pub struct FileError(i32);

/// Sends files to the recycle bin.
pub fn recycle(owner: HWND, files: &[String]) -> Result<Outcome, FileError> {
    run(owner, FO_DELETE, files, None)
}

/// Moves (or copies) files into `folder`.
pub fn transfer(
    owner: HWND,
    files: &[String],
    folder: &str,
    copy: bool,
) -> Result<Outcome, FileError> {
    run(
        owner,
        if copy { FO_COPY } else { FO_MOVE },
        files,
        Some(folder),
    )
}

/// `true` if both paths are on the same volume, where Explorer moves instead of copying.
#[must_use]
pub fn same_volume(a: &str, b: &str) -> bool {
    let root = |path: &str| path.get(..3).map(str::to_ascii_lowercase);
    root(a).is_some() && root(a) == root(b)
}

/// SHFileOperation takes lists of paths separated by and ending in two NULs.
fn path_list<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<u16> {
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.encode_utf16());
        list.push(0);
    }
    list.push(0);
    list
}

fn run(
    owner: HWND,
    operation: u32,
    files: &[String],
    destination: Option<&str>,
) -> Result<Outcome, FileError> {
    if files.is_empty() {
        return Ok(Outcome::Completed);
    }
    let from = path_list(files.iter().map(String::as_str));
    let to = destination.map(|folder| path_list([folder]));
    let mut request = SHFILEOPSTRUCTW {
        hwnd: owner,
        wFunc: operation,
        pFrom: PCWSTR(from.as_ptr()),
        pTo: to.as_ref().map_or(PCWSTR::null(), |to| PCWSTR(to.as_ptr())),
        fFlags: FOF_ALLOWUNDO.0 as u16,
        ..Default::default()
    };
    // SAFETY: The path buffers outlive the call. Preserve the native shell's
    // confirmations and progress UI, and distinguish cancellation from failure.
    let code = unsafe { SHFileOperationW(&mut request) };
    if request.fAnyOperationsAborted.as_bool() {
        Ok(Outcome::Cancelled)
    } else if code == 0 {
        Ok(Outcome::Completed)
    } else {
        Err(FileError(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_transfer_finishes_without_calling_the_shell() {
        assert_eq!(
            transfer(HWND::default(), &[], "", true).ok(),
            Some(Outcome::Completed)
        );
    }

    #[test]
    fn path_list_should_end_with_two_nuls() {
        assert_eq!(path_list(["a", "b"]), [97, 0, 98, 0, 0]);
    }

    #[test]
    fn same_volume_should_ignore_letter_case() {
        assert!(same_volume(r"c:\Users\x.txt", r"C:\Downloads"));
    }

    #[test]
    fn same_volume_should_detect_different_drives() {
        assert!(!same_volume(r"D:\x.txt", r"C:\Downloads"));
    }
}
