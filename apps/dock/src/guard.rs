//! Watchdog process: a minimal second `nosih.exe` that waits for the dock to
//! exit (normally, after a failure, or via End Task) and then restores the
//! Windows taskbar. Explicit recovery requests also replace that instance,
//! after a bounded graceful close and restoration of the desktop.

use std::{env, os::windows::process::CommandExt, process::Command, time::Duration};

use anyhow::{Context, Result};
use nosih_shell::{
    dock_link::DOCK_WINDOW_CLASS, power, system::set_window_transitions, taskbar,
    tracker::WindowTracker,
};
use windows::{
    Win32::{
        Foundation::{HANDLE, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM},
        System::Threading::{
            CreateEventW, EVENT_MODIFY_STATE, NORMAL_PRIORITY_CLASS, OpenEventW, OpenProcess,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
            TerminateProcess, WaitForMultipleObjects, WaitForSingleObject,
        },
        UI::WindowsAndMessaging::{
            FindWindowW, GetSystemMetrics, GetWindowThreadProcessId, PostMessageW, SM_SHUTTINGDOWN,
            SMTO_ABORTIFHUNG, SMTO_ERRORONEXIT, SendMessageTimeoutW, WM_CLOSE,
        },
    },
    core::{HSTRING, Owned},
};

pub const FLAG: &str = "--guard";

/// Starts without a console in its own process group so Ctrl+C in the dock's
/// terminal does not also terminate the watchdog.
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const HEALTH_INTERVAL_MS: u32 = 5_000;
const PING_TIMEOUT_MS: u32 = 250;
const MISSES_BEFORE_RESTORE: u8 = 4;
const CLOSE_TIMEOUT_MS: u32 = 8_000;

fn recovery_event(pid: u32) -> HSTRING {
    HSTRING::from(format!("Local\\Nosih.Recover.{pid}"))
}

fn ready_event(pid: u32) -> HSTRING {
    HSTRING::from(format!("Local\\Nosih.GuardReady.{pid}"))
}

pub struct Channels {
    _restart: Owned<HANDLE>,
    _ready: Owned<HANDLE>,
}

/// Signal the existing guard, even if the UI thread can no longer pump messages.
/// A per-process channel keeps an old guard from restarting a newer instance.
pub fn request_restart(pid: u32) -> Result<()> {
    // SAFETY: The event name is local to this session and contains only a PID.
    unsafe {
        let ready = Owned::new(OpenEventW(
            SYNCHRONIZATION_SYNCHRONIZE,
            false,
            &ready_event(pid),
        )?);
        // Only recovery waits for readiness, never ordinary startup. This also
        // covers device creation failing before the child guard has started.
        anyhow::ensure!(
            WaitForSingleObject(*ready, 2_000) == WAIT_OBJECT_0,
            "el guardián de Nosih no confirmó que esté preparado"
        );
        let event = Owned::new(OpenEventW(EVENT_MODIFY_STATE, false, &recovery_event(pid))?);
        SetEvent(*event)?;
    }
    Ok(())
}

