<div align="center">
  
<img width="50%" alt="Frame 1116606779" src="https://github.com/user-attachments/assets/8c715a53-a0df-4780-9dd4-09ce3ccd9043" />

### A lightweight, macOS-inspired dock for Windows 10 and 11

  Nosih replaces the Windows taskbar with a customizable dock and an optional
  macOS-style menu bar.

</div>

<img width="1500" height="436" alt="Frame 1116606780" src="https://github.com/user-attachments/assets/3c91b488-214d-453d-a8c2-a9e0dc7f245e" />

<img width="1000" height="259" alt="ezgif-6345057d171e16ef" src="https://github.com/user-attachments/assets/e5b2ce63-cec1-4d1d-8ef1-a4fa1c668c40" />

| Clawd Pet | Genie Effect on Minimize |
| :---: | :---: |
| ![GIF showing mascot the Dock and Menu Bar](https://github.com/user-attachments/assets/580b235e-6141-493a-a333-8f2b96f3acca) | ![GIF showing a window minimizing into the Dock with the Genie effect](https://github.com/user-attachments/assets/878503a8-7286-445d-826b-41c18da013bc) |
| **Usage preview** | **Usage detail** |
| ![GIF showing the Dock and Menu Bar automatically hiding and reappearing](https://github.com/user-attachments/assets/60a8f7c0-20fc-429b-977f-d5b4b7d97d88) | ![GIF showing a window minimizing into the Dock with the Genie effect](https://github.com/user-attachments/assets/fbf21cf6-c1c0-4173-9d68-ae883f0d2a45) |

## Features

- Glass-style dock with smooth, cursor-based icon magnification.
- Choose the dock's screen edge in Settings → Dock → *Position on screen*:
  left, bottom, or right. Bottom is the default.
- Launch, pin, reorder, and manage apps, including recent apps and open windows.
- Group pinned apps into a single dock icon: drop one app onto another, click
  the group to fan its apps out, and drag one out of the fan to take it back.
- Hover over an app with multiple windows to show live, titled previews;
  click a preview to switch to that window. Scroll over the previews for more
  windows, or press Escape to dismiss it.
- Minimized windows fold into their app's icon with the Genie effect; turn on
  Settings → Windows → *Show minimized windows* to keep them as live thumbnails
  on the right side of the dock instead.
- Click the app already in front to minimize it, as on the Windows taskbar.
- Right-click an app to see its jump list, with icons, as on the Windows
  taskbar: pinned and recent items, the app's own categories and its tasks.
- Drag and drop files onto apps, Downloads, or the Recycle Bin.
- A shortcuts folder at the right of the dock: its contents open as a menu,
  with a submenu for each folder inside, like a Windows 10 taskbar toolbar.
- Dock and menu bar visibility: always visible, automatic, or intelligent
  (hidden only while the active window reaches under them).
- Appearance: follow Windows, or pick Light, Dark, Frosted glass, Clear or
  Aurora; plus an offline wallpaper gallery.
- An optional system monitor in the menu bar: CPU, memory and network speed.
- A pixel-art mascot for moments worth it: AI plans, the battery, the network,
  the recycle bin and a daily hello. Pick Claude's crab, a cat, a fox, a
  capybara, a mochi or a girl; an optional break reminder is off by default.
- Optional battery levels for compatible connected mice and headsets. Enable
  Settings → Menu bar → *Show accessory battery*; it is off by default.
- A top menu bar with the app in front, clock, network, volume and battery, the
  input language, microphone/camera in use, the hidden tray icons, plus
  the Search and Task View buttons if enabled on the Windows taskbar. The
  Widgets button is off by default and can be enabled in Settings.
- A curved island in the menu bar for usage limits from Claude Code, Codex,
  OpenCode Go, Z.ai GLM Coding Plan, Gemini CLI, and Antigravity. Z.ai shows
  its 5-hour and weekly limits; Gemini Pro and Flash quotas appear separately
  when available.
- Separate Settings app with live changes and a configurable Genie animation.
  Eight languages: English, Spanish, French, Simplified Chinese, Hindi, Arabic,
  Bengali and Portuguese. Pick a language or follow Windows automatically.

The Genie effect is **on by default** and can be turned off in Settings → Windows.
While enabled, Nosih temporarily disables Windows window transitions for
tracked windows so its minimize animation can run; those transitions are
restored when Nosih exits.

## Install

**[Download Nosih-Setup.exe](https://github.com/crafter-station/Nosih/releases/latest/download/Nosih-Setup.exe)**
(Windows 10 version 1903 or later, or Windows 11; 64-bit) or pick a version from
[Releases](https://github.com/crafter-station/Nosih/releases).

1. Run `Nosih-Setup.exe`. It installs for your user only: no administrator
   rights, no wizard, about two seconds.
2. Nosih is new, so Windows SmartScreen may not recognize it yet and say it
   protected your PC. Choose **More info → Run anyway**. See the
   [code-signing policy](https://nosih.crafter.run/code-signing-policy).
3. Open **Nosih** from the Start menu, then choose **Dock** to
   personalize the dock and top menu bar.

## Try it on Windows

### Prerequisites

- Windows 10 (version 1903 or later) or Windows 11. On Windows 10 the dock and
  menu bar are solid instead of glass: Windows 10 has no backdrop blur for
  desktop apps.
- Rust with the MSVC toolchain and Visual Studio C++ Build Tools
- Node.js and pnpm

### Run the dock

From the repository root:

```powershell
cargo run -p nosih
```

The dock hides the taskbar while it is running. Press **Ctrl+C** in the
terminal to exit; Nosih should restore the taskbar automatically. If you need
to restore it manually:

```powershell
cargo run -p nosih -- --restore
```

### Run Settings

Keep the dock running, open a second terminal, and run:

```powershell
cd apps\settings
pnpm install
pnpm tauri dev
```

This opens the native Settings window. Frontend changes hot-reload; after
changing the Rust backend, restart `tauri dev`. Settings changes are applied to
the running dock.

## Build and test

From the repository root:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
```

To build the Settings frontend:

```powershell
cd apps\settings
pnpm build
```

## Project layout

| Path | Purpose |
|---|---|
| `apps/dock` | Native Windows dock |
| `apps/settings` | Tauri desktop settings app |
| `crates/nosih-core` | Platform-independent layout and animation logic |
| `crates/nosih-shell` | Windows shell integration |
| `crates/nosih-config` | Shared settings and persistence |
| `nosih` | Astro website |

## Privacy

Nosih collects no data, has no telemetry and never sends anything to the
project. It makes two kinds of network requests, both optional:

- **Updates:** Settings asks GitHub Releases for a newer version when it
  opens, and once a day, a couple of minutes after you turn on the computer,
  Nosih does the same in the background and installs what it finds (the dock
  itself never uses the network). Turn off the installing in Settings → System
  → *Install updates automatically*, or everything with *Check for updates
  automatically*.
- **AI provider usage:** while a supported client is running, Nosih reads its
  existing local sign-in and asks that provider for available plan limits
  every five minutes while visible, or when you press Refresh. For Gemini CLI
  and Antigravity, Gemini Pro and Flash quotas are shown separately when
  available; Antigravity is queried through its local service. Credentials stay
  in memory and are never stored by Nosih. For Z.ai Coding Plan, Nosih reads
  the key from OpenCode's local `auth.json` or `ZAI_API_KEY` /
  `ZHIPU_API_KEY`, then sends it to Z.ai's quota service. Turn usage queries
  off in Settings → Menu bar → *Show AI provider usage*.

Details: [nosih.crafter.run/privacy](https://nosih.crafter.run/privacy).
