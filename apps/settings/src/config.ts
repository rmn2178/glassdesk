import { invoke } from "@tauri-apps/api/core";

export type Language = "auto" | "es" | "en" | "fr" | "zh" | "hi" | "ar" | "bn" | "pt";
export type IslandStyle = "notch" | "bubble";
export type GenieSpeed = "normal" | "fast" | "faster";
export type MascotCharacter = "crab" | "cat" | "fox" | "capybara" | "mochi" | "girl";
export type DockPosition = "left" | "bottom" | "right";
export type DockDensity = "compact" | "normal" | "spacious";
export type DockAutoHideSpeed = "smooth" | "normal" | "fast";
/** View of the existing two settings; saved atomically as a mutually exclusive pair. */
export type DockHidingMode = "visible" | "automatic" | "intelligent";
/** `auto` follows Windows' light or dark mode. */
export type Appearance = "auto" | "light" | "dark" | "frosted" | "clear" | "liquidGlass";

/** Mirrors `nosih_config::Config`. */
export interface Config {
  version: number;
  pinned: string[];
  recent: string[];
  autoHide: boolean;
  smartHide: boolean;
  iconSize: number;
  magnification: boolean;
  magnifiedSize: number;
  dockPosition: DockPosition;
  dockDensity: DockDensity;
  dockAutoHideSpeed: DockAutoHideSpeed;
  showStart: boolean;
  showTrash: boolean;
  showDownloads: boolean;
  pinExplorer: boolean;
  genie: boolean;
  genieSpeed: GenieSpeed;
  showMinimized: boolean;
  showOtherWorkspaces: boolean;
  jumpLists: boolean;
  menuBar: boolean;
  menuBarAutoHide: boolean;
  menuBarSmartHide?: boolean;
  aiUsage: boolean;
  menuBarMusic: boolean;
  menuBarIslandStyle: IslandStyle;
  /** A bell at the right end of the menu bar that opens the notification center. */
  menuBarNotifications: boolean;
  /** Use Nosih's native macOS-style Control Center instead of Windows Quick Settings. */
  menuBarControlCenter?: boolean;
  /** Use Nosih's native macOS-style Notification Center and pop-up banners. */
  menuBarNativeNotifications?: boolean;
  /** Global macOS-style Spotlight Search launcher. */
  spotlightSearch?: boolean;
  /** Native macOS-style HUD overlay for volume and brightness changes. */
  nativeHud?: boolean;
  /** Active application menus in the menu bar. */
  menuBarAppMenus?: boolean;
  /** Subtle rounded corners at the top of the screen. */
  roundedScreenCorners?: boolean;
  /** What the clock opens (an app, shortcut or URI); null for the notification center. */
  menuBarClockTarget: string | null;
  accessoryBattery: boolean;
  menuBarSystemMonitor?: boolean;
  showIndicators: boolean;
  showRecents: boolean;
  bounceOnOpen: boolean;
  mascot: boolean;
  mascotCharacter?: MascotCharacter;
  mascotBreakReminder?: boolean;
  checkUpdates: boolean;
  autoUpdate: boolean;
  language: Language;
  appearance: Appearance;
  /** The menu bar's own theme; null (or missing) gives it the dock's. */
  menuBarAppearance: Appearance | null;
  /** The folder shown at the right of the dock, whose contents open as a menu. */
  shortcutsFolder: string | null;
}

export const ICON_SIZE = { min: 32, max: 96 } as const;
export const MAX_MAGNIFIED_SIZE = 160;

export const loadConfig = () => invoke<Config>("get_config");
export const defaultConfig = () => invoke<Config>("default_config");
export const saveConfig = (patch: Partial<Config>) => invoke<Config>("set_config", { patch });
export const getAutostart = () => invoke<boolean>("get_autostart");
/** Current Windows values for display only; never merge these into Config. */
export interface WindowsPreferences {
  animations: boolean;
}

let windowsPreferencesRequest: Promise<WindowsPreferences> | undefined;
/** Coalesce overlapping focus/StrictMode reads, but reread after returning from Windows Settings. */
export const loadWindowsPreferences = () =>
  windowsPreferencesRequest ??= invoke<WindowsPreferences>("windows_preferences")
    .finally(() => { windowsPreferencesRequest = undefined; });
export const openAnimationSettings = () => invoke<void>("open_animation_settings");
export const setAutostart = (enabled: boolean) => invoke<void>("set_autostart", { enabled });
/** The folder the user picks, or null if they cancel. */
export const pickFolder = (current: string | null) =>
  invoke<string | null>("pick_folder", { current });
/** An app or shortcut for the clock to open, or null if the user cancels. */
export const pickClockTarget = (current: string | null, apps: string, all: string) =>
  invoke<string | null>("pick_clock_target", { current, apps, all });
export interface UpdateInfo {
  version: string;
  /** Release page with the notes, if the manifest has one. */
  notes: string | null;
}

/** Resolves to the newer version available, or null when up to date. */
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");
export const openReleaseNotes = (url: string) => invoke<void>("open_release_notes", { url });
/** Fixed project URL, opened only when the user asks. */
export const openRepository = () => invoke<void>("open_repository");
let wallpaperRequest: Promise<string | null> | undefined;
/** One bounded native read per Settings process, including React StrictMode. */
export const loadWallpaperPreview = () =>
  wallpaperRequest ??= invoke<string | null>("wallpaper_preview").catch(() => null);
let wallpaperSelectionRequest: Promise<number | null> | undefined;
/** Share overlapping reads from StrictMode or quick section navigation. */
export const loadWallpaperSelection = () => {
  if (wallpaperSelectionRequest) return wallpaperSelectionRequest;
  const request = invoke<number | null>("wallpaper_selection").finally(() => {
    if (wallpaperSelectionRequest === request) wallpaperSelectionRequest = undefined;
  });
  wallpaperSelectionRequest = request;
  return request;
};
export const applyWallpaper = async (id: number, preview: string) => {
  await invoke<void>("apply_wallpaper", { id });
  // Original images stay in native storage; Settings uses only the small preview.
  wallpaperRequest = Promise.resolve(preview);
  wallpaperSelectionRequest = undefined;
};
export const installUpdate = () => invoke<void>("install_update");