pub fn recover_existing() -> Result<()> {
    // A second launch may arrive before the first has created its HWND. Wait
    // briefly in this disposable process, never in the running dock's UI.
    for _ in 0..10 {
        // SAFETY: Read-only lookup; no messages or pointers cross process boundaries.
        if let Ok(window) = unsafe { FindWindowW(DOCK_WINDOW_CLASS, None) } {
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(window, Some(&mut pid));
            }
            request_restart(pid).context("no se pudo recuperar la instancia de Nosih")?;
            nosih_shell::log::line(
                "segunda apertura: se solicita recuperar la instancia existente",
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("Nosih aún está iniciándose o cerrándose; vuelve a abrirlo en unos segundos")
}

#[derive(Default)]
struct Health {
    missed: u8,
    restored: bool,
}

impl Health {
    fn observe(&mut self, responsive: bool) -> bool {
        if responsive {
            self.missed = 0;
            self.restored = false;
            return false;
        }
        self.missed = self.missed.saturating_add(1);
        if self.missed >= MISSES_BEFORE_RESTORE && !self.restored {
            self.restored = true;
            return true;
        }
        false
    }
}

pub fn spawn() -> Result<Channels> {
    // SAFETY: This process owns the event until run_dock finishes; the guard
    // opens its own handle, so it survives UI cleanup and forced termination.
    let event = unsafe {
        Owned::new(CreateEventW(
            None,
            true,
            false,
            &recovery_event(std::process::id()),
        )?)
    };
    let ready = unsafe {
        Owned::new(CreateEventW(
            None,
            true,
            false,
            &ready_event(std::process::id()),
        )?)
    };
    let exe = env::current_exe().context("no se pudo localizar nosih.exe")?;
    Command::new(exe)
        .args([FLAG, &std::process::id().to_string()])
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .context("no se pudo lanzar el proceso guardián")?;
    Ok(Channels {
        _restart: event,
        _ready: ready,
    })
}

/// Watchdog body: restores the taskbar after exit or a sustained UI hang.
pub fn run(pid: u32) -> Result<()> {
    // The watchdog may wait for weeks, so it should not compete for CPU or memory.
    power::run_as_background_process();
    // A fast startup failure can end the dock before this child opens it.
    // The taskbar still needs to be restored in that case.
    let process = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) }
    {
        Ok(process) => unsafe { Owned::new(process) },
        Err(error) => {
            eprintln!("nosih: el guardián no pudo abrir el dock: {error}");
            restore_desktop();
            return Ok(());
        }
    };
    // SAFETY: The parent creates this event before launching its guard.
    let channels = (|| -> Result<_> {
        unsafe {
            let recovery = Owned::new(OpenEventW(
                SYNCHRONIZATION_SYNCHRONIZE,
                false,
                &recovery_event(pid),
            )?);
            let ready = Owned::new(OpenEventW(EVENT_MODIFY_STATE, false, &ready_event(pid))?);
            SetEvent(*ready)?;
            Ok(recovery)
        }
    })();
    let recovery = match channels {
        Ok(event) => event,
        Err(error) => {
            restore_desktop();
            return Err(error.context("no se pudo abrir el canal de recuperación"));
        }
    };
    let restart = match watch(pid, *process, *recovery) {
        Ok(restart) => restart,
        Err(error) => {
            nosih_shell::log::line(&format!("no se pudo supervisar Nosih: {error:#}"));
            false
        }
    };
    restore_desktop();
    drop(recovery);
    drop(process);
    // Never race the previous instance's mutex or the old guard's restoration.
    // A logoff/shutdown must not start a replacement during session teardown.
    if restart && unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } == 0 {
        let exe = env::current_exe().context("no se pudo localizar nosih.exe")?;
        Command::new(exe)
            .arg(crate::recovery::RECOVERED_FLAG)
            // The guard runs at IDLE priority; the replacement UI must not
            // inherit it or its animations/startup would be starved under load.
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | NORMAL_PRIORITY_CLASS.0)
            .spawn()
            .context("no se pudo reiniciar Nosih tras recuperar el escritorio")?;
        nosih_shell::log::line("escritorio recuperado; se inicia una interfaz gráfica nueva");
    }
    nosih_shell::log::flush();
    Ok(())
}

fn watch(pid: u32, process: HANDLE, recovery: HANDLE) -> Result<bool> {
    let mut health = Health::default();
    loop {
        // Reuse the existing health wait. A recovery request wakes immediately;
        // no new polling thread, timer or periodic allocation is needed.
        let wait =
            unsafe { WaitForMultipleObjects(&[process, recovery], false, HEALTH_INTERVAL_MS) };
        if wait == WAIT_OBJECT_0 {
            // The UI may exit immediately after signaling its device loss.
            return Ok(unsafe { WaitForSingleObject(recovery, 0) } == WAIT_OBJECT_0);
        }
        if wait.0 == WAIT_OBJECT_0.0 + 1 {
            nosih_shell::log::line("recuperación solicitada; se cierra la instancia anterior");
            return stop_dock(pid, process, CLOSE_TIMEOUT_MS);
        }
        if wait != WAIT_TIMEOUT {
            anyhow::bail!("el guardián no pudo esperar al dock: {wait:?}");
        }
        if health.observe(ui_responds(pid)) {
            nosih_shell::log::line(
                "la interfaz dejó de responder; se restaura la barra de Windows",
            );
            // Keep monitoring this process. An unrelated hung app must not
            // hold the guard in window enumeration while it repairs the shell.
            taskbar::give_back_on_exit();
            // Apps must not wait on a hung window for their overlay calls.
            nosih_shell::taskband::restore_after_exit();
        }
    }
}

