//! Read-only plan quota snapshots for locally active AI clients.

use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Local, Utc};
use nosih_shell::icons::IconImage;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use windows::Win32::{
    Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    UI::WindowsAndMessaging::PostMessageW,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Loading,
    Ready,
    SignIn,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    #[serde(default)]
    pub label: Option<String>,
    pub minutes: i64,
    pub used_percent: u8,
    pub resets_at: Option<DateTime<Utc>>,
}

impl Window {
    pub fn title(&self) -> String {
        if let Some(label) = &self.label {
            label.clone()
        } else if self.minutes >= 25 * 24 * 60 {
            "30 d".to_owned()
        } else if self.minutes >= 2 * 24 * 60 {
            "7 d".to_owned()
        } else if self.minutes >= 60 {
            format!("{} h", self.minutes / 60)
        } else {
            format!("{} min", self.minutes)
        }
    }

    pub fn reset_text(&self) -> Option<String> {
        self.resets_at
            .map(|time| time.with_timezone(&Local).format("%d/%m %H:%M").to_string())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub state: State,
    pub windows: Vec<Window>,
    pub fetched_at: Option<SystemTime>,
}

impl Default for ProviderUsage {
    fn default() -> Self {
        Self {
            state: State::Loading,
            windows: Vec::new(),
            fetched_at: None,
        }
    }
}

impl ProviderUsage {
    pub fn compact_text(&self) -> String {
        if let Some(window) = self.main_window() {
            format!("{}%", window.used_percent)
        } else {
            "—".to_owned()
        }
    }

    pub fn main_window(&self) -> Option<&Window> {
        self.windows.first()
    }

    pub fn age_minutes(&self) -> Option<u64> {
        Some(self.fetched_at?.elapsed().ok()?.as_secs() / 60)
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub providers: [ProviderUsage; Provider::COUNT],
}

impl Snapshot {
    pub fn get(&self, provider: Provider) -> &ProviderUsage {
        &self.providers[provider as usize]
    }

    /// OpenCode's process also serves Zen and unrelated providers. Only real
    /// Go or Z.ai quota windows justify their indicator; a running process alone does not.
    pub fn visible_providers(&self, mut active: ActiveProviders) -> ActiveProviders {
        active.0[Provider::OpenCode as usize] &= !self.get(Provider::OpenCode).windows.is_empty();
        active.0[Provider::Zai as usize] &= !self.get(Provider::Zai).windows.is_empty();
        active
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Provider {
    Claude,
    Codex,
    OpenCode,
    Gemini,
    Zai,
}

impl Provider {
    pub const COUNT: usize = 5;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Claude,
        Self::Codex,
        Self::OpenCode,
        Self::Gemini,
        Self::Zai,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode Go",
            Self::Gemini => "Gemini",
            Self::Zai => "Z.ai GLM",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveProviders(pub [bool; Provider::COUNT]);

impl ActiveProviders {
    pub fn any(self) -> bool {
        self.0.iter().any(|active| *active)
    }

    pub fn count(self) -> usize {
        self.0.iter().filter(|active| **active).count()
    }

    pub fn has(self, provider: Provider) -> bool {
        self.0[provider as usize]
    }
}

/// Only show a provider while its local client is running. A stored login by
/// itself does not imply the client is in use.
pub fn active_providers() -> ActiveProviders {
    // SAFETY: The snapshot handle is closed before this function returns.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return ActiveProviders::default();
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut active = ActiveProviders::default();
    // SAFETY: `entry` has the documented size and belongs to this snapshot.
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let name_len = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
            match name.to_ascii_lowercase().as_str() {
                "claude.exe" => {
                    if !active.has(Provider::Claude) {
                        active.0[Provider::Claude as usize] =
                            claude_process_in_use(entry.th32ProcessID);
                    }
                }
                "codex.exe" => active.0[Provider::Codex as usize] = true,
                "opencode.exe" => {
                    active.0[Provider::OpenCode as usize] = true;
                    active.0[Provider::Zai as usize] = true;
                }
                "zai.exe" => active.0[Provider::Zai as usize] = true,
                "gemini.exe" | "antigravity.exe" | "agy.exe" => {
                    active.0[Provider::Gemini as usize] = true;
                }
                _ => {}
            }
            if active.count() == Provider::COUNT {
                break;
            }
            // SAFETY: The snapshot and entry remain valid for this iteration.
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    // SAFETY: `snapshot` came from CreateToolhelp32Snapshot above.
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    active
}

fn claude_process_in_use(pid: u32) -> bool {
    use windows::Win32::{
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowTextLengthW,
            GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
        },
    };
    use windows::core::{BOOL, PWSTR};
    static IMAGES: std::sync::OnceLock<Mutex<std::collections::HashMap<PathBuf, bool>>> =
        std::sync::OnceLock::new();
    let path = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .ok()
            .and_then(|handle| {
                let mut buffer = [0u16; 1024];
                let mut length = buffer.len() as u32;
                let result = QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_WIN32,
                    PWSTR(buffer.as_mut_ptr()),
                    &mut length,
                );
                let _ = CloseHandle(handle);
                result
                    .ok()
                    .map(|()| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
            })
    };
    if let Some(path) = path {
        let cache = IMAGES.get_or_init(Default::default);
        let cached = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&path)
            .copied();
        let console = cached.unwrap_or_else(|| {
            // The CLI and Desktop share a filename. Their PE subsystem reliably
            // distinguishes a console client from Electron, independent of install path.
            let Some(console) = console_image(&path) else {
                return false;
            };
            let mut cache = cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.len() >= 32 {
                cache.clear();
            }
            cache.insert(path, console);
            console
        });
        if console {
            return true;
        }
    }
    // Desktop is in use only while it owns an application window (minimized
    // windows count). Background updaters/renderers do not qualify.
    struct Visible {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = unsafe { &mut *(data.0 as *mut Visible) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
        }
        if owner == state.pid
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_TOOLWINDOW.0 as isize == 0
            && unsafe { GetWindowTextLengthW(hwnd) } > 0
        {
            state.found = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut state = Visible { pid, found: false };
    // SAFETY: EnumWindows invokes this callback synchronously while `state` lives.
    unsafe {
        let _ = EnumWindows(Some(find), LPARAM((&raw mut state) as isize));
    }
    state.found
}

fn console_image(path: &Path) -> Option<bool> {
    let mut file = fs::File::open(path).ok()?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?);
    if !(64..=1024 * 1024).contains(&offset) {
        return None;
    }
    file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
    let mut pe = [0u8; 94];
    file.read_exact(&mut pe).ok()?;
    if &pe[..4] != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes([pe[92], pe[93]]) == 3)
}

pub fn logo(provider: Provider) -> Option<&'static IconImage> {
    static CLAUDE: std::sync::OnceLock<Option<IconImage>> = std::sync::OnceLock::new();
    static CODEX: std::sync::OnceLock<Option<IconImage>> = std::sync::OnceLock::new();
    static OPENCODE: std::sync::OnceLock<Option<IconImage>> = std::sync::OnceLock::new();
    static GEMINI: std::sync::OnceLock<Option<IconImage>> = std::sync::OnceLock::new();
    static ZAI: std::sync::OnceLock<Option<IconImage>> = std::sync::OnceLock::new();
    match provider {
        Provider::Claude => CLAUDE
            .get_or_init(|| decode_logo(include_bytes!("../../../assets/usage/claude.png")))
            .as_ref(),
        Provider::Codex => CODEX
            .get_or_init(|| decode_logo(include_bytes!("../../../assets/usage/codex.png")))
            .as_ref(),
        Provider::OpenCode => OPENCODE
            .get_or_init(|| decode_logo(include_bytes!("../../../assets/usage/opencode.png")))
            .as_ref(),
        Provider::Gemini => GEMINI
            .get_or_init(|| decode_logo(include_bytes!("../../../assets/usage/gemini.png")))
            .as_ref(),
        Provider::Zai => ZAI
            .get_or_init(|| decode_logo(include_bytes!("../../../assets/usage/zai.png")))
            .as_ref(),
    }
}

fn decode_logo(bytes: &[u8]) -> Option<IconImage> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let output = reader.next_frame(&mut rgba).ok()?;
    if output.color_type != png::ColorType::Rgba || output.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let mut bgra = Vec::with_capacity(output.buffer_size());
    for pixel in rgba[..output.buffer_size()].as_chunks::<4>().0 {
        let alpha = u16::from(pixel[3]);
        bgra.extend_from_slice(&[
            (u16::from(pixel[2]) * alpha / 255) as u8,
            (u16::from(pixel[1]) * alpha / 255) as u8,
            (u16::from(pixel[0]) * alpha / 255) as u8,
            pixel[3],
        ]);
    }
    Some(IconImage {
        width: output.width,
        height: output.height,
        bgra,
    })
}

