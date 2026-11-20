# GLASSDESK — Windows 11 Dynamic Island Desktop Overlay

A slim, top-edge floating Liquid Glass desktop overlay for Windows 11 built with **Rust**, **Tauri 2**, **SQLite (`rusqlite`)**, **TypeScript**, and native **Win32 / COM APIs**.

## Core Features

- **Native Top-Edge Floating Overlay**: 34 px collapsed bar attached flush to the top screen edge (`WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`), transparent, borderless, hidden from the taskbar and Alt-Tab, and dynamically resized only when a panel expands so the desktop below stays 100% usable.
- **Dynamic Island Capsule**: Interactive center glass pill with spring hover bloom, live clock, held file count, pending task count, and smooth click-to-expand attached Liquid Glass panels (dismissible via outside click or `Escape`).
- **FILE HOLD Shelf**: Drag single or multiple files/folders directly from Windows Explorer onto the top bar. Persists path references in SQLite (`held_files`), generates 64×64 thumbnails for images, detects missing files, supports **Open**, **Reveal in Explorer**, **Copy Path**, optional **Copy to Vault** (never moves or deletes originals), category filters, search, and sorting.
- **Real Windows 11 System Status & Quick Controls**: Live foreground app name, keyboard layout (`ENG`, etc.), real battery percentage & charging state (`GetSystemPowerStatus`), real master volume slider & mute (`IAudioEndpointVolume`), live network status (`INetworkListManager`), and direct `ms-settings:` shortcuts.
- **Clock, Calendar & Task Manager**: Lightweight clock ticker (updates only text nodes, zero full re-renders), 12h/24h formats, monthly calendar with deadline dots, and SQLite-backed tasks across `College`, `Personal`, `Projects`, and `Other` categories.
- **Auto-Hide & Multi-Monitor Ready**: Optional full-screen app/game auto-hide (`SHQueryUserNotificationState`), customizable global shortcut (`Alt+Space` default), Dark/Light/Automatic themes, accent swatches, and monitor selection.

## Automatic Windows Compilation (GitHub Actions)

Push this repository to GitHub to automatically compile Windows x64 binaries on `windows-latest` via `.github/workflows/build.yml`:

1. Create a new GitHub repository and push all files to `main`.
2. Open the **Actions** tab and run (or wait for) **Build GLASSDESK (Windows x64)**.
3. Download the generated artifacts:
   - `GLASSDESK-Windows-x64-NSIS-Installer` (`.exe` installer)
   - `GLASSDESK-Windows-x64-MSI-Installer` (`.msi` installer)
   - `GLASSDESK-Windows-x64-Portable` (standalone `glassdesk.exe`)
4. Tag a release (e.g., `git tag v1.0.0 && git push --tags`) to automatically publish a GitHub Release with all installers attached.

## Local Development on Windows 11

### Prerequisites
- [Rust (MSVC toolchain)](https://rustup.rs/) 1.77.2+
- [Node.js](https://nodejs.org/) 20 LTS
- Microsoft Edge WebView2 Runtime (pre-installed on Windows 11)
- Visual Studio C++ Build Tools (Desktop development with C++)

### Run Locally
```powershell
npm install
npm run tauri dev
```

### Run Tests & Build Installers Locally
```powershell
npm run typecheck
npm run build
cd src-tauri && cargo test && cd ..
npm run tauri build -- --bundles nsis,msi
```
