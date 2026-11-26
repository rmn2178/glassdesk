//! Explicit force termination of a selected app's window-owning processes.
//! Handles are captured before opening the menu, never reopened by PID/name
//! after it closes. No process-tree traversal or automatic elevation.

use std::{
    collections::HashSet,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM, WAIT_OBJECT_0},
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::Threading::{
            GetCurrentProcessId, IsProcessCritical, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GW_OWNER, GWL_EXSTYLE, GetShellWindow, GetWindow, GetWindowLongW,
            GetWindowThreadProcessId, IsWindowVisible, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        },
    },
    core::{BOOL, PWSTR},
};

use crate::tracker::{self, TrackedWindow};

pub struct PreparedEndTask {
    processes: Vec<(u32, OwnedHandle)>,
    windows: Vec<(isize, u32)>,
}

fn independent_windows() -> windows::core::Result<Vec<(isize, u32)>> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: EnumWindows keeps the local Vec alive for this callback.
        let windows = unsafe { &mut *(data.0 as *mut Vec<(isize, u32)>) };
        let style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        if unsafe { IsWindowVisible(hwnd) }.as_bool()
            && style & WS_EX_TOOLWINDOW.0 == 0
            && (style & WS_EX_APPWINDOW.0 != 0
                || (style & WS_EX_NOACTIVATE.0 == 0
                    && unsafe { GetWindow(hwnd, GW_OWNER) }.is_err()))
        {
            let raw = hwnd.0 as isize;
            windows.push((raw, window_pid(raw)));
        }
        true.into()
    }
    let mut windows = Vec::new();
    // SAFETY: A read-only enumeration, no title/COM/message calls into the apps.
    unsafe {
        EnumWindows(Some(collect), LPARAM(&raw mut windows as isize))?;
    }
    Ok(windows)
}

fn window_pid(window: isize) -> u32 {
    let mut pid = 0;
    // SAFETY: Read-only query; a destroyed window yields zero.
    unsafe {
        GetWindowThreadProcessId(HWND(window as *mut _), Some(&mut pid));
    }
    pid
}

fn protected_pid(pid: u32, own: u32, shell: u32) -> bool {
    pid <= 4 || pid == own || pid == shell
}

fn shared_host(image: &str) -> bool {
    matches!(
        image
            .rsplit('\\')
            .next()
            .unwrap_or(image)
            .to_ascii_lowercase()
            .as_str(),
        "applicationframehost.exe" | "runtimebroker.exe" | "dllhost.exe" | "svchost.exe"
    )
}

