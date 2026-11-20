# NotchGlass — Detailed Engineering Plan & Architecture Blueprint

**Target Product:** Native Windows 10 (v2004+) & Windows 11 Dynamic Island / MacBook-Style Notch Utility (inspired by [NotchPixel](https://notchpixel.com/))  
**Core Stack:** Rust (Tauri 2 + `windows` 0.58 Win32/WinRT + `rusqlite` SQLite + `axum` 0.7 + `sysinfo` 0.32) & React 18 + TypeScript + Vite  
**CI/CD Pipeline:** Automated GitHub Actions (`windows-latest`) compiling portable `.exe`, companion `notch-cli.exe`, NSIS `.exe` installer, and WiX `.msi` installer on every push.

---

## 1. Product Scope & Core Design Philosophy

NotchGlass implements the **top-edge Dynamic Island & MacBook-style Notch** experience on Windows 10/11 without replacing the native Windows taskbar:

1. **Dual Geometry Modes**:
   - **Floating Dynamic Island**: Rounded capsule anchored at `y = 8px` from top edge with soft shadow and Fresnel specular rim.
   - **MacBook-Style Notch**: Flush with `y = 0` top bezel, flat top edge, rounded bottom corners (`16px`–`24px`), and optional horizontal placement (Top-Centre, Top-Left, Top-Right).
2. **Four Adaptive Visual States**:
   - **Resting Pill**: Compact live telemetry (clock, battery, CPU/RAM, mini now-playing visualizer, active mic/cam privacy dot, or auto-hide until hover).
   - **Expanded Interactive Card**: Smooth spring-animated expansion (`580px × 320px`) on pointer hover (configurable 0–1500 ms delay) or global hotkey (`Ctrl+Alt+N`), remembering the last active tab across restarts.
   - **Transient Activity / HUD Pill**: Automatic temporary pill notifications for volume changes, global mic mute, reminders, AI agent nudges, and file drops.
   - **Fullscreen Game Strip**: Detects borderless/windowed fullscreen applications (`GetForegroundWindow` vs `GetMonitorInfoW`) and collapses to a slim non-intrusive hardware strip (CPU, RAM, Network Mbps) with hover-expand disabled so mouse aiming is never interrupted.
3. **Liquid Glass & 11 Themes**:
   - **Liquid Glass** (frosted translucent glass, directional Fresnel highlight, dynamic blur slider, album-art accent bleed), **Candy Glass**, **Acrylic**, **Pure Black (OLED)**, **Black**, **Graphite**, **Rosé**, **Lavender**, **Peach**, **Sakura**, and **Pure White**.
   - **Capture Exclusion**: Uses `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` to hide the island from screenshots and screen recordings when toggled.

---

## 2. System Architecture & Module Breakdown

| Layer | Technology | Responsibility |
| :--- | :--- | :--- |
| **Window & Shell Engine** | Rust + Tauri 2 + Win32 (`windows` 0.58) | Frameless transparent topmost `HWND` (`WS_EX_TOPMOST \| WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE`), dynamic hit-testing, capture exclusion, fullscreen detector, system tray, and global hotkeys (`tauri-plugin-global-shortcut`). |
| **Media & Lyrics Engine** | WinRT SMTC + `reqwest` (`rustls-tls`) | Queries Windows `GlobalSystemMediaTransportControlsSessionManager` for Spotify, Chrome/Edge YouTube, Apple Music, VLC, etc., plus synchronized LRC lyrics via [LRCLIB](https://lrclib.net/). |
| **Hardware & Privacy Telemetry** | `sysinfo` 0.32 + Win32 Registry & Core Audio | Samples CPU %, RAM usage, Network Down/Up Mbps; reads `IMMDeviceEnumerator` / `IAudioEndpointVolume` for volume & global mic mute; inspects Windows `CapabilityAccessManager\ConsentStore` for active mic/webcam apps. |
| **Persistence Layer** | SQLite (`rusqlite` bundled, WAL mode) | Local zero-telemetry storage in `%AppData%\NotchGlass\notchglass.db` for clipboard history, pinned snippets, reminders, to-dos, notification logs, and 6 launcher slots. |
| **Local Scripting API & CLI** | `axum` 0.7 (`127.0.0.1:47823`) + `notch-cli.exe` | Loopback HTTP endpoints (`POST /notify`, `POST /tile`, `GET /health`) allowing Claude Code, Codex, Antigravity, Hermes, or custom scripts to push live status pills and agent alerts. |
| **Send-to-Phone Server** | Ephemeral `axum` server (`0.0.0.0:0`) + `qrcode` | Spins up a random-port, 128-bit random-token LAN download page for staged File Shelf items, rendered as an instant inline SVG QR code; auto-shuts down on close. |
| **PC Deck Remote** | LAN `axum` HTTP + WebSocket + Win32 `SendInput` | Mobile browser remote controller (Trackpad, Keyboard, Media, Slides, Movies, PC Stats) protected by a 2-minute expiring QR pairing token and explicit PC-side Allow/Deny confirmation. |
| **UI & Motion Engine** | React 18 + TypeScript + Vite + CSS Glass | Spring-physics transitions, 11 themes, 30 fps canvas audio visualizer (pixel/rounded bars), 11 modular feature tabs, and crisp `Segoe UI Variable` / `Cascadia Mono` (`tabular-nums`) typography. |

---

## 3. Repository Directory Structure

```text
notchglass/
├── .github/
│   └── workflows/
│       └── build-windows.yml        # Automated CI/CD: builds .exe, notch-cli.exe, NSIS, MSI + Releases
├── .gitignore
├── PLAN.md                          # This detailed architectural & implementation plan
├── README.md                        # Setup, build, shortcuts, CLI, and GitHub Actions guide
├── index.html                       # Transparent frameless root HTML
├── package.json                     # React 18, Vite 5, TypeScript 5, Tauri 2 API/CLI
├── tsconfig.json
├── vite.config.ts
├── src/
│   ├── main.tsx
│   ├── App.tsx                      # Root island controller: states, hover delay, HUDs, tabs
│   ├── types.ts                     # Shared TypeScript interfaces matching Rust serde structs
│   ├── styles/
│   │   └── notch.css                # Liquid Glass surfaces, 11 themes, spring keyframes, layout
│   └── components/
│       ├── RestingPill.tsx          # Compact resting pill + Game Strip + privacy dots
│       ├── MusicTab.tsx             # SMTC controls, 30fps visualizer, LRCLIB synced lyrics
│       ├── ShelfTab.tsx             # 12-item drag-and-drop File Shelf + QR Send-to-Phone
│       ├── ClipboardTab.tsx         # Categorized SQLite clipboard history + pin/search/copy
│       ├── WidgetsTab.tsx           # Clock, World Clock, Weather, To-Do, Pomodoro, Calc, Stats
│       ├── RemindersTab.tsx         # Repeating reminders + live alert snooze/done
│       ├── ControlsTab.tsx          # Volume, Global Mic Mute, Color Picker, Lock PC, Privacy
│       ├── LauncherTab.tsx          # 6 quick-launch app slots (Ctrl+Alt+1..6)
│       ├── AgentsTab.tsx            # AI coding agent monitor (Claude Code, Codex, Hermes, etc.)
│       ├── DeckTab.tsx              # PC Deck QR pairing, Allow/Deny approval, session status
│       ├── NotificationsTab.tsx     # Local notification feed + privacy sensor log
│       └── SettingsTab.tsx          # Shape, theme, blur, open delay, capture exclusion, hotkeys
└── src-tauri/
    ├── Cargo.toml                   # Workspace/crate dependencies + notch-cli binary target
    ├── build.rs                     # Tauri build script
    ├── tauri.conf.json              # Tauri 2 window, bundle (NSIS + MSI), and icon config
    ├── capabilities/
    │   └── default.json             # Tauri 2 security capability grants
    ├── icons/                       # Multi-res RGBA PNGs + Windows .ico
    └── src/
        ├── main.rs                  # Windows GUI entry point
        ├── lib.rs                   # Tauri setup, tray icon, hotkeys, all IPC commands
        ├── config.rs                # JSON config persistence & defaults (%AppData%\NotchGlass)
        ├── db.rs                    # SQLite schema & CRUD (clipboard, reminders, todos, notes, launcher)
        ├── win32.rs                 # Win32 HWND flags, capture exclusion, fullscreen, volume/mic, pixel picker, SendInput, privacy registry
        ├── media.rs                 # WinRT SMTC media controller + LRCLIB lyrics fetcher
        ├── clipboard.rs             # Background clipboard watcher with SHA-256 deduplication
        ├── hardware.rs              # sysinfo CPU/RAM/Network telemetry sampler
        ├── api_server.rs            # Local loopback API (:47823) for /notify, /tile, /health
        ├── phone_transfer.rs        # Ephemeral LAN Send-to-Phone HTTP server + SVG QR generator
        ├── pc_deck.rs               # LAN PC Deck WebSocket remote + 2-min QR pairing + mobile web UI
        └── bin/
            └── notch_cli.rs         # Companion CLI binary (notch-cli.exe) for scripts & AI agents
```

---

## 4. Database Schema (SQLite WAL Mode)

Stored locally at `%AppData%\NotchGlass\notchglass.db`:

- `clipboard_items (id, kind, content, content_hash UNIQUE, is_pinned, created_at)` — stores `text`, `link`, and `file` entries with SHA-256 deduplication and retention pruning for unpinned items.
- `reminders (id, title, due_at, repeat_rule, is_done, created_at)` — supports `once`, `daily`, `weekdays`, and `weekly` schedules.
- `todos (id, text, completed, created_at)` — backs the Widgets tab To-Do list.
- `notifications (id, app_name, title, body, accent, received_at)` — local notification and agent alert history.
- `launcher_slots (slot_index PRIMARY KEY, label, target_path, icon_hint)` — 6 customizable quick-launch slots mapped to `Ctrl+Alt+1..6`.

---

## 5. Default Global Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + Alt + N` | Toggle Island Expand / Collapse |
| `Ctrl + Alt + H` | Toggle Island Visibility (Hide / Show) |
| `Ctrl + Alt + P` | Media Play / Pause (SMTC) |
| `Ctrl + Alt + M` | Global Microphone Mute / Unmute |
| `Ctrl + Alt + C` | Instant Screen Colour Picker (`#HEX`) |
| `Ctrl + Alt + 1` .. `6` | Launch Quick-Launcher Slots 1–6 |

---

## 6. Local Loopback API & Companion CLI (`127.0.0.1:47823`)

- `POST http://127.0.0.1:47823/notify`
  ```json
  {
    "agent": "claude-code",
    "status": "needs_input",
    "title": "Permission Required",
    "body": "Approve running cargo test in workspace?",
    "accent": "#F59E0B"
  }
  ```
- `POST http://127.0.0.1:47823/tile`
  ```json
  {
    "id": "build-1",
    "label": "CI Build",
    "value": "74%",
    "progress": 0.74
  }
  ```
- **Companion CLI (`notch-cli.exe`)**:
  ```powershell
  notch-cli notify --agent claude-code --status needs_input --title "Approve Edit" --body "Ready for review"
  notch-cli tile --id deploy --label "Deploy" --value "Done" --progress 1.0
  ```

---

## 7. Security & Privacy Model

1. **Zero Telemetry & Local-First**: No user account, no analytics, and no cloud sync. Clipboard, reminders, to-dos, and notifications remain inside `%AppData%\NotchGlass\`.
2. **Opt-In Network Calls Only**: External HTTP requests occur strictly when the user opens Synced Lyrics ([LRCLIB](https://lrclib.net/)) or Weather ([Open-Meteo](https://open-meteo.com/)).
3. **Ephemeral File Transfer**: Send-to-Phone binds a random OS port with a 128-bit random URL token and terminates immediately when closed.
4. **PC Deck Authorization**: Disabled by default; pairing QR codes expire after 120 seconds and require explicit **Allow** confirmation inside the PC island before any mouse/keyboard input is accepted.

---

## 8. GitHub Actions CI/CD Pipeline (`.github/workflows/build-windows.yml`)

Triggered automatically on every `push` to `main`/`master`, every `v*` tag, pull requests, and manual `workflow_dispatch`:

1. Provisions `windows-latest` runner with Node.js 20 and Rust stable (`dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache@v2`).
2. Installs frontend dependencies (`npm install`) and verifies strict TypeScript compilation (`npx tsc --noEmit`) + Vite production bundle (`npm run build`).
3. Builds the companion CLI (`cargo build --release --bin notch-cli`) and full Tauri 2 Windows installers (`npx tauri build --bundles nsis,msi`).
4. Uploads downloadable build artifacts (`notchglass.exe`, `notch-cli.exe`, NSIS setup `.exe`, WiX `.msi`) directly to the workflow run — and automatically publishes a GitHub Release when pushing a version tag (e.g., `git tag v0.1.0 && git push --tags`).
