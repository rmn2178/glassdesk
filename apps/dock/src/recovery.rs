//! Device-loss recovery shares the watchdog's restart path with a second launch.
//! Recreate the whole UI, including cached surfaces and secondary compositors.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::D2DERR_RECREATE_TARGET,
    Graphics::Dxgi::{
        DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
        DXGI_ERROR_DRIVER_INTERNAL_ERROR,
    },
    UI::WindowsAndMessaging::PostQuitMessage,
};

use crate::guard;

pub const RECOVERED_FLAG: &str = "--recovered";
const STABLE_FOR: Duration = Duration::from_secs(60);
static START: OnceLock<(bool, Instant)> = OnceLock::new();
static STOPPING: AtomicBool = AtomicBool::new(false);

pub fn configure(recovered: bool) {
    let _ = START.set((recovered, Instant::now()));
}

fn can_restart(recovered: bool, uptime: Duration) -> bool {
    !recovered || uptime >= STABLE_FOR
}

fn device_lost(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<windows::core::Error>()
            .is_some_and(|error| {
                matches!(
                    error.code(),
                    D2DERR_RECREATE_TARGET
                        | DXGI_ERROR_DEVICE_REMOVED
                        | DXGI_ERROR_DEVICE_RESET
                        | DXGI_ERROR_DEVICE_HUNG
                        | DXGI_ERROR_DRIVER_INTERNAL_ERROR
                )
            })
    })
}

/// Called at both drawing and dispatch boundaries: optional icons can swallow
/// drawing errors, but a lost device invalidates all of its cached surfaces.
pub fn graphics_error(error: &anyhow::Error) {
    let Some((recovered, started)) = START.get() else {
        return;
    };
    if !device_lost(error) || STOPPING.swap(true, Ordering::Relaxed) {
        return;
    }
    nosih_shell::log::line(&format!("dispositivo gráfico perdido: {error:#}"));
    if can_restart(*recovered, started.elapsed()) {
        if let Err(error) = guard::request_restart(std::process::id()) {
            nosih_shell::log::line(&format!("no se pudo pedir recuperación: {error:#}"));
        }
    } else {
        nosih_shell::log::line(
            "fallo gráfico repetido antes de 60 s; se detiene Nosih y se devuelve la barra de Windows",
        );
    }
    // SAFETY: Drawing and UI dispatch run on the dock's UI thread. Wake its
    // loop without destroying windows inside a drawing/OLE callback.
    unsafe { PostQuitMessage(0) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{Win32::Foundation::E_ACCESSDENIED, core::Error};

    #[test]
    fn recognizes_device_loss_through_context_without_matching_unrelated_errors() {
        for code in [
            D2DERR_RECREATE_TARGET,
            DXGI_ERROR_DEVICE_REMOVED,
            DXGI_ERROR_DEVICE_RESET,
            DXGI_ERROR_DEVICE_HUNG,
            DXGI_ERROR_DRIVER_INTERNAL_ERROR,
        ] {
            let error = anyhow::Error::from(Error::from_hresult(code)).context("surface");
            assert!(device_lost(&error));
        }
        assert!(!device_lost(&Error::from_hresult(E_ACCESSDENIED).into()));
        assert!(!device_lost(&anyhow::anyhow!("DXGI_ERROR_DEVICE_REMOVED")));
    }

    #[test]
    fn repeated_failure_requires_a_minute_of_stable_uptime() {
        assert!(can_restart(false, Duration::ZERO));
        assert!(!can_restart(true, Duration::from_secs(59)));
        assert!(can_restart(true, Duration::from_secs(60)));
    }
}
