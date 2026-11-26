//! The moments, besides AI plans, the charger and energy saver, when the
//! mascot comes out (see `mascot`): the battery running low or full, the
//! recycle bin emptied, the first time Nosih is seen each day, a new
//! version, and an optional reminder to take a break. The network's are
//! decided by the menu bar, which follows it.
//!
//! Each one is decided here from events the dock already receives; nothing
//! polls. Only the break reminder has a timer, a minute long, and only while
//! it is on.

use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::mascot::Act;

/// Battery levels worth a word while running on battery.
const LOW: [u8; 2] = [20, 10];
/// The recycle bin is not celebrated more often than this.
pub const TIDY_AGAIN: Duration = Duration::from_secs(10 * 60);
/// Work without a pause this long earns a reminder.
pub const BREAK_AFTER: Duration = Duration::from_secs(90 * 60);
/// No input this long already was a break.
pub const BREAK_IDLE: Duration = Duration::from_secs(5 * 60);

/// What the dock remembers between moments while it runs.
#[derive(Debug, Default)]
pub struct Moments {
    /// The last battery reading: percent and whether it is charging.
    battery: Option<(u8, bool)>,
    /// When the recycle bin was last celebrated.
    pub tidied: Option<Instant>,
    /// Since when the user has been working without a pause.
    working_since: Option<Instant>,
}

impl Moments {
    /// A new battery reading; the act it deserves, if any. The first reading
    /// only sets the starting point.
    pub fn battery(&mut self, now: Option<(u8, bool)>) -> Option<Act> {
        let before = std::mem::replace(&mut self.battery, now);
        battery_act(before, now?)
    }

    /// Whether to remind of a break now: called once a minute with how long
    /// the user has not touched the keyboard or the mouse.
    pub fn break_due(&mut self, idle: Duration, now: Instant) -> bool {
        let since = *self.working_since.get_or_insert(now);
        if idle >= BREAK_IDLE {
            // A pause already: the count starts again.
            self.working_since = Some(now);
            return false;
        }
        if now.duration_since(since) >= BREAK_AFTER {
            self.working_since = Some(now);
            return true;
        }
        false
    }

    /// After sleep, the work count starts again.
    pub fn rested(&mut self) {
        self.working_since = None;
    }
}

/// The battery moment between two readings: running low on battery power
/// (crossing 20 % or 10 %), or full while charging.
fn battery_act(before: Option<(u8, bool)>, (percent, charging): (u8, bool)) -> Option<Act> {
    let (was, _) = before?;
    if charging {
        (percent >= 100 && was < 100).then_some(Act::BatteryFull)
    } else {
        LOW.iter()
            .any(|&low| percent <= low && was > low)
            .then_some(Act::BatteryLow)
    }
}

/// What Nosih remembers between runs, so a restart does not repeat a
/// greeting or a celebration: `%LOCALAPPDATA%\Nosih\moments.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Memory {
    /// The day of the last greeting, `YYYY-MM-DD`.
    pub greeted: Option<String>,
    /// The version that last ran.
    pub version: Option<String>,
}

impl Memory {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Nosih\\moments.json"))
    }

    #[must_use]
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(folder) = path.parent() {
            let _ = fs::create_dir_all(folder);
        }
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = fs::write(path, bytes);
        }
    }
}

/// The first moment of a run, after a restart, a wake-up or an unlock: a
/// new version is celebrated (not on the very first install, where the
/// greeting welcomes instead), otherwise the first time today is greeted.
#[must_use]
pub fn welcome(memory: &Memory, today: &str, version: &str) -> Option<Act> {
    if memory
        .version
        .as_deref()
        .is_some_and(|seen| seen != version)
    {
        return Some(Act::Updated);
    }
    (memory.greeted.as_deref() != Some(today)).then_some(Act::Greeting)
}

/// Today's date in local time, `YYYY-MM-DD`.
#[must_use]
pub fn today() -> String {
    // SAFETY: No arguments; returns the local time.
    let now = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_battery_speaks_up_only_when_crossing_a_level() {
        // The first reading is only the starting point.
        assert_eq!(battery_act(None, (15, false)), None);
        assert_eq!(
            battery_act(Some((21, false)), (20, false)),
            Some(Act::BatteryLow)
        );
        assert_eq!(
            battery_act(Some((11, false)), (9, false)),
            Some(Act::BatteryLow)
        );
        // Already below, or unplugged when already low: nothing new.
        assert_eq!(battery_act(Some((19, false)), (18, false)), None);
        assert_eq!(battery_act(Some((15, true)), (15, false)), None);
        // Full while charging, once.
        assert_eq!(
            battery_act(Some((99, true)), (100, true)),
            Some(Act::BatteryFull)
        );
        assert_eq!(battery_act(Some((100, true)), (100, true)), None);
        // Plugged in at 100 %: already full, nothing to say.
        assert_eq!(battery_act(Some((100, false)), (100, true)), None);
        let mut moments = Moments::default();
        assert_eq!(moments.battery(Some((21, false))), None);
        assert_eq!(moments.battery(Some((20, false))), Some(Act::BatteryLow));
        assert_eq!(moments.battery(None), None);
    }

    #[test]
    fn a_break_is_due_after_long_work_and_pauses_reset_the_count() {
        let start = Instant::now();
        let minute = Duration::from_secs(60);
        let mut moments = Moments::default();
        assert!(!moments.break_due(Duration::ZERO, start));
        assert!(!moments.break_due(Duration::ZERO, start + 89 * minute));
        assert!(moments.break_due(Duration::ZERO, start + 90 * minute));
        // Reminded: the count starts again.
        assert!(!moments.break_due(Duration::ZERO, start + 100 * minute));
        // A pause of five minutes is a break.
        assert!(!moments.break_due(BREAK_IDLE, start + 150 * minute));
        assert!(!moments.break_due(Duration::ZERO, start + 200 * minute));
        assert!(moments.break_due(Duration::ZERO, start + 240 * minute));
        moments.rested();
        assert!(!moments.break_due(Duration::ZERO, start + 241 * minute));
    }

    #[test]
    fn a_new_version_is_celebrated_and_each_day_greeted_once() {
        let fresh = Memory::default();
        // First install: a welcome, not an update.
        assert_eq!(welcome(&fresh, "2026-11-25", "0.1.23"), Some(Act::Greeting));
        let seen = Memory {
            greeted: Some("2026-11-25".into()),
            version: Some("0.1.23".into()),
        };
        assert_eq!(welcome(&seen, "2026-11-25", "0.1.23"), None);
        assert_eq!(welcome(&seen, "2026-11-26", "0.1.23"), Some(Act::Greeting));
        assert_eq!(welcome(&seen, "2026-11-25", "0.1.25"), Some(Act::Updated));
        assert_eq!(welcome(&seen, "2026-11-26", "0.1.25"), Some(Act::Updated));
    }

    #[test]
    fn memory_reads_back_what_it_wrote_and_tolerates_unknown_fields() -> serde_json::Result<()> {
        let memory = Memory {
            greeted: Some("2026-11-25".into()),
            version: Some("0.1.23".into()),
        };
        let text = serde_json::to_string(&memory)?;
        assert_eq!(serde_json::from_str::<Memory>(&text)?, memory);
        let newer: Memory = serde_json::from_str(r#"{"greeted":"2026-11-25","other":1}"#)?;
        assert_eq!(newer.greeted.as_deref(), Some("2026-11-25"));
        assert_eq!(today().len(), 10);
        Ok(())
    }
}
