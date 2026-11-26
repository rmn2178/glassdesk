//! Slow shell actions run in disposable processes. File operations already
//! started can finish even if the dock exits; no shell extension runs on its UI.

use std::{
    env,
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        mpsc::{SyncSender, sync_channel},
    },
    thread,
};

use anyhow::{Context, Result, bail};
use nosih_shell::{
    files, jumplist, launcher,
    pins::{self, unpin_from_taskbar},
    recycle_bin, tracker,
};
use serde::{Deserialize, Serialize};
use windows::Win32::{
    Foundation::{HANDLE, HWND, WAIT_TIMEOUT},
    System::{
        Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
        Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetErrorMode},
        Threading::WaitForSingleObject,
    },
    UI::WindowsAndMessaging::AllowSetForegroundWindow,
};

pub const FLAG: &str = "--shell-worker";
const MAX_REQUEST_BYTES: u64 = 4 * 1024 * 1024;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Open {
        id: String,
    },
    OpenAsAdmin {
        id: String,
    },
    Reveal {
        path: String,
    },
    OpenWith {
        app: String,
        files: Vec<String>,
    },
    Recycle {
        files: Vec<String>,
    },
    Transfer {
        folder: String,
        files: Vec<String>,
        copy: bool,
    },
    EmptyBin,
    Unpin {
        id: String,
    },
    /// A jump list entry of the dock item `program` (see `jumplist::open`).
    OpenJump {
        target: jumplist::Target,
        program: String,
    },
}

impl Action {
    fn launch_timeout_ms(&self) -> Option<u32> {
        match self {
            Self::Open { .. }
            | Self::OpenWith { .. }
            | Self::Reveal { .. }
            | Self::OpenJump { .. } => Some(30_000),
            // File progress and UAC can legitimately wait for a user decision.
            _ => None,
        }
    }

    fn modifies_files(&self) -> bool {
        matches!(
            self,
            Self::Recycle { .. } | Self::Transfer { .. } | Self::EmptyBin
        )
    }

    fn run(self) -> Result<()> {
        // The helper owns the native progress/confirmation windows. Making
        // the dock their owner would disable its input during long operations.
        let owner = HWND::default();
        match self {
            // Through its shortcut when it has one with arguments (issue #8).
            Self::Open { id } => {
                launcher::open(&nosih_shell::pins::launch_target(&id).unwrap_or(id))?
            }
            Self::OpenAsAdmin { id } => {
                launcher::open_as_admin(&nosih_shell::pins::launch_target(&id).unwrap_or(id))?;
            }
            Self::Reveal { path } => launcher::reveal(&path)?,
            Self::OpenWith { app, files } => launcher::open_with(&app, &files)?,
            Self::Recycle { files: paths } => {
                files::recycle(owner, &paths)?;
            }
            Self::Transfer {
                folder,
                files: paths,
                copy,
            } => {
                files::transfer(owner, &paths, &folder, copy)?;
            }
            Self::EmptyBin => recycle_bin::empty(owner),
            Self::Unpin { id } => {
                let _ = unpin_from_taskbar(&id);
            }
            Self::OpenJump { target, program } => {
                // A pinned shortcut stands for the program it opens.
                let program = if tracker::is_shortcut(&program) {
                    pins::link_target(Path::new(&program))
                } else {
                    Some(program)
                };
                jumplist::open(&target, program.as_deref())?;
            }
        }
        Ok(())
    }
}

/// Fixed worker count and bounded queue; sending never waits for a shell job.
struct Queue<T> {
    sender: SyncSender<T>,
}

impl<T: Send + 'static> Queue<T> {
    fn new(
        workers: usize,
        capacity: usize,
        run: impl Fn(T) + Send + Sync + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = sync_channel::<T>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let run = Arc::new(run);
        for index in 0..workers {
            let receiver = Arc::clone(&receiver);
            let run = Arc::clone(&run);
            thread::Builder::new()
                .name(format!("nosih-shell-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let Ok(receiver) = receiver.lock() else { break };
                            receiver.recv()
                        };
                        let Ok(job) = job else { break };
                        // No shell call or process wait holds the queue lock.
                        run(job);
                    }
                })?;
        }
        Ok(Self { sender })
    }

    fn submit(&self, job: T) -> Result<()> {
        self.sender
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("la cola del shell está llena o no está disponible"))
    }
}