pub struct Monitor {
    current: Arc<Mutex<Snapshot>>,
    running: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    last_started: Option<Instant>,
    last_active: ActiveProviders,
    hwnd: isize,
    changed_message: u32,
}

impl Monitor {
    pub fn new(hwnd: isize, changed_message: u32) -> Self {
        Self {
            current: Arc::new(Mutex::new(Snapshot::default())),
            running: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
            last_started: None,
            last_active: ActiveProviders::default(),
            hwnd,
            changed_message,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn is_refreshing(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub fn mark_inactive(&mut self) {
        self.last_active = ActiveProviders::default();
    }

    pub fn refresh(&mut self, force: bool) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let active = active_providers();
        if !active.any() {
            self.running.store(false, Ordering::Release);
            self.mark_inactive();
            return;
        }
        if !force
            && active == self.last_active
            && self
                .last_started
                .is_some_and(|then| then.elapsed() < REFRESH_INTERVAL)
        {
            self.running.store(false, Ordering::Release);
            return;
        }
        self.last_started = Some(Instant::now());
        self.last_active = active;
        let current = Arc::clone(&self.current);
        let running = Arc::clone(&self.running);
        let alive = Arc::clone(&self.alive);
        let hwnd = self.hwnd;
        let message = self.changed_message;
        thread::spawn(move || {
            let fetched = fetch_in_child();
            {
                let mut stored = current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for (previous, fetched) in stored.providers.iter_mut().zip(fetched.providers) {
                    merge(previous, fetched);
                }
            }
            running.store(false, Ordering::Release);
            if alive.load(Ordering::Acquire) {
                // SAFETY: The window is owned by the live menu bar; posting is asynchronous.
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
                }
            }
        });
    }
}

pub fn worker_main() -> anyhow::Result<()> {
    serde_json::to_writer(std::io::stdout().lock(), &fetch_all())?;
    Ok(())
}

fn fetch_in_child() -> Snapshot {
    let Ok(executable) = env::current_exe() else {
        return unavailable_snapshot();
    };
    let Ok(output) = Command::new(executable)
        .arg("--usage-worker")
        .creation_flags(0x0800_0000)
        .output()
    else {
        return unavailable_snapshot();
    };
    if !output.status.success() {
        return unavailable_snapshot();
    }
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| unavailable_snapshot())
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

fn merge(previous: &mut ProviderUsage, fetched: ProviderUsage) {
    if fetched.state == State::Unavailable && !previous.windows.is_empty() {
        previous.state = State::Unavailable;
    } else {
        *previous = fetched;
    }
}

fn fetch_all() -> Snapshot {
    let Ok(client) = Client::builder().timeout(Duration::from_secs(6)).build() else {
        return unavailable_snapshot();
    };
    let active = active_providers();
    Snapshot {
        providers: std::array::from_fn(|index| {
            let provider = Provider::ALL[index];
            if !active.has(provider) {
                return ProviderUsage::default();
            }
            match provider {
                Provider::Claude => fetch_claude(&client),
                Provider::Codex => fetch_codex(&client),
                Provider::OpenCode => fetch_opencode(&client),
                Provider::Gemini => fetch_gemini(&client),
                Provider::Zai => fetch_zai(&client),
            }
        }),
    }
}

fn unavailable_snapshot() -> Snapshot {
    Snapshot {
        providers: std::array::from_fn(|_| unavailable()),
    }
}

fn unavailable() -> ProviderUsage {
    ProviderUsage {
        state: State::Unavailable,
        ..ProviderUsage::default()
    }
}

fn sign_in() -> ProviderUsage {
    ProviderUsage {
        state: State::SignIn,
        ..ProviderUsage::default()
    }
}

fn ready(windows: Vec<Window>) -> ProviderUsage {
    ProviderUsage {
        state: State::Ready,
        windows,
        fetched_at: Some(SystemTime::now()),
    }
}

fn home() -> Option<PathBuf> {
    env::var_os("USERPROFILE").map(PathBuf::from)
}

fn fetch_claude(client: &Client) -> ProviderUsage {
    let Some(root) = env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home().map(|path| path.join(".claude")))
    else {
        return sign_in();
    };
    let Ok(contents) = fs::read(root.join(".credentials.json")) else {
        return sign_in();
    };
    let Ok(credentials) = serde_json::from_slice::<Value>(&contents) else {
        return unavailable();
    };
    let Some(token) = credentials
        .pointer("/claudeAiOauth/accessToken")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
    else {
        return sign_in();
    };
    let Ok(response) = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
    else {
        return unavailable();
    };
    if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
        return sign_in();
    }
    let Ok(body) = response.error_for_status().and_then(|value| value.text()) else {
        return unavailable();
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return unavailable();
    };
    let windows = parse_claude(&value);
    if windows.is_empty() {
        unavailable()
    } else {
        ready(windows)
    }
}

