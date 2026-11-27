//! Spotlight-style quick launcher, calculator, unit converter, and system
//! command runner for Nosih.

use std::{
    cmp::Ordering,
    collections::HashSet,
    env,
    fs,
    path::{Path, PathBuf},
};

use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HWND},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

/// Built-in system commands exposed through Spotlight search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAction {
    Sleep,
    LockScreen,
    Restart,
    ShutDown,
    EmptyTrash,
    ToggleDarkMode,
    ToggleDoNotDisturb,
    OpenSettings,
    AboutPc,
    ScreenSnip,
}

/// Discriminant for a Spotlight search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    App { path: String },
    Math { expression: String, result: String },
    Command(SystemAction),
}

/// Single result row rendered by the Spotlight UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub subtitle: String,
    pub kind: EntryKind,
}

/// Indexed Start Menu application shortcut.
#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    pub name_lower: String,
    pub path: String,
}

/// In-memory index of Start Menu shortcuts for fast Spotlight queries.
#[derive(Clone, Debug, Default)]
pub struct Index {
    apps: Vec<AppEntry>,
}

struct CommandSpec {
    action: SystemAction,
    title: &'static str,
    subtitle: &'static str,
    keywords: &'static [&'static str],
}

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        action: SystemAction::OpenSettings,
        title: "Settings",
        subtitle: "Open Nosih Settings",
        keywords: &["settings", "preferences", "prefs", "config", "options", "nosih"],
    },
    CommandSpec {
        action: SystemAction::ToggleDarkMode,
        title: "Toggle Dark Mode",
        subtitle: "Switch between Light and Dark theme",
        keywords: &["dark", "dark mode", "light", "light mode", "theme", "appearance"],
    },
    CommandSpec {
        action: SystemAction::ToggleDoNotDisturb,
        title: "Toggle Do Not Disturb",
        subtitle: "Mute or unmute notifications",
        keywords: &["dnd", "do not disturb", "focus", "notifications", "quiet", "mute"],
    },
    CommandSpec {
        action: SystemAction::ScreenSnip,
        title: "Screen Snip",
        subtitle: "Capture a screenshot",
        keywords: &["screenshot", "snip", "screen snip", "capture", "snipping"],
    },
    CommandSpec {
        action: SystemAction::LockScreen,
        title: "Lock Screen",
        subtitle: "Lock this Windows session",
        keywords: &["lock", "lock screen", "lockscreen"],
    },
    CommandSpec {
        action: SystemAction::Sleep,
        title: "Sleep",
        subtitle: "Put PC to sleep",
        keywords: &["sleep", "suspend", "standby"],
    },
    CommandSpec {
        action: SystemAction::Restart,
        title: "Restart",
        subtitle: "Restart Windows",
        keywords: &["restart", "reboot"],
    },
    CommandSpec {
        action: SystemAction::ShutDown,
        title: "Shut Down",
        subtitle: "Power off this PC",
        keywords: &["shutdown", "shut down", "power", "power off", "turn off"],
    },
    CommandSpec {
        action: SystemAction::EmptyTrash,
        title: "Empty Trash",
        subtitle: "Empty the Recycle Bin",
        keywords: &["trash", "empty trash", "recycle", "recycle bin", "bin"],
    },
    CommandSpec {
        action: SystemAction::AboutPc,
        title: "About This PC",
        subtitle: "View system specifications",
        keywords: &["about", "about pc", "about this pc", "system info", "specs"],
    },
];

const QUICK_COMMANDS: [SystemAction; 6] = [
    SystemAction::OpenSettings,
    SystemAction::ToggleDarkMode,
    SystemAction::ToggleDoNotDisturb,
    SystemAction::ScreenSnip,
    SystemAction::LockScreen,
    SystemAction::Sleep,
];

const SKIP_NAME_FRAGMENTS: [&str; 3] = ["uninstall", "readme", "help"];
const MAX_SCAN_DEPTH: usize = 4;
const MAX_PARSE_DEPTH: usize = 64;

