# NotchGlass

A native **Windows 10 (v2004+) & Windows 11 Dynamic Island / MacBook-Style Notch** productivity hub inspired by [NotchPixel](https://notchpixel.com/), built with **Rust (Tauri 2 + Win32/WinRT + SQLite + Axum)** and **React 18 + TypeScript + Vite**.

## Key Features

- **Dual Geometry & 11 Themes**: Switch live between **Floating Dynamic Island** and **MacBook-style Notch** across 11 themes (`Liquid Glass`, `Pure Black`, `Black`, `Graphite`, `Acrylic`, `Candy Glass`, `Rosé`, `Lavender`, `Peach`, `Sakura`, `Pure White`) with adjustable glass blur, corner radius, and hover open delay.
- **Music & Synced Lyrics**: Native Windows SMTC media controls (Spotify, YouTube in browsers, Apple Music, VLC, etc.), 30 fps canvas visualizer (rounded or pixel bars), and live synced lyrics via [LRCLIB](https://lrclib.net/).
- **File Shelf & Send to Phone**: Drag-and-drop staging shelf (up to 12 files) + ephemeral LAN HTTP server with 128-bit random token and instant QR code download (zero phone app required).
- **Clipboard History**: Persistent SQLite clipboard watcher (`All`, `Text`, `Links`, `Files`) with search, pinning, one-click re-copy, and configurable retention.
- **Widgets & Reminders**: Live Weather ([Open-Meteo](https://open-meteo.com/)), Pomodoro Focus Timer, Day Progress, PC Telemetry, Quick Calculator, SQLite To-Do list, and repeating Reminders with Snooze/Done.
- **Quick Controls, Privacy Sensors & Game Strip**: Windows master volume slider, Global Mic Mute (`Ctrl+Alt+M`), Screen Color Picker (`Ctrl+Alt+C`), Lock PC, active Mic/Camera privacy indicator dots (Windows `CapabilityAccessManager`), and automatic Fullscreen Game Strip.
- **AI Agent Monitor & Local API (`127.0.0.1:47823`)**: Track Claude Code, Codex, Antigravity, Hermes, or custom agents via `POST /notify`, `POST /tile`, and the bundled `notch-cli.exe` binary.
- **PC Deck (Smartphone Browser Remote)**: Local Wi-Fi WebSocket remote (Trackpad, Keyboard, Media, Slides, Movies) with 2-minute expiring QR pairing and PC-side Allow/Deny confirmation.

## Push to GitHub & Build Automatically (GitHub Actions)

This repository includes `.github/workflows/build-windows.yml`, which automatically compiles Windows x64 binaries on every push to `main`/`master` or tag push (`v*`):

```bash
git init
git add .
git commit -m "Initial NotchGlass release"
git branch -M main
git remote add origin https://github.com/<your-username>/notchglass.git
git push -u origin main
```

1. Open your GitHub repository → **Actions** tab → **Build NotchGlass (Windows x64)**.
2. Download the **`NotchGlass-Windows-x64`** artifact containing:
   - `NotchGlass_0.1.0_x64-setup.exe` (NSIS installer)
   - `NotchGlass_0.1.0_x64_en-US.msi` (WiX MSI installer)
   - `NotchGlass-portable-x64.exe` (standalone portable binary)
   - `notch-cli-x64.exe` (companion CLI for AI agent hooks)
3. To publish a GitHub Release automatically:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

## Local Development (Windows)

Requirements: [Node.js 20+](https://nodejs.org/), [Rust stable](https://rustup.rs/), and Microsoft Visual Studio C++ Build Tools.

```powershell
npm install
npm run tauri dev
```

## Companion CLI Usage (`notch-cli.exe`)

```powershell
notch-cli notify --agent claude-code --status needs_input --title "Approve Edit" --body "Ready to apply diff"
notch-cli tile --id build --label "Cargo Build" --value "82%" --progress 0.82
```

## Default Global Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + Alt + N` | Expand / Collapse Island |
| `Ctrl + Alt + H` | Show / Hide Island |
| `Ctrl + Alt + P` | Media Play / Pause |
| `Ctrl + Alt + M` | Global Microphone Mute |
| `Ctrl + Alt + C` | Instant Cursor Color Picker (`#HEX`) |

See [`PLAN.md`](./PLAN.md) for the full architectural blueprint and module specification.
