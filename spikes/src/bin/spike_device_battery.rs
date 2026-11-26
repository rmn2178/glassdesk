//! Phase 11a compatibility probe. Does not integrate with or launch the dock.
//! Usage: spike_device_battery [--seconds 1..30] [--cycles 1..5]
//!        spike_device_battery --verify-timeout

#[path = "../device_battery/metrics.rs"]
mod metrics;
#[path = "../device_battery/model.rs"]
mod model;
#[path = "../device_battery/native.rs"]
mod native;
#[path = "../device_battery/watcher.rs"]
mod watcher;

use std::{
    env,
    io::Read,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Threading::{CREATE_NO_WINDOW, WaitForSingleObject},
        WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    },
};

const OUTPUT_LIMIT: u64 = 64 * 1024;

struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Balanced successful initialization on this thread.
        unsafe {
            RoUninitialize();
        }
    }
}

/// One isolated worker, bounded output, no per-device threads. The supervisor
/// uses a kernel wait instead of periodically polling the child or its CPU.
fn supervise(args: &[String], timeout: Duration) -> Result<Option<Vec<u8>>> {
    let mut worker = Worker(
        Command::new(env::current_exe()?)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW.0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let output = worker
        .0
        .stdout
        .take()
        .context("worker stdout unavailable")?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let handle = HANDLE(worker.0.as_raw_handle());
    // SAFETY: The child owns this live process handle until `worker` drops.
    let waited = unsafe { WaitForSingleObject(handle, timeout.as_millis() as u32) };
    if waited != WAIT_OBJECT_0 {
        let _ = worker.0.kill();
        // A failed driver must not turn cancellation itself into an unbounded wait.
        // SAFETY: The same live process handle.
        ensure!(
            unsafe { WaitForSingleObject(handle, 1000) } == WAIT_OBJECT_0,
            "worker termination did not complete within one second"
        );
    }
    let bytes = reader
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader failed"))??;
    ensure!(
        bytes.len() as u64 <= OUTPUT_LIMIT,
        "worker output exceeded 64 KiB"
    );
    if waited == WAIT_TIMEOUT {
        return Ok(None);
    }
    ensure!(
        waited == WAIT_OBJECT_0,
        "waiting for worker failed: {waited:?}"
    );
    ensure!(
        worker.0.try_wait()?.is_some_and(|status| status.success()),
        "probe failed: {}",
        String::from_utf8_lossy(&bytes)
    );
    Ok(Some(bytes))
}

#[derive(Serialize)]
struct Cycle {
    cached_query_ms: u128,
    devices: Vec<model::Device>,
    notifications: watcher::Observation,
    after_cleanup: metrics::Metrics,
}

fn probe(seconds: u64, cycles: u64) -> Result<()> {
    // SAFETY: This short-lived worker has no UI and initializes its own MTA.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
    let _apartment = Apartment;
    let baseline = metrics::capture()?;
    let started = Instant::now();
    let mut results = Vec::new();
    for _ in 0..cycles {
        let notifications = watcher::observe(seconds)?;
        let query = Instant::now();
        let mut nodes = native::snapshot()?;
        let cached_query_ms = query.elapsed().as_millis();
        // AEP describes the physical connection better than a service node.
        // Combine all matching transports before overriding the cache hint.
        for node in &mut nodes {
            let mut endpoints = notifications
                .endpoints
                .iter()
                .filter(|endpoint| endpoint.container.as_ref() == Some(&node.identity))
                .peekable();
            if endpoints.peek().is_some() {
                node.connected = model::connection(endpoints.map(|endpoint| endpoint.connected));
            }
        }
        results.push(Cycle {
            cached_query_ms,
            devices: model::merge(nodes),
            notifications,
            after_cleanup: metrics::capture()?,
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1, "scope": "cached Windows metadata; no device I/O; prototype only",
            "observation_seconds_per_cycle": seconds, "elapsed_ms": started.elapsed().as_millis(),
            "baseline": baseline, "cycles": results,
        }))?
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let child = args.first().is_some_and(|arg| arg == "--worker");
    if args.first().is_some_and(|arg| arg == "--stall-worker") {
        loop {
            thread::park();
        }
    }
    if args.as_slice() == ["--verify-timeout"] {
        let start = Instant::now();
        ensure!(
            supervise(&["--stall-worker".into()], Duration::from_millis(200))?.is_none(),
            "timeout was not enforced"
        );
        println!(
            "{{\"timeout_enforced\":true,\"elapsed_ms\":{}}}",
            start.elapsed().as_millis()
        );
        return Ok(());
    }
    let (mut seconds, mut cycles) = (5, 1);
    let mut options = args.iter().skip(usize::from(child));
    while let Some(arg) = options.next() {
        let value = options
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--seconds" => seconds = value.parse::<u64>()?,
            "--cycles" => cycles = value.parse::<u64>()?,
            _ => bail!("unknown argument: {arg}"),
        }
    }
    ensure!(
        (1..=30).contains(&seconds) && (1..=5).contains(&cycles) && seconds * cycles <= 40,
        "use 1..30 seconds and 1..5 cycles, with at most 40 seconds total observation"
    );
    if child {
        if let Err(error) = probe(seconds, cycles) {
            println!("{}", serde_json::json!({"error": format!("{error:#}")}));
            return Err(error);
        }
        return Ok(());
    }
    let command = [
        "--worker".into(),
        "--seconds".into(),
        seconds.to_string(),
        "--cycles".into(),
        cycles.to_string(),
    ];
    let bytes = supervise(&command, Duration::from_secs(seconds * cycles + 8))?
        .context("device probe timed out; worker terminated; dock unaffected")?;
    println!("{}", String::from_utf8(bytes)?);
    Ok(())
}
