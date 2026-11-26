//! Measures the memory, handle, and time cost of each shell call the dock makes
//! at startup. Usage: `cargo run --release -p spikes --bin spike_shellcost`

use std::{mem::size_of, time::Instant};

use nosih_shell::{recycle_bin, system};
use windows::Win32::{
    System::{
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::{GetCurrentProcess, GetProcessHandleCount},
        WinRT::{
            CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT,
            DispatcherQueueOptions,
        },
    },
    UI::WindowsAndMessaging::{CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE},
};
use windows::core::w;

fn report(stage: &str, started: Instant) {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let mut handles = 0u32;
    // SAFETY: Queries target this process and use correctly sized structures.
    unsafe {
        let _ = GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        );
        let _ = GetProcessHandleCount(GetCurrentProcess(), &mut handles);
    }
    println!(
        "{stage:<34} privada {:>5.1} MB  handles {handles:>4}  {:>7.1} ms",
        counters.PrivateUsage as f64 / 1_048_576.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
}

fn main() -> windows::core::Result<()> {
    let _queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })?
    };
    report("base", Instant::now());
    let t = Instant::now();
    let _ = system::downloads_folder();
    report("downloads_folder", t);
    let t = Instant::now();
    let _ = recycle_bin::has_items();
    report("recycle_bin::has_items", t);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )?
    };
    let t = Instant::now();
    let watch = recycle_bin::BinWatch::register(hwnd, 0x8003);
    report("BinWatch::register", t);
    drop(watch);
    Ok(())
}