impl Index {
    /// Scans the user and system Start Menu `Programs` folders for `.lnk`
    /// shortcuts (up to depth 4), skipping uninstallers/help links,
    /// deduplicating by lowercase name, and sorting alphabetically.
    #[must_use]
    pub fn build() -> Self {
        let mut roots: Vec<PathBuf> = Vec::with_capacity(2);
        if let Some(appdata) = env::var_os("APPDATA") {
            let mut path = PathBuf::from(appdata);
            path.push("Microsoft");
            path.push("Windows");
            path.push("Start Menu");
            path.push("Programs");
            roots.push(path);
        }
        if let Some(program_data) = env::var_os("ProgramData") {
            let mut path = PathBuf::from(program_data);
            path.push("Microsoft");
            path.push("Windows");
            path.push("Start Menu");
            path.push("Programs");
            roots.push(path);
        }

        let mut seen = HashSet::new();
        let mut apps = Vec::new();
        for root in &roots {
            scan_shortcuts(root, 0, &mut seen, &mut apps);
        }
        apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower).then_with(|| a.name.cmp(&b.name)));
        Self { apps }
    }

    /// Creates an index directly from a pre-populated list of applications.
    #[must_use]
    pub fn from_apps(mut apps: Vec<AppEntry>) -> Self {
        let mut seen = HashSet::with_capacity(apps.len());
        apps.retain(|app| seen.insert(app.name_lower.clone()));
        apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower).then_with(|| a.name.cmp(&b.name)));
        Self { apps }
    }

    /// Returns matching Spotlight entries for `query`, capped at `max_results`.
    #[must_use]
    pub fn search(&self, query: &str, max_results: usize) -> Vec<Entry> {
        if max_results == 0 {
            return Vec::new();
        }
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return QUICK_COMMANDS
                .iter()
                .take(max_results)
                .filter_map(|&action| command_entry(action))
                .collect();
        }

        let mut results = Vec::with_capacity(max_results);
        if let Some(result) = eval_math_or_unit(trimmed) {
            let subtitle = format!("{trimmed} = {result}");
            results.push(Entry {
                title: result.clone(),
                subtitle,
                kind: EntryKind::Math {
                    expression: trimmed.to_owned(),
                    result,
                },
            });
            if results.len() >= max_results {
                return results;
            }
        }

        let query_lower = trimmed.to_ascii_lowercase();
        let mut scored: Vec<(u32, Entry)> = Vec::new();

        for spec in COMMANDS {
            let title_lower = spec.title.to_ascii_lowercase();
            let mut best = fuzzy_score(&query_lower, &title_lower);
            for &kw in spec.keywords {
                if let Some(score) = fuzzy_score(&query_lower, kw) {
                    best = Some(best.map_or(score, |prev| prev.max(score)));
                }
            }
            if let Some(score) = best {
                scored.push((
                    score,
                    Entry {
                        title: spec.title.to_owned(),
                        subtitle: spec.subtitle.to_owned(),
                        kind: EntryKind::Command(spec.action),
                    },
                ));
            }
        }

        for app in &self.apps {
            if let Some(score) = fuzzy_score(&query_lower, &app.name_lower) {
                scored.push((
                    score,
                    Entry {
                        title: app.name.clone(),
                        subtitle: "Application".to_owned(),
                        kind: EntryKind::App {
                            path: app.path.clone(),
                        },
                    },
                ));
            }
        }

        scored.sort_by(|(sa, ea), (sb, eb)| {
            sb.cmp(sa)
                .then_with(|| cmp_ascii_case_insensitive(&ea.title, &eb.title))
                .then_with(|| ea.title.cmp(&eb.title))
        });

        let remaining = max_results.saturating_sub(results.len());
        results.extend(scored.into_iter().take(remaining).map(|(_, entry)| entry));
        results
    }
}

fn command_entry(action: SystemAction) -> Option<Entry> {
    COMMANDS.iter().find(|spec| spec.action == action).map(|spec| Entry {
        title: spec.title.to_owned(),
        subtitle: spec.subtitle.to_owned(),
        kind: EntryKind::Command(spec.action),
    })
}

