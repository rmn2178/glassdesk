use std::mem::size_of;

use serde::Serialize;
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        Threading::{GetCurrentProcess, GetProcessHandleCount, GetProcessTimes},
    },
};

#[derive(Serialize)]
pub struct Metrics {
    pub cpu_ms: f64,
    pub private_bytes: usize,
    pub peak_private_bytes: usize,
    pub handles: u32,
}

pub fn capture() -> windows::core::Result<Metrics> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let mut handles = 0;
    // SAFETY: All output buffers have the required size and the pseudo-handle
    // refers to this process; it does not need to be closed.
    unsafe {
        let process = GetCurrentProcess();
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user)?;
        GetProcessHandleCount(process, &mut handles)?;
        GetProcessMemoryInfo(process, (&raw mut memory).cast(), memory.cb)?;
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Ok(Metrics {
        cpu_ms: (ticks(kernel) + ticks(user)) as f64 / 10_000.0,
        private_bytes: memory.PrivateUsage,
        peak_private_bytes: memory.PeakPagefileUsage,
        handles,
    })
}
