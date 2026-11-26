//! Offline previews, on-demand originals. Only an explicit choice changes Windows.

mod cache;
mod catalog;

use catalog::entry;

use std::{
    env,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            CoUninitialize,
        },
        UI::{
            Shell::{DesktopWallpaper, IDesktopWallpaper},
            WindowsAndMessaging::{
                SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW,
            },
        },
    },
    core::{HSTRING, PCWSTR, PWSTR},
};

static APPLYING: AtomicBool = AtomicBool::new(false);

struct ApplyGuard;
impl ApplyGuard {
    fn acquire() -> Result<Self, String> {
        APPLYING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "A wallpaper change is already in progress".into())
    }
}
impl Drop for ApplyGuard {
    fn drop(&mut self) {
        APPLYING.store(false, Ordering::Release);
    }
}

struct Apartment(bool);
impl Apartment {
    fn new() -> Result<Self, String> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status.is_err() && status != RPC_E_CHANGED_MODE {
            return Err(status.to_string());
        }
        Ok(Self(status.is_ok()))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct Text(PWSTR);
impl Drop for Text {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
    }
}

fn with_desktop<T>(
    action: impl FnOnce(&IDesktopWallpaper) -> Result<T, String>,
) -> Result<T, String> {
    let _apartment = Apartment::new()?;
    // The interface is released before its apartment, on this same worker.
    let desktop: IDesktopWallpaper =
        unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
            .map_err(|error| error.to_string())?;
    action(&desktop)
}

fn cache_dir() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("Nosih").join("Wallpapers"))
        .ok_or_else(|| "The local application data folder is unavailable".into())
}

fn selected_path(root: &Path, current: &Path) -> Option<u8> {
    catalog::all()
        .ok()?
        .iter()
        .position(|asset| {
            root.join(&asset.name)
                .to_string_lossy()
                .eq_ignore_ascii_case(&current.to_string_lossy())
        })
        .map(|index| index as u8 + 1)
}

/// Read-only snapshot; an empty/multi-monitor/slideshow result is not a gallery selection.
#[tauri::command]
pub async fn wallpaper_selection() -> Result<Option<u8>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let root = cache_dir()?;
        with_desktop(|desktop| {
            let text = Text(
                unsafe { desktop.GetWallpaper(PCWSTR::null()) }
                    .map_err(|error| error.to_string())?,
            );
            if text.0.is_null() {
                return Ok(None);
            }
            let path = unsafe { text.0.to_string() }.map_err(|error| error.to_string())?;
            Ok(selected_path(&root, Path::new(&path)))
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

fn apply_at(
    id: u8,
    root: &Path,
    fetch: impl FnOnce(&catalog::Wallpaper, &mut std::fs::File) -> Result<(), String>,
    set: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let asset = entry(id)?;
    let path = cache::materialize(asset, root, fetch)?;
    set(&path)
}

/// Only accepts a catalog ID, never a URL, caller-supplied path or executable.
#[tauri::command]
pub async fn apply_wallpaper(id: u8) -> Result<(), String> {
    entry(id)?;
    let guard = ApplyGuard::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        apply_at(id, &cache_dir()?, cache::download, |path| {
            let path = HSTRING::from(path.as_os_str());
            // The gallery applies one image to the entire desktop. Use the
            // persisted system setting and its normal shell notification:
            // SetWallpaper(NULL) can return while DWM still samples the old
            // wallpaper. This runs on a blocking worker, never the UI thread.
            // SAFETY: SPI_SETDESKWALLPAPER reads the live, NUL-terminated path;
            // the Win32 signature uses PVOID but does not modify that string.
            unsafe {
                SystemParametersInfoW(
                    SPI_SETDESKWALLPAPER,
                    0,
                    Some(path.as_ptr().cast_mut().cast()),
                    SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
                )
            }
            .map_err(|error| error.to_string())
        })?;
        // The compositor's material keys do not include the wallpaper. Notify
        // only after the system operation, rather than waiting for a later
        // theme/colorization broadcast (which need not happen).
        nosih_shell::dock_link::notify_wallpaper_changed();
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests;
