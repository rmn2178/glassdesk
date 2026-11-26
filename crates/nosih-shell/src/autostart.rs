//! Starting Nosih when the user signs in, with a logon scheduled task.
//!
//! Windows can defer Run entries and makes no guarantee about their order
//! or delay. The task uses the interactive user's logon trigger. Registration
//! uses the Task Scheduler COM API with a Run entry as fallback.
//!
//! Whether autostart is on is also written to `HKCU\Software\Nosih`,
//! because task registrations cannot be read quickly and the watchdog has
//! to know it instantly while Windows shuts down.

use std::env;

use windows::{
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_TIMEOUT, HANDLE, HLOCAL, LocalFree,
            RPC_E_CHANGED_MODE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR,
        },
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, LookupAccountNameW, PSID,
            SID_NAME_USE, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
            },
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
                RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
                RegDeleteValueW, RegGetValueW, RegSetValueExW,
            },
            TaskScheduler::{
                IExecAction, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE,
                TASK_LOGON_INTERACTIVE_TOKEN, TASK_UPDATE, TaskScheduler,
            },
            Threading::{
                CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex,
                WaitForSingleObject,
            },
            Variant::VARIANT,
        },
    },
    core::{BSTR, HSTRING, Interface, Owned, PCWSTR, PWSTR, w},
};

const RUN_VALUE: PCWSTR = w!("Nosih");
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const NOSIH_KEY: PCWSTR = w!(r"Software\Nosih");
const MARKER_VALUE: PCWSTR = w!("Autostart");

/// Starts `exe` at every sign-in, or stops doing so (`None`).
pub fn set(exe: Option<&str>) -> windows::core::Result<()> {
    let sid = user_sid()?;
    let _lock = ConfigurationLock::acquire(&sid)?;
    set_locked(exe, &sid)
}

fn set_locked(exe: Option<&str>, sid: &str) -> windows::core::Result<()> {
    match exe {
        Some(exe) => {
            if let Err(error) = register_task(exe, sid) {
                crate::log::line(&format!(
                    "inicio automático: tarea no disponible, se usa Run: {error}"
                ));
                set_run_value(exe)?;
                set_marker(true)?;
            } else {
                // Save the fast shutdown marker before retiring the fallback.
                set_marker(true)?;
                delete_legacy_task(sid)?;
                delete_run_value()?;
            }
        }
        None => {
            // Attempt both removals, reporting errors instead of claiming the
            // feature is off while a task can still launch the dock.
            let tasks = with_tasks(|folder| {
                delete_task(folder, &task_name(sid))?;
                delete_matching_legacy_task(folder, sid)
            });
            let registry = delete_run_value();
            tasks?;
            registry?;
            set_marker(false)?;
        }
    }
    Ok(())
}

/// `true` if Nosih is set to start when the user signs in. Reads only the
/// registry, so it is instant and works while Windows shuts down.
#[must_use]
pub fn is_enabled() -> bool {
    marker() || run_value().is_some()
}

/// Migrates this installation's enabled Run entry or username-based task.
/// A copy run from elsewhere never takes over the installed one's autostart.
pub fn migrate() {
    if let Err(error) = migrate_inner() {
        crate::log::line(&format!(
            "migración del inicio automático pendiente: {error}"
        ));
    }
}

fn migrate_inner() -> windows::core::Result<()> {
    let sid = user_sid()?;
    let _lock = ConfigurationLock::acquire(&sid)?;
    let exe = env::current_exe()?;
    let exe = exe.to_string_lossy();
    // Current SID-based registrations also need policy upgrades. Only this
    // exact executable may update its task, and a disabled task stays disabled.
    let current = with_tasks(|folder| upgrade_task(folder, &task_name(&sid), &exe, &sid))?;
    if current {
        return Ok(());
    }
    let migratable = if let Some(command) = run_value() {
        run_approved() && command.trim().trim_matches('"').eq_ignore_ascii_case(&exe)
    } else if marker() {
        with_tasks(|folder| {
            // SAFETY: Query only; do not re-enable a task disabled externally.
            let task = match unsafe { folder.GetTask(&legacy_task_name()) } {
                Ok(task) => task,
                Err(error) if task_missing(&error) => return Ok(false),
                Err(error) => return Err(error),
            };
            let mut path = BSTR::new();
            unsafe {
                task.Definition()?
                    .Actions()?
                    .get_Item(1)?
                    .cast::<IExecAction>()?
                    .Path(&mut path)?;
            }
            Ok(unsafe { task.Enabled()? }.as_bool() && path.to_string().eq_ignore_ascii_case(&exe))
        })?
    } else {
        false
    };
    if migratable {
        // Reread under the same cross-process lock used by Settings. A
        // concurrent disable cannot be undone by an older migration read.
        set_locked(Some(&exe), &sid)?;
    }
    Ok(())
}