struct Workers {
    launches: Queue<Action>,
    files: Queue<Action>,
}

impl Workers {
    fn new() -> Result<Self> {
        let execute = |action| {
            if let Err(error) = start_helper(action) {
                eprintln!("nosih: operación del shell: {error:#}");
            }
        };
        Ok(Self {
            launches: Queue::new(2, 16, execute)?,
            // Preserve file-operation order and avoid competing transfers.
            files: Queue::new(1, 4, execute)?,
        })
    }
}

pub fn dispatch(action: Action) -> Result<()> {
    static WORKERS: OnceLock<Result<Workers, String>> = OnceLock::new();
    let workers = WORKERS.get_or_init(|| Workers::new().map_err(|error| error.to_string()));
    let workers = workers
        .as_ref()
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    if action.modifies_files() {
        workers.files.submit(action)
    } else {
        workers.launches.submit(action)
    }
}

/// A rejected request must be visible to the user, without blocking the UI
/// or creating a new dialog for every repeated click on a full queue.
pub fn notify_not_started() {
    notify_failure(crate::strings::text().shell_operation_failed);
}

pub fn notify_end_task_failed() {
    notify_failure(crate::strings::text().end_task_failed);
}

fn notify_failure(message: &'static str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};
    static SHOWING: AtomicBool = AtomicBool::new(false);
    if SHOWING.swap(true, Ordering::AcqRel) {
        return;
    }
    let shown = thread::Builder::new()
        .name("nosih-shell-notice".into())
        .spawn(move || {
            // SAFETY: Independent native dialog; it never disables the dock.
            unsafe {
                MessageBoxW(
                    None,
                    &windows::core::HSTRING::from(message),
                    windows::core::w!("Nosih"),
                    MB_OK | MB_ICONWARNING,
                );
            }
            SHOWING.store(false, Ordering::Release);
        });
    if shown.is_err() {
        SHOWING.store(false, Ordering::Release);
    }
}

fn start_helper(action: Action) -> Result<()> {
    let timeout = action.launch_timeout_ms();
    let input = serde_json::to_vec(&action)?;
    if input.len() as u64 > MAX_REQUEST_BYTES {
        bail!("la operación contiene demasiados datos");
    }
    let mut child = Command::new(env::current_exe()?)
        .arg(FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        // A launched app may inherit a standard handle. Waiting for a stderr
        // pipe to close could then hold this slot for that app's entire life.
        .stderr(Stdio::inherit())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("no se pudo iniciar el auxiliar del shell")?;
    // SAFETY: Give this child the foreground right granted by the dock click.
    unsafe {
        let _ = AllowSetForegroundWindow(child.id());
    }
    if let Some(mut stdin) = child.stdin.take() {
        // Closing the pipe after writing is the helper's end-of-request marker.
        stdin.write_all(&input)?;
    }
    let timed_out = timeout.is_some_and(|milliseconds| {
        // SAFETY: `child` owns this process handle throughout the bounded wait.
        unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), milliseconds) == WAIT_TIMEOUT }
    });
    if timed_out {
        // Ordinary activation must not permanently consume both slots.
        // Transfers and UAC are exempt from this deadline.
        child
            .kill()
            .context("no se pudo cerrar el auxiliar de apertura atascado")?;
    }
    let status = child.wait()?;
    if timed_out {
        bail!("el shell no respondió a la apertura en 30 segundos");
    }
    if !status.success() {
        bail!("el auxiliar del shell terminó con {status}");
    }
    Ok(())
}

