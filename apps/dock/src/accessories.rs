//! Event-driven accessory reader. Slow Windows providers live in a disposable
//! process; the menu bar only reads an immutable cached snapshot.

#[cfg(test)]
mod tests;
mod usb;
mod visibility;
mod worker;

use std::{
    env,
    io::{BufRead, BufReader, Read},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context, Result, ensure};
use nosih_shell::accessories::{Snapshot, State};
use serde::{Deserialize, Serialize};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WPARAM},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::{RegisterWaitForSingleObject, UnregisterWaitEx, WT_EXECUTEONLYONCE},
    },
    UI::WindowsAndMessaging::PostMessageW,
};

pub const FLAG: &str = "--accessories-worker";
pub const USB_FLAG: &str = usb::FLAG;
pub fn usb_worker_main() -> Result<()> {
    usb::query_main()
}
const MAX_FRAME: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Frame {
    Busy,
    Ready(Snapshot),
    Failed,
}

pub fn worker_main(visibility: Option<&str>) -> Result<()> {
    if let Err(error) = worker::run(visibility) {
        let _ = worker::emit(&Frame::Failed);
        return Err(error);
    }
    Ok(())
}

struct Shared {
    visibility: visibility::Controller,
    snapshot: Mutex<Arc<Snapshot>>,
    child: Mutex<Option<Child>>,
    alive: AtomicBool,
    timed_out: AtomicBool,
    executable: PathBuf,
    window: isize,
    message: u32,
}

impl Shared {
    fn kill(&self) {
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            // TerminateProcess initiates termination; never wait on the UI thread.
            let _ = child.kill();
        }
    }
    fn publish(&self, value: Snapshot) {
        let changed = {
            let mut stored = self
                .snapshot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if **stored == value {
                false
            } else {
                *stored = Arc::new(value);
                true
            }
        };
        if changed && self.alive.load(Ordering::Acquire) {
            // SAFETY: Asynchronous scalar notification to the live owner.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(self.window as *mut _)),
                    self.message,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
    fn failed(&self) {
        let mut previous = (**self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
        .clone();
        previous.state = State::Unavailable;
        self.publish(previous);
    }
}

pub struct Monitor {
    shared: Arc<Shared>,
}
impl Monitor {
    pub fn new(window: isize, message: u32, visible: bool) -> Result<Self> {
        Self::start(window, message, env::current_exe()?, visible)
    }
    fn start(window: isize, message: u32, executable: PathBuf, visible: bool) -> Result<Self> {
        let shared = Arc::new(Shared {
            visibility: visibility::Controller::new(visible)?,
            snapshot: Mutex::new(Arc::new(Snapshot::default())),
            child: Mutex::new(None),
            alive: AtomicBool::new(true),
            timed_out: AtomicBool::new(false),
            executable,
            window,
            message,
        });
        let background = shared.clone();
        thread::Builder::new()
            .name("nosih-accessories".into())
            .spawn(move || {
                if background.alive.load(Ordering::Acquire) {
                    let _ = read_worker(&background);
                }
                background.kill();
                background
                    .child
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if background.alive.load(Ordering::Acquire) {
                    background.failed();
                }
            })?;
        Ok(Self { shared })
    }
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.shared
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
    pub fn set_visible(&self, visible: bool) {
        self.shared.visibility.set(visible);
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.shared.alive.store(false, Ordering::Release);
        self.shared.kill();
    }
}

struct Deadline {
    wait: HANDLE,
    _target: Arc<Shared>,
}
impl Deadline {
    fn arm(shared: Arc<Shared>, process: HANDLE) -> Result<Self> {
        Self::arm_after(shared, process, 5000)
    }
    fn arm_after(shared: Arc<Shared>, process: HANDLE, milliseconds: u32) -> Result<Self> {
        let target = shared;
        let mut wait = HANDLE::default();
        // SAFETY: Child handle and Arc context outlive registration. Revoked
        // on the reader thread before its child handle/context can be released.
        unsafe {
            RegisterWaitForSingleObject(
                &mut wait,
                process,
                Some(expired),
                Some(Arc::as_ptr(&target).cast()),
                milliseconds,
                WT_EXECUTEONLYONCE,
            )?;
        }
        Ok(Self {
            wait,
            _target: target,
        })
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        // SAFETY: Wait for a running callback on this background reader only.
        unsafe {
            let _ = UnregisterWaitEx(self.wait, Some(INVALID_HANDLE_VALUE));
        }
    }
}
unsafe extern "system" fn expired(context: *mut std::ffi::c_void, _: bool) {
    // SAFETY: The registered wait owns a reference until Deadline drops.
    let shared = unsafe { &*context.cast::<Shared>() };
    shared.timed_out.store(true, Ordering::Release);
    if shared.alive.load(Ordering::Acquire) {
        shared.failed();
    }
    shared.kill();
}

/// The kernel also cleans up an idle helper if the dock crashes or is killed.
/// Only the reader thread owns this non-inherited job handle.
struct WorkerJob(HANDLE);
impl WorkerJob {
    fn new() -> Result<Self> {
        // SAFETY: Own unnamed job; closing its sole handle terminates its children.
        unsafe {
            let job = Self(CreateJobObjectW(None, None)?);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(job)
        }
    }
}
impl Drop for WorkerJob {
    fn drop(&mut self) {
        // SAFETY: This handle is owned here and never inherited by the helper.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn read_worker(shared: &Arc<Shared>) -> Result<()> {
    let job = WorkerJob::new()?;
    let mut child = Command::new(&shared.executable)
        .arg(FLAG)
        .arg(&shared.visibility.name)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .context("accessory worker output unavailable")?;
    let process = HANDLE(child.as_raw_handle());
    *shared
        .child
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(child);
    // SAFETY: The child is live; failure is handled by the reader's cleanup.
    unsafe {
        AssignProcessToJobObject(job.0, process)?;
    }
    if !shared.alive.load(Ordering::Acquire) {
        shared.kill();
        return Ok(());
    }
    let mut deadline = Some(Deadline::arm(shared.clone(), process)?);
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = Vec::with_capacity(1024);
        let size = (&mut reader)
            .take(MAX_FRAME + 1)
            .read_until(b'\n', &mut line)?;
        ensure!(
            !shared.timed_out.load(Ordering::Acquire),
            "accessory query expired"
        );
        ensure!(
            size != 0 && size as u64 <= MAX_FRAME && line.last() == Some(&b'\n'),
            "invalid accessory frame"
        );
        match serde_json::from_slice::<Frame>(&line)? {
            Frame::Busy => {
                if deadline.is_none() {
                    deadline = Some(Deadline::arm(shared.clone(), process)?);
                }
            }
            Frame::Ready(snapshot) => {
                ensure!(
                    snapshot.state == State::Ready && snapshot.valid(),
                    "invalid accessory snapshot"
                );
                drop(deadline.take());
                // Revoking a wait can finish a timeout callback that raced with
                // the read above. Never resurrect its failed snapshot.
                ensure!(
                    !shared.timed_out.load(Ordering::Acquire),
                    "accessory query expired"
                );
                shared.publish(snapshot);
            }
            Frame::Failed => return Ok(()),
        }
        if !shared.alive.load(Ordering::Acquire) {
            return Ok(());
        }
    }
}