/// Query the actual startup registration for Settings, instead of presenting
/// the fast shutdown marker as proof that a task still exists and is enabled.
pub fn enabled_for(exe: &str) -> windows::core::Result<bool> {
    if let Some(command) = run_value() {
        return Ok(run_approved() && command.trim().trim_matches('"').eq_ignore_ascii_case(exe));
    }
    let sid = user_sid()?;
    with_tasks(|folder| {
        for name in [task_name(&sid), legacy_task_name()] {
            let task = match unsafe { folder.GetTask(&name) } {
                Ok(task) => task,
                Err(error) if task_missing(&error) => continue,
                Err(error) => return Err(error),
            };
            let mut path = BSTR::new();
            unsafe {
                task.Definition()?
                    .Actions()?
                    .get_Item(1)?
                    .cast::<IExecAction>()?
                    .Path(&mut path)?;
                if path.to_string().eq_ignore_ascii_case(exe) {
                    return Ok(task.Enabled()?.as_bool());
                }
            }
        }
        Ok(false)
    })
}

fn upgrade_task(
    folder: &ITaskFolder,
    name: &BSTR,
    exe: &str,
    sid: &str,
) -> windows::core::Result<bool> {
    // SAFETY: This query and registration run off the UI thread under the
    // configuration lock; preserve the existing triggers and user settings.
    unsafe {
        let task = match folder.GetTask(name) {
            Ok(task) => task,
            Err(error) if task_missing(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        let definition = task.Definition()?;
        let mut path = BSTR::new();
        definition
            .Actions()?
            .get_Item(1)?
            .cast::<IExecAction>()?
            .Path(&mut path)?;
        if !path.to_string().eq_ignore_ascii_case(exe) {
            return Ok(true);
        }
        if !task.Enabled()?.as_bool() {
            return Ok(true);
        }
        let mut account = BSTR::new();
        definition.Principal()?.UserId(&mut account)?;
        if account_sid(&account.to_string())? != sid {
            return Ok(true);
        }
        let settings = definition.Settings()?;
        let mut count = 0;
        let mut interval = BSTR::new();
        settings.RestartCount(&mut count)?;
        settings.RestartInterval(&mut interval)?;
        if count != 3 || interval != "PT1M" {
            settings.SetRestartInterval(&BSTR::from("PT1M"))?;
            settings.SetRestartCount(3)?;
            let none = VARIANT::default();
            folder.RegisterTaskDefinition(
                name,
                &definition,
                TASK_UPDATE.0,
                &none,
                &none,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &none,
            )?;
            crate::log::line("inicio automático: recuperación tras error actualizada");
        }
        Ok(true)
    }
}

/// Stable account identity survives account renames and local/domain name
/// collisions. The token buffer is pointer-aligned for TOKEN_USER.
fn user_sid() -> windows::core::Result<String> {
    // SAFETY: The process pseudo-handle is valid. Owned closes the real token.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = Owned::new(token);
        let mut needed = 0u32;
        let _ = GetTokenInformation(*token, TokenUser, None, 0, &mut needed);
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(
            *token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        sid_string(user.User.Sid)
    }
}

fn sid_string(sid: PSID) -> windows::core::Result<String> {
    // SAFETY: Callers supply a SID in a buffer that remains alive. Windows
    // allocates the string, which LocalFree releases after conversion.
    unsafe {
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut text)?;
        let result = text.to_string();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        result.map_err(Into::into)
    }
}

fn account_sid(account: &str) -> windows::core::Result<String> {
    if account.starts_with("S-1-") {
        return Ok(account.to_owned());
    }
    let account = HSTRING::from(account);
    let (mut sid_bytes, mut domain_chars) = (0, 0);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: First query sizes, then provide pointer-aligned SID storage
    // and a UTF-16 domain buffer. Both stay alive until conversion finishes.
    unsafe {
        let _ = LookupAccountNameW(
            None,
            &account,
            None,
            &mut sid_bytes,
            None,
            &mut domain_chars,
            &mut kind,
        );
        let mut sid = vec![0usize; (sid_bytes as usize).div_ceil(size_of::<usize>())];
        let mut domain = vec![0u16; domain_chars as usize];
        LookupAccountNameW(
            None,
            &account,
            Some(PSID(sid.as_mut_ptr().cast())),
            &mut sid_bytes,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_chars,
            &mut kind,
        )?;
        sid_string(PSID(sid.as_mut_ptr().cast()))
    }
}

struct ConfigurationLock(Owned<HANDLE>);

impl ConfigurationLock {
    fn acquire(sid: &str) -> windows::core::Result<Self> {
        let name = HSTRING::from(format!("Global\\Nosih.Autostart.{sid}"));
        // SAFETY: The handle is owned here. Only the successful wait gives
        // ownership of the mutex, which Drop releases on this same thread.
        unsafe {
            let handle = Owned::new(CreateMutexW(None, false, &name)?);
            match WaitForSingleObject(*handle, 10_000) {
                WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(handle)),
                WAIT_TIMEOUT => Err(windows::core::Error::from_hresult(
                    ERROR_TIMEOUT.to_hresult(),
                )),
                _ => Err(windows::core::Error::from_thread()),
            }
        }
    }
}

