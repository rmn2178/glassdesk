# GLASSDESK — Windows 11 Liquid Glass Dynamic Island

A production-grade, macOS-inspired **Liquid Glass Dynamic Island** desktop overlay for **Windows 11**, engineered with **Rust**, **Tauri 2**, **WinRT SMTC**, **Win32/COM**, **SQLite (`rusqlite`)**, and **TypeScript**.

## Highlights

- **macOS 26-Inspired Liquid Glass & Harmonic Spring Physics**
  - Concentric nested geometry (`24 px` floating panel → `16 px` inner cards → `10 px` nested controls), multi-layer specular rim highlights, energizing hover/click inner glow, and pointer-tracked radial caustics.
  - Frame-rate-independent damped harmonic oscillator spring physics (`60 Hz–240 Hz`) with a self-sleeping `requestAnimationFrame` loop (**0% idle CPU**).
  - Draggable, movable floating macOS-style panels (Settings, Music, FILE HOLD, Tasks, Calendar, Quick Controls) with traffic-light header controls and double-click recenter.
- **Native Windows 11 Live Music Controls (WinRT SMTC)**
  - Reads real **Now Playing** track title, artist, album, source app, and playback state from **Spotify, YouTube (Chrome/Edge), Apple Music, VLC, and Windows Media Player** via `GlobalSystemMediaTransportControlsSessionManager`.
  - Live animated equalizer pill inside the Dynamic Island + expanded Liquid Glass **Now Playing** panel with Previous, Play/Pause, Next, and real master volume control (with Win32 media-key fallback).
- **Two-Way Native FILE HOLD Shelf (Drag IN + Drag OUT)**
  - **Drag IN**: Drop single or multiple files/folders from Windows Explorer onto the top bar (`onDragDropEvent`) with a breathing Liquid Glass drop zone and 48×48 JPEG thumbnails.
  - **Drag OUT**: Grab any held file chip and drag it natively out of GLASSDESK into Windows Explorer, Desktop, Chrome, Discord, or any app via native Windows OLE (`drag` crate).
  - **File Safety**: Stores path references in SQLite (`held_files`) — never moves or deletes originals — with optional independent **Copy to Vault**, missing-file detection, search, categories, and sorting.
- **Real Windows 11 System Status & Productivity Suite**
  - Foreground app tracking (ignoring GLASSDESK itself), keyboard layout (`GetKeyboardLayout`), battery & charging (`GetSystemPowerStatus`), throttled master volume (`IAudioEndpointVolume`), network status (`INetworkListManager`), and direct `ms-settings:` shortcuts.
  - SQLite-backed **Task Manager** (inline edit, deadlines, reminders, priorities, categories) and **Clock & Calendar** with interactive Quick Reminders.
- **Memory & Storage Optimized**
  - Size-optimized Rust release binary (`opt-level = "z"`, LTO, `panic = "abort"`, stripped), bounded 2 MB SQLite cache with incremental auto-vacuum, automatic hidden-panel DOM unloading, Windows working-set trim (`SetProcessWorkingSetSize`), and adaptive idle polling.

## Automated Windows x64 Compilation (GitHub Actions)

Push this repository to GitHub to automatically compile Windows x64 binaries on `windows-latest` via `.github/workflows/build.yml`:

1. Push to `main` (or trigger manually via **Actions → Build GLASSDESK (Windows x64) → Run workflow**).
2. Download the compiled artifacts:
   - `GLASSDESK-Windows-x64-NSIS-Installer` (`.exe` installer)
   - `GLASSDESK-Windows-x64-MSI-Installer` (`.msi` installer)
   - `GLASSDESK-Windows-x64-Portable` (standalone `glassdesk.exe`)
3. Push a version tag (`git tag v1.1.0 && git push --tags`) to publish a GitHub Release automatically.

## Local Windows 11 Development

```powershell
npm install
npm run tauri dev
```

### Build & Test Locally

```powershell
npm run build
cd src-tauri && cargo test && cd ..
npx tauri build --bundles "nsis,msi"
```

## License

Released under the [MIT License](./LICENSE).