fn scan_shortcuts(
    dir: &Path,
    depth: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<AppEntry>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan_shortcuts(&path, depth + 1, seen, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let is_lnk = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
        if !is_lnk {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = stem.trim();
        if name.is_empty() {
            continue;
        }
        let name_lower = name.to_ascii_lowercase();
        if SKIP_NAME_FRAGMENTS
            .iter()
            .any(|fragment| name_lower.contains(fragment))
        {
            continue;
        }
        if seen.insert(name_lower.clone()) {
            out.push(AppEntry {
                name: name.to_owned(),
                name_lower,
                path: path.to_string_lossy().into_owned(),
            });
        }
    }
}

fn cmp_ascii_case_insensitive(a: &str, b: &str) -> Ordering {
    let mut ai = a.bytes().map(|c| c.to_ascii_lowercase());
    let mut bi = b.bytes().map(|c| c.to_ascii_lowercase());
    loop {
        match (ai.next(), bi.next()) {
            (Some(ca), Some(cb)) if ca != cb => return ca.cmp(&cb),
            (Some(_), Some(_)) => {}
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// Scores how well `query_lower` matches `target_lower`:
/// exact match > exact prefix > word-boundary > substring > subsequence.
fn fuzzy_score(query_lower: &str, target_lower: &str) -> Option<u32> {
    if query_lower.is_empty() || target_lower.is_empty() {
        return None;
    }
    if target_lower == query_lower {
        return Some(10_000);
    }
    let len_diff = target_lower.len().saturating_sub(query_lower.len()).min(500) as u32;
    if target_lower.starts_with(query_lower) {
        return Some(8_000 - len_diff);
    }
    if let Some(pos) = find_word_boundary_match(target_lower, query_lower) {
        let pos_penalty = pos.min(500) as u32;
        return Some(6_000 - pos_penalty - len_diff);
    }
    if let Some(pos) = target_lower.find(query_lower) {
        let pos_penalty = pos.min(500) as u32;
        return Some(4_000 - pos_penalty - len_diff);
    }
    subsequence_score(query_lower, target_lower)
}

fn is_word_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '-' | '_' | '.' | '/' | '\\' | '(' | ')')
}

fn find_word_boundary_match(target: &str, query: &str) -> Option<usize> {
    let mut prev_sep = false;
    for (idx, ch) in target.char_indices() {
        if prev_sep && target[idx..].starts_with(query) {
            return Some(idx);
        }
        prev_sep = is_word_separator(ch);
    }
    None
}

fn subsequence_score(query: &str, target: &str) -> Option<u32> {
    let mut query_chars = query.chars().peekable();
    let mut first_match: Option<usize> = None;
    let mut last_match: usize = 0;
    let mut prev_match: Option<usize> = None;
    let mut consecutive: u32 = 0;
    let mut boundary_hits: u32 = 0;
    let mut prev_sep = true;

    for (idx, ch) in target.chars().enumerate() {
        if let Some(&qch) = query_chars.peek()
            && ch == qch
        {
            let _ = query_chars.next();
            if first_match.is_none() {
                first_match = Some(idx);
            }
            if prev_match.is_some_and(|p| p + 1 == idx) {
                consecutive = consecutive.saturating_add(1);
            }
            if prev_sep {
                boundary_hits = boundary_hits.saturating_add(1);
            }
            prev_match = Some(idx);
            last_match = idx;
        }
        prev_sep = is_word_separator(ch);
    }

    if query_chars.peek().is_some() {
        return None;
    }

    let start = first_match?;
    let span = last_match.saturating_sub(start).min(500) as u32;
    let bonus = (consecutive.saturating_mul(25) + boundary_hits.saturating_mul(30)).min(500);
    Some((1_000 + bonus).saturating_sub(span).max(1))
}

/* ------------------------------------------------ Math & unit conversions */

/// Evaluates an arithmetic expression, percentage (`X% of Y`), or unit
/// conversion (`f to c`, `c to f`, `km to mi`, `mi to km`, `kg to lb`,
/// `lb to kg`, `cm to in`, `in to cm`, `m to ft`, `ft to m`).
///
/// Returns `None` for bare numbers without an operator/conversion or for
/// invalid/non-finite expressions.
#[must_use]
pub fn eval_math_or_unit(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(converted) = eval_unit_conversion(trimmed) {
        return Some(converted);
    }
    if let Some(percent) = eval_percent_of(trimmed) {
        return Some(percent);
    }
    eval_arithmetic(trimmed)
}

fn eval_unit_conversion(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let (left, right) = lower.split_once(" to ")?;
    let left = left.trim();
    let to_unit = right.trim();
    if left.is_empty() || to_unit.is_empty() {
        return None;
    }

    let (num_str, from_unit) = split_number_and_unit(left)?;
    let value: f64 = num_str.parse().ok()?;
    if !value.is_finite() {
        return None;
    }

    let from = normalize_unit(from_unit)?;
    let to = normalize_unit(to_unit)?;

    let (converted, suffix) = match (from, to) {
        ("f", "c") => ((value - 32.0) * 5.0 / 9.0, "°C"),
        ("c", "f") => (value * 9.0 / 5.0 + 32.0, "°F"),
        ("km", "mi") => (value * 0.621_371_192_237_334, "mi"),
        ("mi", "km") => (value * 1.609_344, "km"),
        ("kg", "lb") => (value * 2.204_622_621_848_776, "lb"),
        ("lb", "kg") => (value * 0.453_592_37, "kg"),
        ("cm", "in") => (value / 2.54, "in"),
        ("in", "cm") => (value * 2.54, "cm"),
        ("m", "ft") => (value * 3.280_839_895_013_123, "ft"),
        ("ft", "m") => (value * 0.3048, "m"),
        _ => return None,
    };

    let formatted = format_number(converted)?;
    Some(format!("{formatted} {suffix}"))
}

fn split_number_and_unit(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
        i += 1;
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                saw_digit = true;
                i += 1;
            }
            b'.' if !saw_dot => {
                saw_dot = true;
                i += 1;
            }
            _ => break,
        }
    }
    if !saw_digit {
        return None;
    }
    let num_part = &text[..i];
    let unit_part = text[i..].trim();
    if unit_part.is_empty() {
        return None;
    }
    Some((num_part, unit_part))
}