fn read_action(input: impl Read) -> Result<Action> {
    let mut bytes = Vec::new();
    input.take(MAX_REQUEST_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        bail!("la operación contiene demasiados datos");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn worker_main() -> Result<()> {
    // SAFETY: Suppress system modal error popups (missing DLLs or ordinals in 3rd-party shell extensions).
    unsafe {
        let _ = SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX);
    }
    let action = read_action(std::io::stdin().lock())?;
    // SAFETY: The helper owns this apartment and balances it after the native
    // shell action returns. Shell launch calls wait for activation before exit.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
    }
    let result = action.run();
    unsafe { CoUninitialize() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Condvar, mpsc::channel},
        time::Duration,
    };

    #[test]
    fn full_queue_rejects_without_waiting_for_a_blocked_operation() -> Result<()> {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let release = Arc::clone(&gate);
        let (started, waiting) = channel();
        let queue = Queue::new(1, 1, move |_: ()| {
            let _ = started.send(());
            let (lock, wake) = &*gate;
            let open = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            drop(
                wake.wait_while(open, |open| !*open)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        })?;
        queue.submit(())?;
        waiting.recv_timeout(Duration::from_secs(2))?;
        queue.submit(())?;
        assert!(queue.submit(()).is_err());
        drop(queue);
        *release
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        release.1.notify_all();
        Ok(())
    }

    #[test]
    fn file_actions_use_the_serial_queue() {
        assert!(
            Action::Transfer {
                folder: "x".into(),
                files: vec![],
                copy: true
            }
            .modifies_files()
        );
        assert!(Action::EmptyBin.modifies_files());
        assert!(!Action::Open { id: "x".into() }.modifies_files());
    }

    #[test]
    fn malformed_requests_are_rejected_before_any_shell_call() {
        assert!(read_action(br#"{"operation":"unknown"}"#.as_slice()).is_err());
        assert!(read_action(br#"{"operation":"open","id":5}"#.as_slice()).is_err());
    }

    #[test]
    fn deadlines_exempt_file_operations_and_explicit_admin_launches() {
        assert!(
            Action::Open { id: "x".into() }
                .launch_timeout_ms()
                .is_some()
        );
        assert!(
            Action::Transfer {
                folder: "x".into(),
                files: vec![],
                copy: true
            }
            .launch_timeout_ms()
            .is_none()
        );
        assert!(Action::EmptyBin.launch_timeout_ms().is_none());
        assert!(
            Action::OpenAsAdmin { id: "x".into() }
                .launch_timeout_ms()
                .is_none()
        );
    }

    #[test]
    fn jump_list_entries_survive_the_request_pipe() -> Result<()> {
        let action = Action::OpenJump {
            target: jumplist::Target::Link {
                stream: vec![0x4c, 0, 0, 0, 255],
            },
            program: r"C:\Program Files\Código\app.exe".into(),
        };
        let parsed = read_action(serde_json::to_vec(&action)?.as_slice())?;
        let Action::OpenJump { target, program } = parsed else {
            bail!("la entrada de la lista de saltos cambió durante el transporte");
        };
        assert_eq!(
            target,
            jumplist::Target::Link {
                stream: vec![0x4c, 0, 0, 0, 255]
            }
        );
        assert_eq!(program, r"C:\Program Files\Código\app.exe");
        Ok(())
    }

    #[test]
    fn oversized_requests_are_rejected_before_any_shell_call() {
        assert!(read_action(std::io::repeat(0).take(MAX_REQUEST_BYTES + 2)).is_err());
    }

    #[test]
    fn request_pipe_preserves_unicode_paths_spaces_and_copy_mode() -> Result<()> {
        let action = Action::Transfer {
            folder: r"C:\Destino con espacios\á".into(),
            files: vec![r"C:\Origen\diseño final.txt".into()],
            copy: true,
        };
        let bytes = serde_json::to_vec(&action)?;
        let parsed = read_action(bytes.as_slice())?;
        match parsed {
            Action::Transfer {
                folder,
                files,
                copy,
            } => {
                assert_eq!(folder, r"C:\Destino con espacios\á");
                assert_eq!(files, [r"C:\Origen\diseño final.txt"]);
                assert!(copy);
            }
            _ => bail!("la operación de copia cambió durante el transporte"),
        }
        Ok(())
    }
}
