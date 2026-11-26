import type { ImageMetadata } from "astro";
import downloads from "@/assets/icons/downloads.webp";
import fileExplorer from "@/assets/icons/file-explorer.webp";
import microsoftPhotos from "@/assets/icons/microsoft-photos.webp";
import microsoftStore from "@/assets/icons/microsoft-store.webp";
import microsoftTeams from "@/assets/icons/microsoft-teams.webp";
import recycleBin from "@/assets/icons/recycle-bin.webp";
import settings from "@/assets/icons/settings.webp";
import windowsMediaPlayer from "@/assets/icons/windows-media-player.webp";
import windowsTerminal from "@/assets/icons/windows-terminal.webp";
import { figures } from "@/data/stats";
import { site } from "@/data/site";
export { settingsText, settingsLanguages, type SettingsKey } from "@/data/settings-languages";

/* ---------------------------------------------------------------- Anatomy */

/** A tile of the annotated dock: an app icon, or one of the two drawn tiles. */
export type AnatomyTile =
  | { kind: "app"; icon: ImageMetadata; label: string; open?: boolean }
  | { kind: "start" }
  | { kind: "window" };

/** One labelled group of the dock; labels alternate above and below the glass. */
export interface AnatomyGroup {
  label: string;
  detail: string;
  side: "top" | "bottom";
  tiles: AnatomyTile[];
  /** A thin divider is drawn before this group, as in the real dock. */
  divider?: boolean;
}

export const anatomyIntro = {
  eyebrow: "The Dock",
  title: "Everything in its place.",
  lead: "Your apps, files and open windows in one glass shelf. Place it along the bottom or either side, and keep only the shortcuts you need.",
} as const;

export const anatomy: AnatomyGroup[] = [
  {
    label: "Start",
    detail: "The Windows Start menu. Optional",
    side: "top",
    tiles: [{ kind: "start" }],
  },
  {
    label: "Pinned apps",
    detail: "Drag to reorder. A dot marks the open ones",
    side: "bottom",
    tiles: [
      { kind: "app", icon: fileExplorer, label: "File Explorer", open: true },
      { kind: "app", icon: microsoftStore, label: "Microsoft Store" },
      { kind: "app", icon: windowsTerminal, label: "Terminal", open: true },
      { kind: "app", icon: settings, label: "Settings" },
    ],
  },
  {
    label: "Recent apps",
    detail: "The last three you used",
    side: "top",
    tiles: [
      { kind: "app", icon: microsoftPhotos, label: "Photos", open: true },
      { kind: "app", icon: windowsMediaPlayer, label: "Media Player" },
      { kind: "app", icon: microsoftTeams, label: "Teams" },
    ],
  },
  {
    label: "Downloads",
    detail: "Drop files in. Right-click to hide",
    side: "bottom",
    divider: true,
    tiles: [{ kind: "app", icon: downloads, label: "Downloads" }],
  },
  {
    label: "Minimized",
    detail: "Optional live thumbnails of minimized windows",
    side: "top",
    tiles: [{ kind: "window" }],
  },
  {
    label: "Recycle Bin",
    detail: "Drop files to recycle. Optional",
    side: "bottom",
    tiles: [{ kind: "app", icon: recycleBin, label: "Recycle Bin" }],
  },
];

/* -------------------------------------------------------------- Customize */

export const customizeIntro = {
  eyebrow: "Settings",
  title: "Make it yours.",
  lead: "Adjust your Dock’s size, spacing and animation speed. Available in 8 languages; try them in the preview.",
} as const;

/** The preview dock in the Customize scene. */
export const previewDockItems: {
  icon: ImageMetadata;
  fixed?: "pinExplorer" | "showDownloads" | "showTrash";
}[] = [
  { icon: fileExplorer, fixed: "pinExplorer" },
  { icon: microsoftStore },
  { icon: windowsTerminal },
  { icon: microsoftPhotos },
  { icon: settings },
  { icon: downloads, fixed: "showDownloads" },
  { icon: recycleBin, fixed: "showTrash" },
];

/** Shared decorative docks keep their existing icon list. */
export const previewIcons: ImageMetadata[] = previewDockItems.map(({ icon }) => icon);

/** Same defaults as the app (`crates/nosih-config`). */
export const settingsDefaults = {
  iconSize: 48,
  iconSizeMin: 32,
  iconSizeMax: 96,
  magnification: true,
  magnifiedSize: 77,
  magnifiedSizeMax: 160,
  autoHide: false,
  dockDensity: "normal",
  dockAutoHideSpeed: "normal",
  showStart: true,
  showDownloads: true,
  showTrash: true,
  pinExplorer: true,
  menuBar: true,
  menuBarAutoHide: false,
  aiUsage: true,
  accessoryBattery: false,
  genie: true,
  showMinimized: false,
  autostart: true,
  mascot: true,
  autoUpdate: true,
} as const;

/* ----------------------------------------------------------------- Safety */

export const safetyIntro = {
  eyebrow: "Reversible",
  title: "Your taskbar is never gone. Just tucked away.",
  lead: "Nosih hides the Windows taskbar while it runs and gives it back the moment it stops. Flip the switch and see.",
} as const;

/** The taskbar shown when Nosih is off. */
export const taskbarIcons: ImageMetadata[] = [
  fileExplorer,
  microsoftStore,
  windowsTerminal,
  microsoftPhotos,
  settings,
];

export const safetyFacts = [
  {
    title: "Quit any time",
    body: "Right-click the dock and choose Quit Nosih. The taskbar is back at once.",
  },
  {
    title: "Even if it stops",
    body: "A tiny guardian process watches the dock and restores the taskbar if it ever stops.",
  },
  {
    title: "Uninstall cleanly",
    body: "Windows Settings → Apps → Nosih. Windows is left exactly as it was.",
  },
] as const;

/* --------------------------------------------------------------- Download */

export const download = {
  name: site.name,
  tagline: site.tagline,
  meta: [
    `Version ${site.version}`,
    "Windows 10 & 11 · 64-bit",
    `${figures.installer.value} ${figures.installer.unit}`,
    `Free · ${site.license}`,
  ],
  cta: "Download for Windows",
  releases: "All versions on GitHub",
  steps: [
    { title: "Open the installer", body: "Per user. No admin rights, no wizard." },
    {
      title: "If SmartScreen asks",
      body: "Nosih is new, so Windows may not know it yet: More info → Run anyway.",
    },
    { title: "That's it", body: "The dock appears and starts with Windows from now on." },
  ],
  source: {
    summary: "Build it from source",
    note: "Needs Windows 10 (1903+) or 11, Rust (MSVC) and the Visual Studio C++ build tools.",
    commands: [`git clone ${site.repository}`, "cd Nosih", "cargo run --release -p nosih"],
  },
} as const;