fn normalize_unit(unit: &str) -> Option<&'static str> {
    match unit.trim_start_matches('°') {
        "f" | "fahrenheit" => Some("f"),
        "c" | "celsius" => Some("c"),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres" => Some("km"),
        "mi" | "mile" | "miles" => Some("mi"),
        "kg" | "kilogram" | "kilograms" => Some("kg"),
        "lb" | "lbs" | "pound" | "pounds" => Some("lb"),
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => Some("cm"),
        "in" | "inch" | "inches" => Some("in"),
        "m" | "meter" | "meters" | "metre" | "metres" => Some("m"),
        "ft" | "foot" | "feet" => Some("ft"),
        _ => None,
    }
}

fn eval_percent_of(input: &str) -> Option<String> {
    let (lhs, rest) = input.split_once('%')?;
    let rest_trimmed = rest.trim_start();
    let rhs = if rest_trimmed.len() >= 2
        && rest_trimmed.as_bytes()[..2].eq_ignore_ascii_case(b"of")
    {
        let after_of = &rest_trimmed[2..];
        if !after_of.starts_with(char::is_whitespace) {
            return None;
        }
        after_of.trim()
    } else {
        return None;
    };

    if lhs.trim().is_empty() || rhs.is_empty() {
        return None;
    }
    let pct: f64 = lhs.trim().parse().ok()?;
    let base: f64 = rhs.parse().ok()?;
    if !pct.is_finite() || !base.is_finite() {
        return None;
    }
    format_number((pct / 100.0) * base)
}

fn eval_arithmetic(input: &str) -> Option<String> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        pos: 0,
        has_operator: false,
    };
    let value = parser.parse_expr(0)?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() || !parser.has_operator {
        return None;
    }
    format_number(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    has_operator: bool,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_expr(&mut self, depth: usize) -> Option<f64> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        let mut lhs = self.parse_term(depth + 1)?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(b'+') => b'+',
                Some(b'-') => b'-',
                _ => break,
            };
            self.pos += 1;
            self.has_operator = true;
            let rhs = self.parse_term(depth + 1)?;
            lhs = if op == b'+' { lhs + rhs } else { lhs - rhs };
            if !lhs.is_finite() {
                return None;
            }
        }
        Some(lhs)
    }

    fn parse_term(&mut self, depth: usize) -> Option<f64> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        let mut lhs = self.parse_unary(depth + 1)?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(b'*') => b'*',
                Some(b'/') => b'/',
                Some(b'%') => b'%',
                _ => break,
            };
            self.pos += 1;
            self.has_operator = true;
            let rhs = self.parse_unary(depth + 1)?;
            lhs = match op {
                b'*' => lhs * rhs,
                b'/' => {
                    if rhs == 0.0 {
                        return None;
                    }
                    lhs / rhs
                }
                b'%' => {
                    if rhs == 0.0 {
                        return None;
                    }
                    lhs % rhs
                }
                _ => return None,
            };
            if !lhs.is_finite() {
                return None;
            }
        }
        Some(lhs)
    }

    fn parse_unary(&mut self, depth: usize) -> Option<f64> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        self.skip_ws();
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                let value = self.parse_unary(depth + 1)?;
                Some(-value)
            }
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary(depth + 1)
            }
            _ => self.parse_power(depth + 1),
        }
    }

    fn parse_power(&mut self, depth: usize) -> Option<f64> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        let base = self.parse_primary(depth + 1)?;
        self.skip_ws();
        if self.peek() == Some(b'^') {
            self.pos += 1;
            self.has_operator = true;
            let exp = self.parse_unary(depth + 1)?;
            let result = base.powf(exp);
            if !result.is_finite() {
                return None;
            }
            Some(result)
        } else {
            Some(base)
        }
    }

    fn parse_primary(&mut self, depth: usize) -> Option<f64> {
        if depth > MAX_PARSE_DEPTH {
            return None;
        }
        self.skip_ws();
        if self.peek() == Some(b'(') {
            self.pos += 1;
            let value = self.parse_expr(depth + 1)?;
            self.skip_ws();
            if self.peek() != Some(b')') {
                return None;
            }
            self.pos += 1;
            return Some(value);
        }

        let start = self.pos;
        let mut saw_digit = false;
        let mut saw_dot = false;
        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' => {
                    saw_digit = true;
                    self.pos += 1;
                }
                b'.' if !saw_dot => {
                    saw_dot = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        if !saw_digit {
            return None;
        }
        let slice = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        let value: f64 = slice.parse().ok()?;
        value.is_finite().then_some(value)
    }
}

