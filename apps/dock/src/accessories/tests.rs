use super::*;
use std::time::{Duration, Instant};
use windows::{
    Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    },
    core::Owned,
};

fn ready_monitor() -> Result<(Monitor, Owned<HANDLE>)> {
    // Explicitly provided executable: never launch a full dock in unit tests.
    let path = PathBuf::from(
        env::var_os("NOSIH_ACCESSORY_TEST_EXE")
            .context("Build nosih and set NOSIH_ACCESSORY_TEST_EXE to its absolute path")?,
    );
    ensure!(
        path.is_absolute() && path.is_file(),
        "test executable missing"
    );
    let monitor = Monitor::start(0, 0, path, true)?;
    let started = Instant::now();
    while monitor.snapshot().state == State::Loading && started.elapsed() < Duration::from_secs(7) {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(monitor.snapshot().state, State::Ready);
    let child = monitor
        .shared
        .child
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let pid = child.as_ref().context("reader exited before test")?.id();
    // SAFETY: Own synchronization-only handle, valid even after reader cleanup.
    let process = unsafe { Owned::new(OpenProcess(PROCESS_SYNCHRONIZE, false, pid)?) };
    drop(child);
    Ok((monitor, process))
}

fn wait_for_reader_cleanup(shared: &Arc<Shared>) {
    let started = Instant::now();
    while Arc::strong_count(shared) > 1 && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        Arc::strong_count(shared),
        1,
        "reader or wait callback retained its context"
    );
}

#[test]
#[ignore = "Requires NOSIH_ACCESSORY_TEST_EXE pointing to a built native worker"]
fn monitor_repeated_shutdown_and_timeout_are_bounded() -> Result<()> {
    for _ in 0..3 {
        let (monitor, process) = ready_monitor()?;
        let shared = monitor.shared.clone();
        let started = Instant::now();
        drop(monitor);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "UI shutdown blocked"
        );
        assert_eq!(
            unsafe { WaitForSingleObject(*process, 1000) },
            WAIT_OBJECT_0
        );
        wait_for_reader_cleanup(&shared);
    }
    let (monitor, process) = ready_monitor()?;
    // Force the same kernel callback used when a provider exceeds its deadline.
    let timeout = Deadline::arm_after(monitor.shared.clone(), *process, 20)?;
    assert_eq!(
        unsafe { WaitForSingleObject(*process, 1000) },
        WAIT_OBJECT_0
    );
    drop(timeout);
    assert!(monitor.shared.timed_out.load(Ordering::Acquire));
    assert_eq!(monitor.snapshot().state, State::Unavailable);
    let shared = monitor.shared.clone();
    drop(monitor);
    wait_for_reader_cleanup(&shared);
    Ok(())
}

#[test]
fn worker_protocol_rejects_unrecognized_events_and_truncated_payloads() {
    assert!(
        serde_json::from_str::<Frame>(r#"{"event":"ready","data":{"state":"ready","devices":[]}}"#)
            .is_ok()
    );
    assert!(serde_json::from_str::<Frame>(r#"{"event":"unexpected"}"#).is_err());
    assert!(
        serde_json::from_str::<Frame>(r#"{"event":"ready","data":{"state":"ready","devices":["#)
            .is_err()
    );
}