fn fetch_codex(client: &Client) -> ProviderUsage {
    let Some(root) = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|path| path.join(".codex")))
    else {
        return sign_in();
    };
    let Ok(contents) = fs::read(root.join("auth.json")) else {
        return local_codex(&root).unwrap_or_else(sign_in);
    };
    let Ok(credentials) = serde_json::from_slice::<Value>(&contents) else {
        return local_codex(&root).unwrap_or_else(unavailable);
    };
    let Some(token) = credentials
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
    else {
        return local_codex(&root).unwrap_or_else(sign_in);
    };
    let mut request = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(token);
    if let Some(account) = credentials
        .pointer("/tokens/account_id")
        .and_then(Value::as_str)
        .filter(|account| !account.is_empty())
    {
        request = request.header("ChatGPT-Account-ID", account);
    }
    let Ok(response) = request.send() else {
        return local_codex(&root).unwrap_or_else(unavailable);
    };
    if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
        return local_codex(&root).unwrap_or_else(sign_in);
    }
    let Ok(body) = response.error_for_status().and_then(|value| value.text()) else {
        return local_codex(&root).unwrap_or_else(unavailable);
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return local_codex(&root).unwrap_or_else(unavailable);
    };
    let windows = parse_codex(&value);
    if windows.is_empty() {
        local_codex(&root).unwrap_or_else(|| ready(windows))
    } else {
        ready(windows)
    }
}

fn fetch_opencode(client: &Client) -> ProviderUsage {
    let api_key = env::var("OPENCODE_API_KEY").ok();
    let data_home = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = home();
    opencode_token(api_key.as_deref(), data_home.as_deref(), home.as_deref())
        .as_deref()
        .map_or_else(sign_in, |token| fetch_opencode_go(client, token))
}

fn fetch_opencode_go(client: &Client, token: &str) -> ProviderUsage {
    let Ok(response) = client
        .get("https://opencode.ai/zen/go/v1/usage")
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
    else {
        return unavailable();
    };
    if response.status().as_u16() == 401 {
        return sign_in();
    }
    let Ok(body) = response
        .error_for_status()
        .and_then(|response| response.text())
    else {
        return unavailable();
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return unavailable();
    };
    let windows = parse_opencode(&value);
    if windows.is_empty() {
        unavailable()
    } else {
        ready(windows)
    }
}

fn opencode_auth_path(data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    // OpenCode follows XDG on native Windows too. An explicit data directory
    // is authoritative: never silently reuse a different account from HOME.
    let data = match data_home.filter(|path| !path.as_os_str().is_empty()) {
        Some(path) => {
            if !path.is_absolute() {
                return None;
            }
            path.to_path_buf()
        }
        None => home?.join(".local/share"),
    };
    Some(data.join("opencode/auth.json"))
}

fn opencode_key<'a>(api_key: Option<&'a str>, auth: Option<&'a Value>) -> Option<&'a str> {
    api_key.and_then(opencode_usable_key).or_else(|| {
        // Zen keys do not establish a Go subscription. Never probe a saved
        // Zen-only connection or forward another provider's credentials.
        opencode_entry(auth?, "opencode-go")
    })
}

fn opencode_usable_key(key: &str) -> Option<&str> {
    let key = key.trim();
    (!key.is_empty()
        && key != "opencode-oauth-dummy-key"
        && !key
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control()))
    .then_some(key)
}

fn opencode_entry<'a>(auth: &'a Value, id: &str) -> Option<&'a str> {
    let entry = auth.get(id)?;
    (entry.get("type")?.as_str()? == "api")
        .then(|| entry.get("key")?.as_str().and_then(opencode_usable_key))?
}

fn read_opencode_auth(path: &Path) -> Option<Value> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(256 * 1024).read_to_end(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn opencode_token(
    api_key: Option<&str>,
    data_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<String> {
    if let Some(key) = opencode_key(api_key, None) {
        return Some(key.to_owned());
    }
    let path = opencode_auth_path(data_home, home)?;
    let auth = read_opencode_auth(&path)?;
    opencode_key(None, Some(&auth)).map(str::to_owned)
}

fn parse_opencode(value: &Value) -> Vec<Window> {
    let Some(usage) = value.get("usage") else {
        return Vec::new();
    };
    [
        ("rolling", 300),
        ("weekly", 10_080),
        ("monthly", 30 * 24 * 60),
    ]
    .into_iter()
    .filter_map(|(name, minutes)| {
        let lane = usage.get(name)?;
        let used_percent = lane.get("percent").and_then(percent).or_else(|| {
            let used = lane.get("used")?.as_f64()?;
            let limit = lane.get("limit")?.as_f64()?;
            (limit > 0.0).then(|| (used * 100.0 / limit).clamp(0.0, 100.0).round() as u8)
        })?;
        let resets_at = lane
            .get("resetsAt")
            .or_else(|| lane.get("resets_at"))
            .and_then(Value::as_str)
            .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&Utc))
            .or_else(|| {
                lane.get("resetInSec")
                    .or_else(|| lane.get("reset_in_sec"))
                    .and_then(Value::as_i64)
                    .filter(|seconds| *seconds >= 0)
                    .and_then(|seconds| Utc::now().timestamp().checked_add(seconds))
                    .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            });
        Some(Window {
            label: None,
            minutes,
            used_percent,
            resets_at,
        })
    })
    .collect()
}

fn fetch_zai(client: &Client) -> ProviderUsage {
    let api_key = env::var("ZAI_API_KEY")
        .ok()
        .or_else(|| env::var("ZHIPU_API_KEY").ok());
    let data_home = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = home();
    zai_token(api_key.as_deref(), data_home.as_deref(), home.as_deref())
        .as_deref()
        .map_or_else(sign_in, |token| fetch_zai_quota(client, token))
}

