//! Nosih: a macOS-style dock for Windows 11.
//!
//! Usage:
//! - `nosih`                     starts or recovers the dock
//! - `nosih --quit`              closes the running dock
//! - `nosih --restore`           restores the Windows taskbar
//! - `nosih --autostart on|off`  enables or disables startup with Windows
//! - `nosih --guard <pid>`       internal watchdog process
//! - `nosih --usage-worker`      internal one-shot quota reader
//! - `nosih --shell-worker`      internal one-shot shell action

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accessories;
mod app;
mod appearance;
mod assets;
mod bluetooth;
mod control_center;
mod drop;
mod edge_watch;
mod flight;
mod folder_menu;
mod gfx;
mod group_view;
mod guard;
mod mascot;
mod media;
mod menu;
mod menubar;
mod moments;
mod notifications;
mod multi_window_view;
mod overlays;
mod recovery;
mod scene;
mod shell_work;
mod strings;
mod thumbnails;
mod usage;

use std::env;

use anyhow::{Context, Result, bail};
use nosih_shell::{autostart, taskbar};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::{
            Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetErrorMode},
            Threading::CreateMutexW,
            WinRT::{
                CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
                DispatcherQueueOptions,
            },
        },
        UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
    },
    core::{Owned, w},
};

fn main() -> Result<()> {
    // Suppress system modal error dialogs (missing DLLs, missing ordinals in 3rd-party shell extensions).
    // SAFETY: Affects process-level error mode without side effects on memory or concurrency.
    unsafe {
        let _ = SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
    let args: Vec<String> = env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => run_dock(false),
        [recovery::RECOVERED_FLAG] => run_dock(true),
        ["--quit"] => {
            app::request_quit();
            Ok(())
        }
        ["--restore"] => {
            app::request_quit();
            taskbar::give_back();
            Ok(())
        }
        ["--autostart", "on"] => {
            let exe = env::current_exe().context("no se pudo localizar nosih.exe")?;
            Ok(autostart::set(Some(&exe.to_string_lossy()))?)
        }
        ["--autostart", "off"] => Ok(autostart::set(None)?),
        [guard::FLAG, pid] => guard::run(pid.parse().context("pid no válido")?),
        ["--usage-worker"] => usage::worker_main(),
        [accessories::FLAG] => accessories::worker_main(None),
        [accessories::FLAG, visibility] => accessories::worker_main(Some(visibility)),
        [accessories::USB_FLAG] => accessories::usb_worker_main(),
        [shell_work::FLAG] => shell_work::worker_main(),
        _ => bail!("uso: nosih [--quit | --restore | --autostart on|off]"),
    }
}

fn run_dock(recovered: bool) -> Result<()> {
    // SAFETY: The static name is valid; the handle is released when the process exits.
    let _single = unsafe { Owned::new(CreateMutexW(None, false, w!("Local\\Nosih.Dock"))?) };
    // SAFETY: Reads the thread's last error immediately after the call.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        guard::recover_existing()?;
        return Ok(());
    }

    recovery::configure(recovered);
    let startup = std::time::Instant::now();
    nosih_shell::log::line(concat!(
        "proceso iniciado, versión ",
        env!("CARGO_PKG_VERSION")
    ));
    // Keep the recovery channel alive through startup errors and final cleanup.
    let _recovery_event = guard::spawn()?;
    let result = initialize_dock();
    if let Err(error) = &result {
        recovery::graphics_error(error);
        nosih_shell::log::line(&format!(
            "el dock falló tras {} ms desde el inicio del proceso: {error:#}",
            startup.elapsed().as_millis()
        ));
    }
    taskbar::give_back_on_exit();
    nosih_shell::log::flush();
    result
}

fn initialize_dock() -> Result<()> {
    // SAFETY: Sets DPI awareness (if not already locked by the application manifest)
    // and initializes the dispatcher before creating windows.
    let _queue = unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // The compositor requires a dispatcher queue on its thread; this also
        // initializes COM, which icon extraction requires.
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })?
    };

    // Moves the `Run` autostart of 0.1.9-0.1.10 to the logon task, off the UI thread.
    let _ = std::thread::Builder::new()
        .name("nosih-autostart".into())
        .spawn(autostart::migrate);
    app::run()
}