/// Only the parent's process handle can be terminated. Try ordinary window
/// close first; a wedged renderer, destructor or Explorer worker has a deadline.
fn stop_dock(pid: u32, process: HANDLE, timeout: u32) -> Result<bool> {
    unsafe {
        if let Ok(window) = FindWindowW(DOCK_WINDOW_CLASS, None) {
            let mut owner = 0;
            GetWindowThreadProcessId(window, Some(&mut owner));
            if owner == pid {
                let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        match WaitForSingleObject(process, timeout) {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => {
                nosih_shell::log::line(
                    "cierre de recuperación agotó su plazo; se finaliza solo la instancia anterior de Nosih",
                );
                if let Err(error) = TerminateProcess(process, 1)
                    && WaitForSingleObject(process, 0) != WAIT_OBJECT_0
                {
                    return Err(error.into());
                }
                Ok(WaitForSingleObject(process, CLOSE_TIMEOUT_MS) == WAIT_OBJECT_0)
            }
            other => anyhow::bail!("no se pudo esperar el cierre del dock: {other:?}"),
        }
    }
}

fn ui_responds(pid: u32) -> bool {
    // SAFETY: The scalar-only health message runs on the dock's UI thread.
    // The timeout bounds an unresponsive peer; no COM call crosses threads.
    unsafe {
        let Ok(window) = FindWindowW(DOCK_WINDOW_CLASS, None) else {
            return false;
        };
        let mut owner = 0u32;
        GetWindowThreadProcessId(window, Some(&mut owner));
        if owner != pid {
            return false;
        }
        SendMessageTimeoutW(
            window,
            crate::app::HEALTH_CHECK,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT,
            PING_TIMEOUT_MS,
            None,
        )
        .0 != 0
    }
}

fn restore_desktop() {
    // If the dock exited cleanly, repeating its restoration is harmless.
    taskbar::give_back_on_exit();
    // And the taskbar's overlay calls, if the dock still held them.
    nosih_shell::taskband::restore_after_exit();
    // The dock turns off window transitions while it runs (genie effect); if
    // it died without restoring them, do it here.
    // The opt-in workspace mode can also have calmed windows on other desktops.
    for window in WindowTracker::default().list_with_workspaces(true) {
        set_window_transitions(window.hwnd, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::windows::io::AsRawHandle, process::Child};

    struct TestChild(Child);

    impl TestChild {
        fn spawn(mode: &str) -> Result<Self> {
            Ok(Self(
                Command::new(env::current_exe()?)
                    .args(["--exact", "guard::tests::recovery_child", "--nocapture"])
                    .env("NOSIH_RECOVERY_TEST_CHILD", mode)
                    .spawn()?,
            ))
        }

        fn handle(&self) -> HANDLE {
            HANDLE(self.0.as_raw_handle())
        }
    }

    impl Drop for TestChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn recovery_child() -> Result<()> {
        // Runs only in a dedicated child created by these tests, never the
        // user's dock or Explorer. No windows or shell settings are changed.
        let mode = env::var("NOSIH_RECOVERY_TEST_CHILD").unwrap_or_default();
        if mode == "blocked" {
            std::thread::sleep(Duration::from_secs(30));
        }
        if mode == "graphics-first" || mode == "graphics-repeat" {
            use windows::Win32::{
                Foundation::{D2DERR_RECREATE_TARGET, E_ACCESSDENIED},
                UI::WindowsAndMessaging::{MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, WM_QUIT},
            };
            let pid = std::process::id();
            let event =
                unsafe { Owned::new(CreateEventW(None, true, false, &recovery_event(pid))?) };
            let _ready = unsafe { Owned::new(CreateEventW(None, true, true, &ready_event(pid))?) };
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            crate::recovery::configure(mode == "graphics-repeat");
            crate::recovery::graphics_error(
                &windows::core::Error::from_hresult(E_ACCESSDENIED).into(),
            );
            assert!(!unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool());
            let error =
                anyhow::Error::from(windows::core::Error::from_hresult(D2DERR_RECREATE_TARGET))
                    .context("simulated drawing loss");
            crate::recovery::graphics_error(&error);
            let expected = if mode == "graphics-first" {
                WAIT_OBJECT_0
            } else {
                WAIT_TIMEOUT
            };
            assert_eq!(unsafe { WaitForSingleObject(*event, 0) }, expected);
            assert!(unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool());
            assert_eq!(message.message, WM_QUIT);
            // Errors bubbling up through several drawing/dispatch boundaries
            // must not enqueue another quit or request another recovery.
            crate::recovery::graphics_error(&error);
            assert!(!unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool());
        }
        Ok(())
    }

    #[test]
    fn device_loss_requests_recovery_but_a_repeated_loss_only_quits() -> Result<()> {
        for mode in ["graphics-first", "graphics-repeat"] {
            let mut child = TestChild::spawn(mode)?;
            assert_eq!(
                unsafe { WaitForSingleObject(child.handle(), 5_000) },
                WAIT_OBJECT_0
            );
            assert!(child.0.try_wait()?.is_some_and(|status| status.success()));
        }
        Ok(())
    }

    #[test]
    fn recovery_channel_wakes_without_a_responsive_ui() -> Result<()> {
        let pid = std::process::id();
        let restart = unsafe { Owned::new(CreateEventW(None, true, false, &recovery_event(pid))?) };
        let _ready = unsafe { Owned::new(CreateEventW(None, true, true, &ready_event(pid))?) };
        request_restart(pid)?;
        assert_eq!(unsafe { WaitForSingleObject(*restart, 0) }, WAIT_OBJECT_0);
        Ok(())
    }

    #[test]
    fn normal_exit_does_not_restart_but_a_simultaneous_recovery_request_does() -> Result<()> {
        let child = TestChild::spawn("exit")?;
        let request = unsafe { Owned::new(CreateEventW(None, true, false, None)?) };
        assert_eq!(
            unsafe { WaitForSingleObject(child.handle(), 5_000) },
            WAIT_OBJECT_0
        );
        assert!(!watch(child.0.id(), child.handle(), *request)?);
        unsafe { SetEvent(*request)? };
        assert!(watch(child.0.id(), child.handle(), *request)?);
        Ok(())
    }

    #[test]
    fn blocked_process_is_stopped_with_a_deadline_before_replacement() -> Result<()> {
        let child = TestChild::spawn("blocked")?;
        assert!(stop_dock(child.0.id(), child.handle(), 100)?);
        assert_eq!(
            unsafe { WaitForSingleObject(child.handle(), 0) },
            WAIT_OBJECT_0
        );
        Ok(())
    }

    #[test]
    fn intermittent_delay_does_not_restore_the_taskbar() {
        let mut health = Health::default();
        for _ in 0..3 {
            assert!(!health.observe(false));
        }
        assert!(!health.observe(true));
        for _ in 0..3 {
            assert!(!health.observe(false));
        }
    }

    #[test]
    fn sustained_hang_restores_once_until_recovery() {
        let mut health = Health::default();
        for _ in 0..3 {
            assert!(!health.observe(false));
        }
        assert!(health.observe(false));
        assert!(!health.observe(false));
        assert!(!health.observe(true));
        for _ in 0..3 {
            assert!(!health.observe(false));
        }
        assert!(health.observe(false));
    }
}