fn fetch_zai_quota(client: &Client, token: &str) -> ProviderUsage {
    for endpoint in [
        "https://api.z.ai/api/monitor/usage/quota/limit",
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit",
    ] {
        let Ok(response) = client
            .get(endpoint)
            .header("Authorization", token)
            .header("Accept", "application/json")
            .send()
        else {
            continue;
        };
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return sign_in();
        }
        let Ok(body) = response.error_for_status().and_then(|resp| resp.text()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        let windows = parse_zai(&value);
        if !windows.is_empty() {
            return ready(windows);
        }
    }
    unavailable()
}

fn zai_auth_paths(data_home: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = data_home.filter(|p| !p.as_os_str().is_empty()) {
        if path.is_absolute() {
            paths.push(path.join("opencode/auth.json"));
        }
    } else if let Some(home) = home {
        paths.push(home.join(".local/share/opencode/auth.json"));
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let p = local_app_data.join("opencode/auth.json");
        if !paths.contains(&p) {
            paths.push(p);
        }
    }
    if let Some(app_data) = env::var_os("APPDATA").map(PathBuf::from) {
        let p = app_data.join("opencode/auth.json");
        if !paths.contains(&p) {
            paths.push(p);
        }
    }
    paths
}

fn zai_usable_key(key: &str) -> Option<&str> {
    let key = key.trim();
    (!key.is_empty()
        && key != "opencode-oauth-dummy-key"
        && !key
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control()))
    .then_some(key)
}

fn zai_entry<'a>(auth: &'a Value, id: &str) -> Option<&'a str> {
    let entry = auth.get(id)?;
    if entry
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t != "api")
    {
        return None;
    }
    entry.get("key")?.as_str().and_then(zai_usable_key)
}

fn zai_key<'a>(api_key: Option<&'a str>, auth: Option<&'a Value>) -> Option<&'a str> {
    api_key.and_then(zai_usable_key).or_else(|| {
        let auth = auth?;
        zai_entry(auth, "zai-coding-plan").or_else(|| zai_entry(auth, "zai"))
    })
}

fn zai_token(
    api_key: Option<&str>,
    data_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<String> {
    if let Some(key) = zai_key(api_key, None) {
        return Some(key.to_owned());
    }
    for path in zai_auth_paths(data_home, home) {
        if let Some(auth) = read_opencode_auth(&path)
            && let Some(key) = zai_key(None, Some(&auth))
        {
            return Some(key.to_owned());
        }
    }
    None
}

fn parse_zai(value: &Value) -> Vec<Window> {
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code != 200 {
        return Vec::new();
    }
    let Some(limits) = value.pointer("/data/limits").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for entry in limits {
        let limit_type = entry.get("type").and_then(Value::as_str).unwrap_or("");
        if limit_type != "TOKENS_LIMIT" {
            continue;
        }
        let unit = entry.get("unit").and_then(Value::as_i64);
        let number = entry.get("number").and_then(Value::as_i64);
        let minutes = match (unit, number) {
            (Some(3), Some(5)) => 300,
            (Some(6), Some(1)) => 10_080,
            _ => continue,
        };
        let used_percent = entry.get("percentage").and_then(percent).or_else(|| {
            let current = entry
                .get("currentValue")
                .and_then(Value::as_f64)
                .or_else(|| entry.get("usage").and_then(Value::as_f64))?;
            let remaining = entry.get("remaining").and_then(Value::as_f64)?;
            let total = current + remaining;
            (total > 0.0).then(|| (current * 100.0 / total).clamp(0.0, 100.0).round() as u8)
        });
        let Some(used_percent) = used_percent else {
            continue;
        };
        let resets_at = entry
            .get("nextResetTime")
            .and_then(Value::as_i64)
            .and_then(DateTime::from_timestamp_millis);
        windows.push(Window {
            label: None,
            minutes,
            used_percent,
            resets_at,
        });
    }
    windows.sort_unstable_by_key(|window| window.minutes);
    windows
}

fn fetch_gemini(client: &Client) -> ProviderUsage {
    // Antigravity and Gemini CLI have distinct accounts and quotas. Prefer the
    // running desktop app's own local quota service when it is available.
    if let Some(usage) = fetch_antigravity() {
        return usage;
    }
    let Some(path) = home().map(|path| path.join(".gemini/oauth_creds.json")) else {
        return unavailable();
    };
    let Ok(contents) = fs::read(path) else {
        return unavailable();
    };
    let Ok(auth) = serde_json::from_slice::<Value>(&contents) else {
        return unavailable();
    };
    let Some(token) = auth
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
    else {
        return sign_in();
    };
    if auth
        .get("expiry_date")
        .and_then(Value::as_i64)
        .is_some_and(|expiry_ms| expiry_ms < Utc::now().timestamp_millis() + 60_000)
    {
        return sign_in();
    }
    let project = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist")
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(r#"{"metadata":{"ideType":"GEMINI_CLI","pluginType":"GEMINI"}}"#)
        .send()
        .ok()
        .filter(|response| response.status().is_success())
        .and_then(|response| response.text().ok())
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .and_then(|value| value.get("cloudaicompanionProject").cloned())
        .and_then(|value| value.as_str().map(str::to_owned));
    let body = project.map_or_else(
        || "{}".to_owned(),
        |project| serde_json::json!({"project": project}).to_string(),
    );
    let Ok(response) = client
        .post("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota")
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
    else {
        return unavailable();
    };
    if matches!(response.status().as_u16(), 401 | 403) {
        return unavailable();
    }
    let Ok(body) = response
        .error_for_status()
        .and_then(|response| response.text())
    else {
        return unavailable();
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return unavailable();
    };
    let windows = parse_gemini(&value);
    if windows.is_empty() {
        unavailable()
    } else {
        ready(windows)
    }
}

fn fetch_antigravity() -> Option<ProviderUsage> {
    let daemon = home()?.join(".gemini/antigravity/daemon");
    let mut files = fs::read_dir(daemon)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("hub_") && name.ends_with(".json"))
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect::<Vec<_>>();
    files.sort_unstable_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let client = Client::builder()
        .timeout(Duration::from_millis(700))
        .danger_accept_invalid_certs(true)
        .build()
        .ok()?;
    for (_, path) in files.into_iter().take(8) {
        let Ok(contents) = fs::read(path) else {
            continue;
        };
        let Ok(info) = serde_json::from_slice::<Value>(&contents) else {
            continue;
        };
        let Some(token) = info.get("csrfToken").and_then(Value::as_str) else {
            continue;
        };
        if token.is_empty() || token.len() > 512 {
            continue;
        }
        for (scheme, port) in [
            ("https", info.get("httpsPort").and_then(Value::as_u64)),
            ("http", info.get("httpPort").and_then(Value::as_u64)),
        ] {
            let Some(port) = port.filter(|port| (1..=65535).contains(port)) else {
                continue;
            };
            for method in [
                "RetrieveUserQuotaSummary",
                "GetUserStatus",
                "GetCommandModelConfigs",
            ] {
                let endpoint = format!(
                    "{scheme}://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/{method}"
                );
                let Ok(response) = client
                    .post(endpoint)
                    .header("X-Codeium-Csrf-Token", token)
                    .header("Connect-Protocol-Version", "1")
                    .header("Content-Type", "application/json")
                    .body(r#"{"metadata":{"ideName":"antigravity","extensionName":"antigravity","locale":"en","ideVersion":"unknown"}}"#)
                    .send()
                else {
                    continue;
                };
                let Ok(body) = response.error_for_status().and_then(|value| value.text()) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&body) else {
                    continue;
                };
                let windows = if method == "RetrieveUserQuotaSummary" {
                    parse_antigravity(&value)
                } else {
                    parse_antigravity_legacy(&value)
                };
                if !windows.is_empty() {
                    return Some(ready(windows));
                }
            }
        }
    }
    None
}

fn parse_antigravity(value: &Value) -> Vec<Window> {
    let groups = value
        .pointer("/response/groups")
        .or_else(|| value.get("groups"))
        .and_then(Value::as_array);
    let Some(groups) = groups else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for group in groups {
        let name = group
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !name.to_ascii_lowercase().contains("gemini") {
            continue;
        }
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        for bucket in buckets {
            let id = bucket.get("bucketId").and_then(Value::as_str).unwrap_or("");
            let cadence = bucket.get("window").and_then(Value::as_str).unwrap_or(id);
            let minutes = match cadence.to_ascii_lowercase().as_str() {
                "5h" | "gemini-5h" => 300,
                "weekly" | "week" | "7d" | "gemini-weekly" => 10_080,
                _ => continue,
            };
            let fraction = bucket
                .pointer("/remaining/remainingFraction")
                .or_else(|| bucket.get("remainingFraction"))
                .and_then(Value::as_f64);
            let Some(fraction) =
                fraction.filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
            else {
                continue;
            };
            let resets_at = bucket
                .pointer("/remaining/resetTime")
                .or_else(|| bucket.get("resetTime"))
                .and_then(Value::as_str)
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc));
            windows.push(Window {
                label: None,
                minutes,
                used_percent: ((1.0 - fraction) * 100.0).round() as u8,
                resets_at,
            });
        }
    }
    windows.sort_unstable_by_key(|window| window.minutes);
    windows
}

