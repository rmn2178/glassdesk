//! When Nosih last looked for an update. Kept in `HKCU\Software\Nosih`
//! (which the uninstaller removes), so it looks once a day however often the
//! computer is restarted or the dock relaunched.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::{
    Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_QWORD,
        RRF_RT_REG_QWORD, RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
    },
    core::{PCWSTR, w},
};

const NOSIH_KEY: PCWSTR = w!(r"Software\Nosih");
const LAST_CHECK: PCWSTR = w!("LastUpdateCheck");

/// How long until the next look is due, `every` after the last one: zero if
/// it is due now, or if Nosih never looked.
#[must_use]
pub fn due_in(every: Duration) -> Duration {
    let Some(last) = last_check() else {
        return Duration::ZERO;
    };
    let now = now_seconds();
    // A clock set back past the last look would postpone it for that long.
    if last > now {
        return Duration::ZERO;
    }
    every.saturating_sub(Duration::from_secs(now - last))
}

/// Records that Nosih looked for an update now.
pub fn mark_checked() {
    let value = now_seconds().to_le_bytes();
    let mut key = HKEY::default();
    // SAFETY: The key is closed before returning; `value` lives through the call.
    unsafe {
        if RegCreateKeyExW(
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
        .is_err()
        {
            return;
        }
        let _ = RegSetValueExW(key, LAST_CHECK, None, REG_QWORD, Some(&value));
        let _ = RegCloseKey(key);
    }
}

fn last_check() -> Option<u64> {
    let mut value = 0u64;
    let mut size = size_of::<u64>() as u32;
    // SAFETY: `value` and `size` describe a valid 8-byte buffer.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            NOSIH_KEY,
            LAST_CHECK,
            RRF_RT_REG_QWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    }
    .is_ok()
    .then_some(value)
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