impl PreparedEndTask {
    /// Capture all eligible processes or refuse the entire action. Keeps closed
    /// menu targets tied to their original kernel objects even if PIDs/HWNDs
    /// are reused. Protected/elevated/shared-host targets remain unavailable.
    #[must_use]
    pub fn capture(app: &str, windows: &[isize], peers: &[TrackedWindow]) -> Option<Self> {
        if windows.is_empty() || windows.len() > 128 {
            return None;
        }
        let own = unsafe { GetCurrentProcessId() };
        let shell = window_pid(unsafe { GetShellWindow() }.0 as isize);
        let shared: HashSet<u32> = peers
            .iter()
            .filter(|peer| peer.app_id != app)
            .map(|peer| window_pid(peer.hwnd))
            .collect();
        let mut seen = HashSet::new();
        let mut processes = Vec::new();
        let mut selected = Vec::new();
        for &window in windows {
            let pid = window_pid(window);
            if pid == 0 {
                continue;
            }
            if protected_pid(pid, own, shell) {
                return None;
            }
            selected.push((window, pid));
            if seen.contains(&pid) {
                continue;
            }
            // A window can close/restart while its icon's context menu opens.
            let raw = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
            }
            .ok()?;
            // SAFETY: OpenProcess transferred one valid, uniquely owned handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
            let mut critical = BOOL::default();
            unsafe { IsProcessCritical(raw, &mut critical) }.ok()?;
            if critical.as_bool() {
                return None;
            }
            let mut image = [0u16; 1024];
            let mut len = image.len() as u32;
            unsafe {
                QueryFullProcessImageNameW(
                    raw,
                    PROCESS_NAME_WIN32,
                    PWSTR(image.as_mut_ptr()),
                    &mut len,
                )
            }
            .ok()?;
            let image = String::from_utf16_lossy(&image[..len as usize]);
            if shared_host(&image)
                || image
                    .rsplit('\\')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("explorer.exe"))
            {
                return None;
            }
            // Query process identity directly. A property-store/COM request to
            // a hung window must never delay opening its recovery menu.
            let identity = if app.starts_with("shell:AppsFolder\\") {
                let mut aumid = [0u16; 1024];
                len = aumid.len() as u32;
                unsafe {
                    GetApplicationUserModelId(raw, &mut len, Some(PWSTR(aumid.as_mut_ptr())))
                }
                .ok()
                .ok()?;
                format!(
                    "shell:AppsFolder\\{}",
                    String::from_utf16_lossy(&aumid[..len.saturating_sub(1) as usize])
                )
            } else {
                tracker::normalize_id(&image)
            };
            if identity != app || window_pid(window) != pid {
                return None;
            }
            // Some hosts (e.g. office suites) give different apps windows in
            // the same process. Never end the other app through this menu.
            if shared.contains(&pid) {
                return None;
            }
            seen.insert(pid);
            processes.push((pid, handle));
        }
        let selection = Self {
            processes,
            windows: selected,
        };
        if selection.processes.is_empty() {
            return None;
        }
        // Also covers windows outside the current virtual desktop, which the
        // visible dock roster intentionally excludes. Never terminate through
        // an incomplete selection of independent windows sharing that process.
        if !selection.includes(&independent_windows().ok()?) {
            return None;
        }
        Some(selection)
    }

    fn includes(&self, current: &[(isize, u32)]) -> bool {
        !current.iter().any(|&(window, pid)| {
            self.processes.iter().any(|(target, _)| *target == pid)
                && !self.windows.contains(&(window, pid))
        })
    }

    fn validate(&self) -> windows::core::Result<()> {
        let denied =
            || windows::core::Error::from_hresult(windows::Win32::Foundation::E_ACCESSDENIED);
        if !self.includes(&independent_windows()?) {
            return Err(denied());
        }
        // Validate every target before ending any. A background process whose
        // selected windows all closed must survive a stale context menu.
        for (pid, process) in &self.processes {
            let handle = HANDLE(process.as_raw_handle());
            if unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0 {
                continue;
            }
            if !self
                .windows
                .iter()
                .any(|&(window, owner)| owner == *pid && window_pid(window) == *pid)
            {
                return Err(denied());
            }
            let mut critical = BOOL::default();
            unsafe {
                IsProcessCritical(handle, &mut critical)?;
            }
            if critical.as_bool() {
                return Err(denied());
            }
        }
        Ok(())
    }

    /// Requests termination without waiting on the UI or calling into a hung
    /// window. Natural exits during an open menu are harmless no-ops.
    pub fn terminate(self) -> windows::core::Result<()> {
        self.validate()?;
        let mut failure = None;
        for (_, process) in self.processes {
            let handle = HANDLE(process.as_raw_handle());
            if unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0 {
                continue;
            }
            let result = (|| {
                let mut critical = BOOL::default();
                unsafe {
                    IsProcessCritical(handle, &mut critical)?;
                }
                if critical.as_bool() {
                    return Err(windows::core::Error::from_hresult(
                        windows::Win32::Foundation::E_ACCESSDENIED,
                    ));
                }
                // SAFETY: Only the original, explicitly selected process is
                // targeted. Its handle stays alive throughout this call.
                unsafe { TerminateProcess(handle, 1) }
            })();
            if let Err(error) = result {
                // Exiting between the check and TerminateProcess is success.
                if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
        sync::mpsc::{self, Receiver},
        time::Duration,
    };

    struct Fixture {
        child: Child,
        windows: [isize; 2],
        events: Receiver<String>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    impl Fixture {
        fn spawn() -> Result<Self, Box<dyn std::error::Error>> {
            let child = Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "end_task::tests::end_task_frozen_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .creation_flags(0x0800_0000)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;
            let mut child = child;
            let stdout = child.stdout.take().ok_or("missing fixture stdout")?;
            let (send, receive) = mpsc::channel();
            let mut fixture = Self {
                child,
                windows: [0, 0],
                events: receive,
            };
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(handles) = line.strip_prefix("NOSIH_END_TASK_FIXTURE ") {
                        let _ = send.send(handles.to_owned());
                    }
                }
            });
            let handles = fixture.events.recv_timeout(Duration::from_secs(5))?;
            let handles: Vec<isize> = handles
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            fixture.windows = handles
                .try_into()
                .map_err(|_| "wrong fixture window count")?;
            Ok(fixture)
        }
        fn command(&mut self, command: &str) -> Result<(), Box<dyn std::error::Error>> {
            let stdin = self.child.stdin.as_mut().ok_or("missing fixture stdin")?;
            writeln!(stdin, "{command}")?;
            stdin.flush()?;
            let reply = self.events.recv_timeout(Duration::from_secs(5))?;
            if reply != "OK" {
                return Err("unexpected fixture reply".into());
            }
            Ok(())
        }
        fn app() -> Result<String, Box<dyn std::error::Error>> {
            Ok(tracker::normalize_id(
                &std::env::current_exe()?.to_string_lossy(),
            ))
        }
    }

    #[test]
    #[ignore = "Internal subprocess fixture, started only by the native end-task tests"]
    fn end_task_frozen_fixture() -> windows::core::Result<()> {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, ShowWindow, WS_EX_APPWINDOW,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
        };
        let create = || unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                windows::core::w!("STATIC"),
                windows::core::w!("Nosih frozen end-task fixture"),
                WS_OVERLAPPEDWINDOW,
                -32000,
                -32000,
                160,
                120,
                None,
                None,
                None,
                None,
            )
        };
        let first = create()?;
        let second = create()?;
        println!(
            "NOSIH_END_TASK_FIXTURE {} {}",
            first.0 as isize, second.0 as isize
        );
        let _ = std::io::stdout().flush();
        // Deliberately no message pump: Quit/WM_CLOSE cannot make progress.
        // Commands mutate test-owned HWNDs on their owning thread only.
        for command in std::io::stdin().lock().lines().map_while(Result::ok) {
            match command.as_str() {
                "close" => unsafe {
                    let _ = DestroyWindow(first);
                    let _ = DestroyWindow(second);
                },
                "new" => unsafe {
                    let added = CreateWindowExW(
                        WS_EX_APPWINDOW | WS_EX_NOACTIVATE,
                        windows::core::w!("STATIC"),
                        windows::core::w!("Nosih extra window test"),
                        WS_OVERLAPPEDWINDOW,
                        -32000,
                        -32000,
                        160,
                        120,
                        None,
                        None,
                        None,
                        None,
                    )?;
                    let _ = ShowWindow(added, SW_SHOWNOACTIVATE);
                },
                _ => {}
            }
            println!("NOSIH_END_TASK_FIXTURE OK");
            let _ = std::io::stdout().flush();
        }
        Ok(())
    }

    #[test]
    #[ignore = "Terminates only test-owned frozen child processes"]
    fn native_end_task_kills_only_the_selected_frozen_process_and_deduplicates_windows()
    -> Result<(), Box<dyn std::error::Error>> {
        use windows::Win32::UI::WindowsAndMessaging::{
            SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL,
        };
        let mut victim = Fixture::spawn()?;
        let mut peer = Fixture::spawn()?;
        let app = Fixture::app()?;
        let peers = vec![TrackedWindow {
            hwnd: peer.windows[0],
            app_id: app.clone(),
            minimized: false,
        }];
        let prepared =
            PreparedEndTask::capture(&app, &victim.windows, &peers).ok_or("capture failed")?;
        assert_eq!(prepared.processes.len(), 1);
        assert_eq!(
            unsafe {
                SendMessageTimeoutW(
                    HWND(victim.windows[0] as *mut _),
                    WM_NULL,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    50,
                    None,
                )
            }
            .0,
            0
        );
        prepared.terminate()?;
        assert_eq!(
            unsafe { WaitForSingleObject(HANDLE(victim.child.as_raw_handle()), 2_000) },
            WAIT_OBJECT_0
        );
        assert!(victim.child.try_wait()?.is_some());
        assert!(peer.child.try_wait()?.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Uses only test-owned child processes to validate stale menus and shared hosts"]
    fn native_end_task_stale_or_cancelled_menu_cannot_kill_a_new_or_unrelated_process()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut victim = Fixture::spawn()?;
        let app = Fixture::app()?;
        let cancelled =
            PreparedEndTask::capture(&app, &victim.windows, &[]).ok_or("capture failed")?;
        drop(cancelled);
        assert!(victim.child.try_wait()?.is_none());
        assert!(PreparedEndTask::capture("wrong-app", &victim.windows, &[]).is_none());
        let shared = [TrackedWindow {
            hwnd: victim.windows[1],
            app_id: "different-app".into(),
            minimized: false,
        }];
        assert!(PreparedEndTask::capture(&app, &victim.windows[..1], &shared).is_none());
        let stale = PreparedEndTask::capture(&app, &victim.windows, &[]).ok_or("capture failed")?;
        victim.child.kill()?;
        victim.child.wait()?;
        let mut replacement = Fixture::spawn()?;
        stale.terminate()?;
        assert!(replacement.child.try_wait()?.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Uses test-owned children; never closes user applications"]
    fn native_end_task_refuses_closed_windows_and_windows_added_while_the_menu_is_open()
    -> Result<(), Box<dyn std::error::Error>> {
        let app = Fixture::app()?;
        let mut closed = Fixture::spawn()?;
        let selection =
            PreparedEndTask::capture(&app, &closed.windows, &[]).ok_or("capture failed")?;
        closed.command("close")?;
        assert!(selection.terminate().is_err());
        assert!(closed.child.try_wait()?.is_none());
        let mut changed = Fixture::spawn()?;
        let selection =
            PreparedEndTask::capture(&app, &changed.windows, &[]).ok_or("capture failed")?;
        changed.command("new")?;
        assert!(selection.terminate().is_err());
        assert!(changed.child.try_wait()?.is_none());
        assert!(PreparedEndTask::capture(&app, &changed.windows, &[]).is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Uses test-owned children to check all targets before any termination"]
    fn native_end_task_does_not_partially_end_a_multi_process_selection_that_changed()
    -> Result<(), Box<dyn std::error::Error>> {
        let app = Fixture::app()?;
        let mut first = Fixture::spawn()?;
        let mut second = Fixture::spawn()?;
        let windows: Vec<isize> = first.windows.into_iter().chain(second.windows).collect();
        let selection = PreparedEndTask::capture(&app, &windows, &[]).ok_or("capture failed")?;
        second.command("new")?;
        assert!(selection.terminate().is_err());
        assert!(first.child.try_wait()?.is_none());
        assert!(second.child.try_wait()?.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "Measures native menu preparation and handle release on test-owned processes"]
    fn native_end_task_menu_preparation_releases_handles_after_repeated_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let mut fixture = Fixture::spawn()?;
        let app = Fixture::app()?;
        let handles = || -> windows::core::Result<u32> {
            let mut count = 0;
            unsafe {
                GetProcessHandleCount(GetCurrentProcess(), &mut count)?;
            }
            Ok(count)
        };
        // Repeating handles simulates the largest permitted multi-window menu
        // without creating 128 real windows on the user's desktop.
        let windows: Vec<isize> = fixture.windows.into_iter().cycle().take(128).collect();
        drop(PreparedEndTask::capture(&app, &windows, &[]).ok_or("capture failed")?);
        let before = handles()?;
        let mut times = Vec::new();
        for _ in 0..64 {
            let started = std::time::Instant::now();
            let selection =
                PreparedEndTask::capture(&app, &windows, &[]).ok_or("capture failed")?;
            assert_eq!(selection.processes.len(), 1);
            drop(selection);
            times.push(started.elapsed().as_micros());
        }
        assert_eq!(handles()?, before, "cancelled menus leaked process handles");
        assert!(fixture.child.try_wait()?.is_none());
        times.sort_unstable();
        println!(
            "end-task capture, 128 slots/1 process, 64 cancelled menus: median={} us, max={} us, handle delta=0",
            times[times.len() / 2],
            times[times.len() - 1]
        );
        Ok(())
    }

    #[test]
    fn end_task_refuses_system_shell_self_and_shared_hosts() {
        for pid in [0, 4, 100, 200] {
            assert!(protected_pid(pid, 100, 200));
        }
        assert!(!protected_pid(300, 100, 200));
        assert!(shared_host(r"C:\Windows\System32\ApplicationFrameHost.exe"));
        assert!(shared_host(r"C:\Windows\System32\RuntimeBroker.exe"));
        assert!(!shared_host(r"C:\Apps\chat.exe"));
        assert!(PreparedEndTask::capture("closed", &[], &[]).is_none());
    }
}