fn parse_antigravity_legacy(value: &Value) -> Vec<Window> {
    let configs = value
        .pointer("/userStatus/cascadeModelConfigData/clientModelConfigs")
        .or_else(|| value.get("clientModelConfigs"))
        .and_then(Value::as_array);
    let Some(configs) = configs else {
        return Vec::new();
    };
    ["Pro", "Flash"]
        .into_iter()
        .filter_map(|family| {
            let (_, model) = configs
                .iter()
                .filter(|model| {
                    let label = model
                        .get("label")
                        .or_else(|| model.get("displayName"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    label.contains("gemini")
                        && label.contains(&family.to_ascii_lowercase())
                        && !label.contains("lite")
                        && !label.contains("image")
                })
                .filter_map(|model| {
                    let fraction = model.pointer("/quotaInfo/remainingFraction")?.as_f64()?;
                    (fraction.is_finite() && (0.0..=1.0).contains(&fraction))
                        .then_some((fraction, model))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0))?;
            let fraction = model.pointer("/quotaInfo/remainingFraction")?.as_f64()?;
            let resets_at = model
                .pointer("/quotaInfo/resetTime")
                .and_then(Value::as_str)
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc));
            Some(Window {
                label: Some(family.to_owned()),
                minutes: 0,
                used_percent: ((1.0 - fraction) * 100.0).round() as u8,
                resets_at,
            })
        })
        .collect()
}

fn parse_gemini(value: &Value) -> Vec<Window> {
    let Some(buckets) = value.get("buckets").and_then(Value::as_array) else {
        return Vec::new();
    };
    ["Pro", "Flash"]
        .into_iter()
        .filter_map(|family| {
            let bucket = buckets
                .iter()
                .filter(|bucket| {
                    bucket
                        .get("modelId")
                        .and_then(Value::as_str)
                        .is_some_and(|model| {
                            let model = model.to_ascii_lowercase();
                            if family == "Pro" {
                                model.contains("pro")
                            } else {
                                model.contains("flash") && !model.contains("flash-lite")
                            }
                        })
                })
                .filter_map(|bucket| {
                    let remaining = bucket.get("remainingFraction")?.as_f64()?;
                    (remaining.is_finite() && (0.0..=1.0).contains(&remaining))
                        .then_some((remaining, bucket))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0))?;
            let resets_at = bucket
                .1
                .get("resetTime")
                .and_then(Value::as_str)
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc));
            Some(Window {
                label: Some(family.to_owned()),
                minutes: 0,
                used_percent: ((1.0 - bucket.0) * 100.0).round() as u8,
                resets_at,
            })
        })
        .collect()
}

fn local_codex(root: &std::path::Path) -> Option<ProviderUsage> {
    let mut recent = Vec::new();
    let mut pending = vec![root.join("sessions"), root.join("archived_sessions")];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                if let Some(modified) = modified {
                    recent.push((modified, path));
                }
            }
        }
    }
    recent.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in recent.into_iter().take(8) {
        if let Some(snapshot) = scan_rollout(&path) {
            return Some(snapshot);
        }
    }
    None
}

fn scan_rollout(path: &std::path::Path) -> Option<ProviderUsage> {
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(256 * 1024);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let lines = tail.split(|byte| *byte == b'\n');
    for line in lines.rev() {
        if !line.windows(13).any(|part| part == b"\"rate_limits\"") {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if record.pointer("/payload/type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(limits) = record.pointer("/payload/rate_limits") else {
            continue;
        };
        let windows = parse_local_codex(limits);
        if windows.is_empty() {
            continue;
        }
        let fetched_at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
            .and_then(|time| u64::try_from(time.timestamp()).ok())
            .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds));
        return Some(ProviderUsage {
            state: State::Ready,
            windows,
            fetched_at,
        });
    }
    None
}