fn format_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let scaled = value * 10_000.0;
    let rounded = if scaled.is_finite() {
        let r = scaled.round() / 10_000.0;
        if r == 0.0 { 0.0 } else { r }
    } else {
        value
    };
    let text = format!("{rounded:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    Some(if trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    })
}

/* --------------------------------------------------------------- Execution */

/// Executes a selected Spotlight entry: launches an app shortcut, copies a
/// calculation result to the Windows clipboard, or invokes a system action.
pub fn execute(entry: &Entry) {
    match &entry.kind {
        EntryKind::App { path } => {
            let _ = crate::launcher::open(path);
        }
        EntryKind::Math { result, .. } => {
            copy_text_to_clipboard(result);
        }
        EntryKind::Command(action) => match action {
            SystemAction::Sleep => crate::control_center::sleep_computer(),
            SystemAction::LockScreen => crate::control_center::lock_workstation(),
            SystemAction::Restart => crate::control_center::restart_computer(),
            SystemAction::ShutDown => crate::control_center::shut_down_computer(),
            SystemAction::EmptyTrash => crate::recycle_bin::empty(HWND::default()),
            SystemAction::ToggleDarkMode => {
                let _ = crate::control_center::set_dark_mode(!crate::control_center::is_dark_mode());
            }
            SystemAction::ToggleDoNotDisturb => {
                let _ = crate::control_center::set_do_not_disturb(
                    !crate::control_center::do_not_disturb(),
                );
            }
            SystemAction::OpenSettings => crate::control_center::open_nosih_settings(),
            SystemAction::AboutPc => crate::control_center::open_about_pc(),
            SystemAction::ScreenSnip => crate::control_center::open_screen_snip(),
        },
    }
}