impl Drop for ConfigurationLock {
    fn drop(&mut self) {
        // SAFETY: This thread acquired the mutex and is releasing it once.
        unsafe {
            let _ = ReleaseMutex(*self.0);
        }
    }
}

fn run_approved() -> bool {
    let mut value = [0u8; 12];
    let mut size = value.len() as u32;
    // SAFETY: The buffer and its size are valid. Preserve unknown formats
    // instead of replacing a potentially disabled Windows startup entry.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"),
            RUN_VALUE,
            RRF_RT_REG_BINARY,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    matches!(status, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
        || status.is_ok()
            && size >= 4
            && matches!(
                u32::from_le_bytes([value[0], value[1], value[2], value[3]]),
                2 | 6
            )
}

fn task_name(sid: &str) -> BSTR {
    BSTR::from(format!("Nosih ({sid})"))
}

fn legacy_task_name() -> BSTR {
    BSTR::from(format!(
        "Nosih ({})",
        env::var("USERNAME").unwrap_or_default()
    ))
}

/// Runs `action` on the root folder of the Task Scheduler.
fn with_tasks<T>(
    action: impl FnOnce(&ITaskFolder) -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    // SAFETY: COM is initialized for this call (or already was, on a UI
    // thread) and released after; the service and folder are released first.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
            return Err(windows::core::Error::from_hresult(initialized));
        }
        let result = (|| {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            let none = VARIANT::default();
            service.Connect(&none, &none, &none, &none)?;
            action(&service.GetFolder(&BSTR::from("\\"))?)
        })();
        if initialized.is_ok() {
            CoUninitialize();
        }
        result
    }
}

fn register_task(exe: &str, sid: &str) -> windows::core::Result<()> {
    let xml = BSTR::from(task_xml(sid, exe));
    with_tasks(|folder| {
        let none = VARIANT::default();
        // SAFETY: All arguments live through the call.
        unsafe {
            folder.RegisterTask(
                &task_name(sid),
                &xml,
                TASK_CREATE_OR_UPDATE.0,
                &none,
                &none,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &none,
            )?;
        }
        Ok(())
    })
}

fn delete_task(folder: &ITaskFolder, name: &BSTR) -> windows::core::Result<()> {
    // SAFETY: The task belongs to the current user and the folder is live.
    match unsafe { folder.DeleteTask(name, 0) } {
        Err(error) if task_missing(&error) => Ok(()),
        result => result,
    }
}