fn parse_local_codex(limits: &Value) -> Vec<Window> {
    ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let entry = limits.get(key)?;
            let used_percent = percent(entry.get("used_percent")?)?;
            let minutes = entry.get("window_minutes")?.as_i64()?;
            let resets_at = entry
                .get("resets_at")
                .and_then(Value::as_i64)
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0));
            Some(Window {
                label: None,
                minutes,
                used_percent,
                resets_at,
            })
        })
        .collect()
}

fn percent(value: &Value) -> Option<u8> {
    let number = value.as_f64()?;
    (number.is_finite() && (0.0..=100.0).contains(&number)).then(|| number.round() as u8)
}

fn parse_claude(value: &Value) -> Vec<Window> {
    [("five_hour", 300), ("seven_day", 10_080)]
        .into_iter()
        .filter_map(|(key, minutes)| {
            let entry = value.get(key)?;
            let used_percent = percent(entry.get("utilization")?)?;
            let resets_at = entry
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                .map(|time| time.with_timezone(&Utc));
            Some(Window {
                label: None,
                minutes,
                used_percent,
                resets_at,
            })
        })
        .collect()
}

fn parse_codex(value: &Value) -> Vec<Window> {
    ["primary_window", "secondary_window"]
        .into_iter()
        .filter_map(|key| {
            let entry = value.get("rate_limit")?.get(key)?;
            let used_percent = percent(entry.get("used_percent")?)?;
            let minutes = entry.get("limit_window_seconds")?.as_i64()? / 60;
            let resets_at = entry
                .get("reset_at")
                .and_then(Value::as_i64)
                .and_then(|seconds| DateTime::from_timestamp(seconds, 0));
            Some(Window {
                label: None,
                minutes,
                used_percent,
                resets_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_native_console_and_desktop_images_are_distinguished_without_path_guesses()
    -> anyhow::Result<()> {
        let path = env::temp_dir().join(format!("nosih-claude-pe-{}.exe", std::process::id()));
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let file = Fixture(path);
        let mut image = vec![0u8; 64 + 94];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&64u32.to_le_bytes());
        image[64..68].copy_from_slice(b"PE\0\0");
        for (subsystem, console) in [(2u16, false), (3, true)] {
            image[156..158].copy_from_slice(&subsystem.to_le_bytes());
            fs::write(&file.0, &image)?;
            assert_eq!(console_image(&file.0), Some(console));
        }
        fs::write(&file.0, b"MZ")?;
        assert_eq!(console_image(&file.0), None);
        Ok(())
    }

    #[test]
    fn claude_real_quota_preserves_zero_and_reset_times() {
        let windows = parse_claude(&json!({
            "five_hour": {"utilization":0.0,"resets_at":"2026-11-23T03:00:00Z"},
            "seven_day": {"utilization":32.0,"resets_at":"2026-11-29T03:00:00Z"}
        }));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].used_percent, 0);
        assert!(windows[0].resets_at.is_some());
    }

    #[test]
    fn opencode_go_reads_official_absolute_resets_and_rejects_overflowing_legacy_dates() {
        let windows = parse_opencode(&json!({"usage": {
            "rolling": {"percent": 15, "resetsAt": "2026-11-20T22:00:00Z"},
            "weekly": {"percent": 35, "resets_at": "2026-11-23T00:00:00Z"},
            "monthly": {"percent": 50, "resetInSec": i64::MAX}
        }}));
        assert_eq!(windows.len(), 3);
        assert_eq!(
            windows[0]
                .resets_at
                .map(|time| time.to_rfc3339())
                .as_deref(),
            Some("2026-11-20T22:00:00+00:00")
        );
        assert!(windows[1].resets_at.is_some());
        assert!(windows[2].resets_at.is_none());
    }

    #[test]
    fn opencode_only_shows_measured_go_quotas_for_a_running_client() {
        let mut snapshot = Snapshot::default();
        let mut active = ActiveProviders::default();
        active.0[Provider::OpenCode as usize] = true;
        active.0[Provider::Claude as usize] = true;
        assert!(!snapshot.visible_providers(active).has(Provider::OpenCode));
        assert!(snapshot.visible_providers(active).has(Provider::Claude));
        snapshot.providers[Provider::OpenCode as usize] = ready(parse_opencode(&json!({"usage": {
            "rolling": {"percent": 15}, "weekly": {"percent": 35}, "monthly": {"percent": 50}
        }})));
        assert!(snapshot.visible_providers(active).has(Provider::OpenCode));
        assert_eq!(Provider::OpenCode.name(), "OpenCode Go");
        assert_eq!(snapshot.get(Provider::OpenCode).compact_text(), "15%");
        assert_eq!(snapshot.get(Provider::OpenCode).windows.len(), 3);
        active.0[Provider::OpenCode as usize] = false;
        assert!(!snapshot.visible_providers(active).has(Provider::OpenCode));
    }

    #[test]
    fn opencode_missing_or_invalid_windows_never_become_a_quota() {
        for value in [
            json!({"balance": 20}),
            json!({"usage": {}}),
            json!({"usage": {"rolling": {"percent": null}}}),
            json!({"usage": {"rolling": {"percent": -1}}}),
            json!({"usage": {"rolling": {"percent": 150}}}),
        ] {
            assert!(parse_opencode(&value).is_empty());
        }
        let zero = parse_opencode(&json!({"usage": {"rolling": {"percent": 0}}}));
        assert_eq!(zero.len(), 1);
        assert_eq!(zero[0].used_percent, 0);
    }

    #[test]
    fn opencode_failed_refresh_keeps_measured_quotas_stale_but_logout_clears_them() {
        let mut previous = ready(parse_opencode(
            &json!({"usage": {"rolling": {"percent": 42}}}),
        ));
        let fetched_at = previous.fetched_at;
        merge(&mut previous, unavailable());
        assert!(previous.state == State::Unavailable);
        assert_eq!(previous.compact_text(), "42%");
        assert_eq!(previous.fetched_at, fetched_at);
        merge(&mut previous, sign_in());
        assert!(previous.windows.is_empty());
    }

    #[test]
    fn claude_missing_window_does_not_become_zero() {
        let windows = parse_claude(
            &json!({"five_hour": null, "seven_day": {"utilization": 24.6, "resets_at": "2026-11-16T13:00:00Z"}}),
        );
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_percent, 25);
    }

    #[test]
    fn codex_weekly_only_primary_keeps_its_duration() {
        let windows = parse_codex(
            &json!({"rate_limit": {"primary_window": {"used_percent": 61, "limit_window_seconds": 604800, "reset_at": 1794834000}, "secondary_window": null}}),
        );
        assert_eq!(windows[0].title(), "7 d");
    }

    #[test]
    fn invalid_percentage_is_ignored() {
        let windows = parse_codex(
            &json!({"rate_limit": {"primary_window": {"used_percent": 143, "limit_window_seconds": 18000}}}),
        );
        assert!(windows.is_empty());
    }

    #[test]
    fn local_codex_weekly_window_is_classified_by_duration() {
        let windows = parse_local_codex(
            &json!({"primary": {"used_percent": 33, "window_minutes": 10080, "resets_at": 1794834000}}),
        );
        assert_eq!(windows[0].title(), "7 d");
    }

    #[test]
    fn opencode_ignores_unmeasured_lanes() {
        let windows = parse_opencode(&json!({"usage": {
            "rolling": {"percent": 19, "resetInSec": 1200},
            "weekly": {"resetInSec": 6000},
            "monthly": {"percent": 33}
        }}));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].title(), "5 h");
        assert!(windows[0].resets_at.is_some());
        assert_eq!(windows[1].title(), "30 d");
    }

    #[test]
    fn opencode_go_credentials_ignore_saved_zen_keys() {
        let auth = json!({
            "opencode-go": {"type": "api", "key": "go-fixture"},
            "opencode": {"type": "api", "key": "zen-fixture"}
        });
        assert_eq!(opencode_key(None, Some(&auth)), Some("go-fixture"));
        assert_eq!(
            opencode_key(Some("explicit-fixture"), Some(&auth)),
            Some("explicit-fixture")
        );
        assert_eq!(opencode_key(Some("  "), Some(&auth)), Some("go-fixture"));
    }

    #[test]
    fn opencode_never_reuses_oauth_dummy_keys_or_another_providers_credentials() {
        let auth = json!({
            "opencode-go": {"type": "oauth", "key": "wrong-type", "access": "oauth-fixture"},
            "opencode": {"type": "api", "key": "opencode-oauth-dummy-key"},
            "openai": {"type": "api", "key": "unrelated-fixture"}
        });
        assert_eq!(opencode_key(None, Some(&auth)), None);
        assert_eq!(opencode_key(Some("invalid\r\nheader"), Some(&auth)), None);
        let legacy = json!({"opencode": {"type": "api", "key": " legacy-fixture "}});
        assert_eq!(opencode_key(None, Some(&legacy)), None);
    }

    #[test]
    fn opencode_reads_native_windows_xdg_storage_without_changing_environment() -> anyhow::Result<()>
    {
        let root = env::temp_dir().join(format!(
            "nosih-opencode-auth-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        // This test owns only this newly created temporary directory.
        fs::create_dir(&root)?;
        let result = (|| -> anyhow::Result<()> {
            let data = root.join("custom-data");
            let home = root.join("home");
            let path = opencode_auth_path(Some(&data), Some(&home))
                .ok_or_else(|| anyhow::anyhow!("missing fixture path"))?;
            fs::create_dir_all(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("missing fixture parent"))?,
            )?;
            let original = br#"{"opencode-go":{"type":"api","key":"saved-go-fixture"}}"#;
            fs::write(&path, original)?;
            assert_eq!(
                opencode_token(None, Some(&data), Some(&home)).as_deref(),
                Some("saved-go-fixture")
            );
            assert_eq!(fs::read(&path)?, original);
            let default_path = home.join(".local/share/opencode/auth.json");
            assert_eq!(
                opencode_auth_path(None, Some(&home)),
                Some(default_path.clone())
            );
            fs::create_dir_all(
                default_path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("missing default parent"))?,
            )?;
            fs::write(
                &default_path,
                br#"{"opencode":{"type":"api","key":"other-account-fixture"}}"#,
            )?;
            assert!(opencode_token(None, None, Some(&home)).is_none());
            fs::write(
                &default_path,
                br#"{"opencode-go":{"type":"api","key":"other-go-account-fixture"}}"#,
            )?;
            assert_eq!(
                opencode_token(None, None, Some(&home)).as_deref(),
                Some("other-go-account-fixture")
            );
            // A missing explicit account must not fall back to another directory.
            assert!(opencode_token(None, Some(&root.join("missing")), Some(&home)).is_none());
            assert!(opencode_auth_path(Some(Path::new("relative-store")), Some(&home)).is_none());
            // An explicitly configured environment key works without HOME.
            assert_eq!(
                opencode_token(Some("env-fixture"), None, None).as_deref(),
                Some("env-fixture")
            );
            Ok(())
        })();
        fs::remove_dir_all(&root)?;
        result
    }

    #[test]
    fn antigravity_only_reports_measured_gemini_buckets() {
        let windows = parse_antigravity(&json!({"response": {"groups": [
            {"displayName": "Gemini Models", "buckets": [
                {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.75, "resetTime": "2026-11-20T00:00:00Z"},
                {"bucketId": "gemini-5h", "window": "5h", "remaining": {"remainingFraction": 0.2, "resetTime": "2026-11-15T20:00:00Z"}},
                {"bucketId": "gemini-missing", "window": "5h"}
            ]},
            {"displayName": "Claude and GPT models", "buckets": [
                {"bucketId": "3p-5h", "window": "5h", "remainingFraction": 0.1}
            ]}
        ]}}));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].title(), "5 h");
        assert_eq!(windows[0].used_percent, 80);
        assert_eq!(windows[1].title(), "7 d");
        assert_eq!(windows[1].used_percent, 25);
    }

    #[test]
    fn antigravity_ide_uses_model_quota_without_inventing_cadence() {
        let windows = parse_antigravity_legacy(&json!({"userStatus": {
            "cascadeModelConfigData": {"clientModelConfigs": [
                {"label": "Gemini 3 Pro (High)", "quotaInfo": {"remainingFraction": 0.4, "resetTime": "2026-11-16T02:00:00Z"}},
                {"label": "Gemini 3 Flash", "quotaInfo": {"remainingFraction": 0.9}},
                {"label": "Claude Sonnet", "quotaInfo": {"remainingFraction": 0.1}},
                {"label": "Gemini 3 Flash Lite", "quotaInfo": {"remainingFraction": 0.05}}
            ]}
        }}));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].title(), "Pro");
        assert_eq!(windows[0].used_percent, 60);
        assert_eq!(windows[1].title(), "Flash");
        assert_eq!(windows[1].used_percent, 10);
    }

    #[test]
    fn gemini_cli_keeps_pro_and_flash_distinct() {
        let windows = parse_gemini(&json!({"buckets": [
            {"modelId": "gemini-3-pro", "remainingFraction": 0.4, "resetTime": "2026-11-16T00:00:00Z"},
            {"modelId": "gemini-3-flash", "remainingFraction": 0.9},
            {"modelId": "gemini-3-flash-lite", "remainingFraction": 0.1}
        ]}));
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].title(), "Pro");
        assert_eq!(windows[0].used_percent, 60);
        assert_eq!(windows[1].title(), "Flash");
        assert_eq!(windows[1].used_percent, 10);
    }

    #[test]
    fn zai_coding_plan_parses_real_sample_fixture_with_five_hour_and_weekly_windows() {
        let fixture = json!({
            "code": 200,
            "msg": "Operation successful",
            "success": true,
            "data": {
                "level": "max",
                "limits": [
                    { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 10, "nextResetTime": 1795286560510_i64 },
                    { "type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 38, "nextResetTime": 1795744817983_i64 },
                    { "type": "TIME_LIMIT", "unit": 5, "number": 1, "usage": 4000, "currentValue": 0, "remaining": 4000,
                      "percentage": 0, "nextResetTime": 1797818417999_i64,
                      "usageDetails": [ {"modelCode":"search-prime","usage":0}, {"modelCode":"web-reader","usage":0}, {"modelCode":"zread","usage":0} ] }
                ]
            }
        });
        let windows = parse_zai(&fixture);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].title(), "5 h");
        assert_eq!(windows[0].used_percent, 10);
        assert_eq!(
            windows[0].resets_at,
            DateTime::from_timestamp_millis(1795286560510)
        );
        assert_eq!(windows[1].title(), "7 d");
        assert_eq!(windows[1].used_percent, 38);
        assert_eq!(
            windows[1].resets_at,
            DateTime::from_timestamp_millis(1795744817983)
        );
    }

    #[test]
    fn zai_handles_missing_windows_unknown_types_and_unsuccessful_envelope() {
        let unsucc = json!({
            "code": 200,
            "msg": "failed",
            "success": false,
            "data": { "limits": [{ "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 10, "nextResetTime": 1795286560510_i64 }] }
        });
        assert!(parse_zai(&unsucc).is_empty());

        let err_code = json!({
            "code": 500,
            "msg": "error",
            "success": true,
            "data": { "limits": [{ "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 10, "nextResetTime": 1795286560510_i64 }] }
        });
        assert!(parse_zai(&err_code).is_empty());

        let unknown = json!({
            "code": 200,
            "success": true,
            "data": {
                "limits": [
                    { "type": "CREDIT_LIMIT", "unit": 3, "number": 5, "percentage": 50 },
                    { "type": "TOKENS_LIMIT", "unit": 99, "number": 1, "percentage": 25 },
                    { "type": "TOKENS_LIMIT", "unit": 3, "number": 99, "percentage": 75 },
                    { "type": "UNKNOWN_LIMIT", "unit": 6, "number": 1, "percentage": 30 }
                ]
            }
        });
        assert!(parse_zai(&unknown).is_empty());

        assert!(parse_zai(&json!({"code": 200, "success": true, "data": {}})).is_empty());
        assert!(
            parse_zai(&json!({"code": 200, "success": true, "data": {"limits": null}})).is_empty()
        );

        let single = json!({
            "code": 200,
            "success": true,
            "data": {
                "limits": [
                    { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 42 }
                ]
            }
        });
        let single_windows = parse_zai(&single);
        assert_eq!(single_windows.len(), 1);
        assert_eq!(single_windows[0].title(), "5 h");
        assert_eq!(single_windows[0].used_percent, 42);
        assert!(single_windows[0].resets_at.is_none());
    }

    #[test]
    fn zai_clamps_percentage_and_computes_fallback_from_usage_details() {
        let invalid = json!({
            "code": 200,
            "success": true,
            "data": {
                "limits": [
                    { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 150 },
                    { "type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": -5 }
                ]
            }
        });
        assert!(parse_zai(&invalid).is_empty());

        let fallback = json!({
            "code": 200,
            "success": true,
            "data": {
                "limits": [
                    { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "currentValue": 250, "remaining": 750 }
                ]
            }
        });
        let windows = parse_zai(&fallback);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_percent, 25);
    }

    #[test]
    fn zai_only_shows_measured_quotas_for_a_running_client() {
        let mut snapshot = Snapshot::default();
        let mut active = ActiveProviders::default();
        active.0[Provider::Zai as usize] = true;
        assert!(!snapshot.visible_providers(active).has(Provider::Zai));

        snapshot.providers[Provider::Zai as usize] = ready(vec![Window {
            label: None,
            minutes: 300,
            used_percent: 15,
            resets_at: None,
        }]);
        assert!(snapshot.visible_providers(active).has(Provider::Zai));
        assert_eq!(Provider::Zai.name(), "Z.ai GLM");
        assert_eq!(snapshot.get(Provider::Zai).compact_text(), "15%");

        active.0[Provider::Zai as usize] = false;
        assert!(!snapshot.visible_providers(active).has(Provider::Zai));
    }

    #[test]
    fn zai_key_extraction_and_file_parsing() -> anyhow::Result<()> {
        let auth = json!({
            "zai-coding-plan": { "type": "api", "key": "test-key-1" },
            "opencode-go": { "type": "api", "key": "go-key" }
        });
        assert_eq!(zai_key(None, Some(&auth)), Some("test-key-1"));
        assert_eq!(
            zai_key(Some("env-override"), Some(&auth)),
            Some("env-override")
        );
        assert_eq!(zai_key(Some("  "), Some(&auth)), Some("test-key-1"));

        let legacy_auth = json!({
            "zai": { "type": "api", "key": "test-key-2" }
        });
        assert_eq!(zai_key(None, Some(&legacy_auth)), Some("test-key-2"));

        let dummy = json!({
            "zai-coding-plan": { "type": "api", "key": "opencode-oauth-dummy-key" }
        });
        assert_eq!(zai_key(None, Some(&dummy)), None);

        let root = env::temp_dir().join(format!(
            "nosih-zai-auth-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let result = (|| -> anyhow::Result<()> {
            let home = root.join("home");
            let default_path = home.join(".local/share/opencode/auth.json");
            fs::create_dir_all(
                default_path
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("missing default parent"))?,
            )?;
            let original = br#"{"zai-coding-plan":{"type":"api","key":"test-file-key"}}"#;
            fs::write(&default_path, original)?;
            assert_eq!(
                zai_token(None, None, Some(&home)).as_deref(),
                Some("test-file-key")
            );
            Ok(())
        })();
        fs::remove_dir_all(&root)?;
        result
    }
}