fn copy_text_to_clipboard(text: &str) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let Some(byte_len) = wide.len().checked_mul(size_of::<u16>()) else {
        return;
    };

    // SAFETY: Standard Win32 clipboard sequence. Global memory is unlocked
    // before `SetClipboardData`; ownership transfers to Windows on success and
    // is explicitly freed on any failure path.
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return;
        }
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, byte_len) else {
            let _ = CloseClipboard();
            return;
        };
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            let _ = GlobalFree(Some(hmem));
            let _ = CloseClipboard();
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr.cast::<u16>(), wide.len());
        let _ = GlobalUnlock(hmem);
        if SetClipboardData(u32::from(CF_UNICODETEXT.0), Some(HANDLE(hmem.0))).is_err() {
            let _ = GlobalFree(Some(hmem));
        }
        let _ = CloseClipboard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_precedence_parentheses_and_unary() {
        assert_eq!(eval_math_or_unit("2 + 3 * 4"), Some("14".to_owned()));
        assert_eq!(eval_math_or_unit("(2 + 3) * 4"), Some("20".to_owned()));
        assert_eq!(eval_math_or_unit("10 / 4"), Some("2.5".to_owned()));
        assert_eq!(eval_math_or_unit("10 % 3"), Some("1".to_owned()));
        assert_eq!(eval_math_or_unit("2 ^ 10"), Some("1024".to_owned()));
        assert_eq!(eval_math_or_unit("2 ^ 3 ^ 2"), Some("512".to_owned()));
        assert_eq!(eval_math_or_unit("-5 + 12"), Some("7".to_owned()));
        assert_eq!(eval_math_or_unit("(-2) ^ 2"), Some("4".to_owned()));
        assert_eq!(eval_math_or_unit("1 / 3"), Some("0.3333".to_owned()));
    }

    #[test]
    fn percent_of_expressions() {
        assert_eq!(eval_math_or_unit("15% of 240"), Some("36".to_owned()));
        assert_eq!(eval_math_or_unit("12.5% OF 80"), Some("10".to_owned()));
        assert_eq!(eval_math_or_unit("20 % of 55"), Some("11".to_owned()));
    }

    #[test]
    fn unit_conversions() {
        assert_eq!(eval_math_or_unit("212 f to c"), Some("100 °C".to_owned()));
        assert_eq!(eval_math_or_unit("0 c to f"), Some("32 °F".to_owned()));
        assert_eq!(eval_math_or_unit("-40f to c"), Some("-40 °C".to_owned()));
        assert_eq!(eval_math_or_unit("10 km to mi"), Some("6.2137 mi".to_owned()));
        assert_eq!(eval_math_or_unit("1 mi to km"), Some("1.6093 km".to_owned()));
        assert_eq!(eval_math_or_unit("10 kg to lb"), Some("22.0462 lb".to_owned()));
        assert_eq!(eval_math_or_unit("10 lb to kg"), Some("4.5359 kg".to_owned()));
        assert_eq!(eval_math_or_unit("2.54 cm to in"), Some("1 in".to_owned()));
        assert_eq!(eval_math_or_unit("10 in to cm"), Some("25.4 cm".to_owned()));
        assert_eq!(eval_math_or_unit("1 m to ft"), Some("3.2808 ft".to_owned()));
        assert_eq!(eval_math_or_unit("10 ft to m"), Some("3.048 m".to_owned()));
    }

    #[test]
    fn invalid_and_bare_numbers_return_none() {
        assert_eq!(eval_math_or_unit(""), None);
        assert_eq!(eval_math_or_unit("2026"), None);
        assert_eq!(eval_math_or_unit("-42"), None);
        assert_eq!(eval_math_or_unit("10 / 0"), None);
        assert_eq!(eval_math_or_unit("10 % 0"), None);
        assert_eq!(eval_math_or_unit("2 + * 3"), None);
        assert_eq!(eval_math_or_unit("(2 + 3"), None);
        assert_eq!(eval_math_or_unit("hello world"), None);
        assert_eq!(eval_math_or_unit("10 km to kg"), None);
    }

    #[test]
    fn fuzzy_scoring_tiers_and_search_ordering() {
        let (Some(exact), Some(prefix), Some(boundary), Some(substring), Some(subseq)) = (
            fuzzy_score("code", "code"),
            fuzzy_score("code", "code editor"),
            fuzzy_score("code", "visual studio code"),
            fuzzy_score("code", "decode tool"),
            fuzzy_score("vsc", "visual studio code"),
        ) else {
            panic!("expected all fuzzy tiers to match");
        };

        assert!(exact > prefix);
        assert!(prefix > boundary);
        assert!(boundary > substring);
        assert!(substring > subseq);
        assert!(fuzzy_score("xyz", "visual studio code").is_none());

        let index = Index::from_apps(vec![
            AppEntry {
                name: "Visual Studio Code".to_owned(),
                name_lower: "visual studio code".to_owned(),
                path: r"C:\Apps\Code.lnk".to_owned(),
            },
            AppEntry {
                name: "Notepad".to_owned(),
                name_lower: "notepad".to_owned(),
                path: r"C:\Apps\Notepad.lnk".to_owned(),
            },
        ]);

        // Empty query returns the 6 quick commands.
        let quick = index.search("", 10);
        assert_eq!(quick.len(), 6);
        assert_eq!(quick[0].kind, EntryKind::Command(SystemAction::OpenSettings));

        // Math result always ranks first.
        let math_hits = index.search("15% of 240", 5);
        assert!(!math_hits.is_empty());
        assert_eq!(
            math_hits[0].kind,
            EntryKind::Math {
                expression: "15% of 240".to_owned(),
                result: "36".to_owned(),
            }
        );

        // System command alias matching.
        let dnd_hits = index.search("dnd", 5);
        assert!(
            dnd_hits
                .iter()
                .any(|e| e.kind == EntryKind::Command(SystemAction::ToggleDoNotDisturb))
        );

        // App matching.
        let code_hits = index.search("vsc", 5);
        assert_eq!(
            code_hits.first().map(|e| e.title.as_str()),
            Some("Visual Studio Code")
        );
    }
}