fn task_missing(error: &windows::core::Error) -> bool {
    matches!(error.code(), code if code == ERROR_FILE_NOT_FOUND.to_hresult()
        || code == ERROR_PATH_NOT_FOUND.to_hresult())
}

fn delete_legacy_task(sid: &str) -> windows::core::Result<()> {
    with_tasks(|folder| delete_matching_legacy_task(folder, sid))
}

fn delete_matching_legacy_task(folder: &ITaskFolder, sid: &str) -> windows::core::Result<()> {
    let name = legacy_task_name();
    // SAFETY: Read-only access to the legacy task before deciding ownership.
    let task = match unsafe { folder.GetTask(&name) } {
        Ok(task) => task,
        Err(error) if task_missing(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    // Retire this user's task even if the executable moved, while protecting
    // a different account whose legacy username happened to collide.
    let mut account = BSTR::new();
    unsafe {
        task.Definition()?.Principal()?.UserId(&mut account)?;
    }
    if account_sid(&account.to_string())? == sid {
        delete_task(folder, &name)?;
    }
    Ok(())
}

/// The task: at this user's sign-in, run `exe` as them, without admin rights,
/// at normal priority (tasks default to below normal), on battery too, with
/// no time limit.
fn task_xml(user: &str, exe: &str) -> String {
    let user = escape(user);
    let exe = escape(exe);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Nosih</Author>
    <Description>Starts the Nosih dock when you sign in. Turn it off in Nosih Settings.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn marker() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` and `size` describe a valid 4-byte buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            NOSIH_KEY,
            MARKER_VALUE,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0
}

fn set_marker(on: bool) -> windows::core::Result<()> {
    let mut key = HKEY::default();
    let value = u32::from(on).to_le_bytes();
    // SAFETY: The key is closed before returning; `value` lives through the call.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            NOSIH_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result = RegSetValueExW(key, MARKER_VALUE, None, REG_DWORD, Some(&value));
        let _ = RegCloseKey(key);
        result.ok()
    }
}

/// The command under `Run`, if Nosih has one there.
fn run_value() -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut size = size_of_val(&buffer) as u32;
    // SAFETY: `buffer` and `size` describe a valid buffer of that many bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then(|| {
        let len = (size as usize / 2).saturating_sub(1);
        String::from_utf16_lossy(&buffer[..len.min(buffer.len())])
    })
}

fn set_run_value(exe: &str) -> windows::core::Result<()> {
    with_run_key(|key| {
        let command: Vec<u16> = format!("\"{exe}\"").encode_utf16().chain(Some(0)).collect();
        // SAFETY: `command` lives through the call.
        unsafe {
            let bytes = std::slice::from_raw_parts(command.as_ptr().cast(), command.len() * 2);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes))
        }
    })
}

fn delete_run_value() -> windows::core::Result<()> {
    // SAFETY: Deletes a value of the open key.
    with_run_key(|key| match unsafe { RegDeleteValueW(key, RUN_VALUE) } {
        // Removing a value that does not exist is not an error.
        WIN32_ERROR(2) => WIN32_ERROR(0),
        other => other,
    })
}

fn with_run_key(action: impl FnOnce(HKEY) -> WIN32_ERROR) -> windows::core::Result<()> {
    let mut key = HKEY::default();
    // SAFETY: The key is closed before returning.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result = action(key);
        let _ = RegCloseKey(key);
        result.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_should_escape_the_path_and_keep_normal_priority() {
        let xml = task_xml(r"PC\Ana", r"C:\Apps & Tools\nosih.exe");
        assert!(xml.contains(r"<Command>C:\Apps &amp; Tools\nosih.exe</Command>"));
        assert!(xml.contains(r"<UserId>PC\Ana</UserId>"));
        assert!(xml.contains("<Priority>4</Priority>"));
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<RestartOnFailure>"));
        assert!(xml.contains("<Interval>PT1M</Interval>"));
        assert!(xml.contains("<Count>3</Count>"));
    }

    #[test]
    fn configuration_mutex_serializes_independent_callers()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use std::{sync::mpsc, thread, time::Duration};
        let name = format!("test.{}", std::process::id());
        let lock = ConfigurationLock::acquire(&name)?;
        let (attempt, ready) = mpsc::channel();
        let (send, acquired) = mpsc::channel();
        let other = thread::spawn(
            move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                attempt.send(())?;
                let _lock = ConfigurationLock::acquire(&name)?;
                send.send(())?;
                Ok(())
            },
        );
        ready.recv()?;
        assert!(acquired.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lock);
        acquired.recv_timeout(Duration::from_secs(3))?;
        other
            .join()
            .map_err(|_| std::io::Error::other("mutex test thread panicked"))??;
        Ok(())
    }

    #[test]
    #[ignore = "Creates and removes a uniquely named disabled task in Windows Task Scheduler"]
    fn scheduler_accepts_user_sid_and_missing_task_deletion_is_idempotent()
    -> windows::core::Result<()> {
        let sid = user_sid()?;
        let exe = env::current_exe()?.to_string_lossy().into_owned();
        let name = BSTR::from(format!(
            "Nosih autostart test {} {}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        // The trigger is disabled before registration, so it cannot run.
        let xml = BSTR::from(
            task_xml(&sid, &exe).replace("<Enabled>true</Enabled>", "<Enabled>false</Enabled>"),
        );
        with_tasks(|folder| {
            let none = VARIANT::default();
            // SAFETY: Register only a uniquely named task in the current user's
            // context and delete it before the closure exits.
            let task = unsafe {
                folder.RegisterTask(
                    &name,
                    &xml,
                    windows::Win32::System::TaskScheduler::TASK_CREATE.0,
                    &none,
                    &none,
                    TASK_LOGON_INTERACTIVE_TOKEN,
                    &none,
                )?
            };
            let checked = (|| -> windows::core::Result<()> {
                let mut path = BSTR::new();
                let mut account = BSTR::new();
                unsafe {
                    task.Definition()?
                        .Actions()?
                        .get_Item(1)?
                        .cast::<IExecAction>()?
                        .Path(&mut path)?;
                    task.Definition()?.Principal()?.UserId(&mut account)?;
                }
                assert_eq!(path.to_string(), exe);
                assert_eq!(account_sid(&account.to_string())?, sid);
                // Emulate an older task. Its trigger remains disabled, so
                // enabling the task cannot launch anything during this test.
                let mut registered_retries = 0;
                unsafe {
                    task.Definition()?
                        .Settings()?
                        .RestartCount(&mut registered_retries)?;
                }
                assert_eq!(registered_retries, 3);
                let old_xml = BSTR::from(xml.to_string().replace(
                    "    <RestartOnFailure>\n      <Interval>PT1M</Interval>\n      <Count>3</Count>\n    </RestartOnFailure>\n", ""));
                unsafe {
                    folder.RegisterTask(
                        &name,
                        &old_xml,
                        TASK_UPDATE.0,
                        &none,
                        &none,
                        TASK_LOGON_INTERACTIVE_TOKEN,
                        &none,
                    )?;
                    task.SetEnabled(false.into())?;
                }
                upgrade_task(folder, &name, &exe, &sid)?;
                let restart_count = || -> windows::core::Result<i32> {
                    let mut count = 0;
                    unsafe {
                        folder
                            .GetTask(&name)?
                            .Definition()?
                            .Settings()?
                            .RestartCount(&mut count)?;
                    }
                    Ok(count)
                };
                assert_eq!(restart_count()?, 0, "a disabled task must not be rewritten");
                assert!(!unsafe { task.Enabled()? }.as_bool());
                unsafe {
                    task.SetEnabled(true.into())?;
                }
                upgrade_task(folder, &name, r"C:\Other\nosih.exe", &sid)?;
                assert_eq!(
                    restart_count()?,
                    0,
                    "another executable must not take over autostart"
                );
                upgrade_task(folder, &name, &exe, &sid)?;
                assert_eq!(restart_count()?, 3);
                let mut interval = BSTR::new();
                unsafe {
                    folder
                        .GetTask(&name)?
                        .Definition()?
                        .Settings()?
                        .RestartInterval(&mut interval)?;
                }
                assert_eq!(interval.to_string(), "PT1M");
                Ok(())
            })();
            drop(task);
            delete_task(folder, &name)?;
            checked?;
            delete_task(folder, &name)
        })
    }
}
