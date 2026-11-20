export type PanelName =
  | "hub"
  | "files"
  | "calendar"
  | "tasks"
  | "controls"
  | "notifications"
  | "music"
  | "settings";

export type SettingsCategory =
  | "general"
  | "appearance"
  | "island"
  | "controls"
  | "tasks"
  | "files"
  | "notifications"
  | "shortcuts"
  | "performance"
  | "about";

export type FileCategory = "all" | "documents" | "images" | "videos" | "other";
export type TaskCategory = "College" | "Personal" | "Projects" | "Other";
export type TaskFilter = "today" | "upcoming" | "overdue" | "completed";
export type ThemeMode = "dark" | "light" | "auto";
export type AccentName = "silver" | "blue" | "emerald" | "amber" | "rose" | "violet";

export interface HeldFile {
  id: string;
  originalPath: string;
  vaultPath: string | null;
  displayName: string;
  extension: string;
  category: string;
  isDir: boolean;
  sizeBytes: number;
  isVaultCopy: boolean;
  thumbnailB64: string | null;
  pinned: boolean;
  available: boolean;
  createdAt: string;
}

export interface HoldFilesBatchResult {
  added: HeldFile[];
  skippedDuplicates: string[];
  failed: string[];
}

export interface TaskItem {
  id: string;
  title: string;
  notes: string | null;
  category: TaskCategory;
  completed: boolean;
  priority: number; // 0=Low, 1=Medium, 2=High
  dueAt: string | null;
  reminderAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ReminderItem {
  id: string;
  title: string;
  remindAt: string;
  dismissed: boolean;
  repeatRule: string | null;
  createdAt: string;
}

export interface NotificationItem {
  id: string;
  title: string;
  message: string;
  category: string;
  read: boolean;
  createdAt: string;
}

export interface ActiveWindowInfo {
  title: string;
  executable: string;
  processId: number;
  isSelf: boolean;
}

export interface BatteryStatus {
  percent: number | null;
  isCharging: boolean;
  acConnected: boolean;
  hasBattery: boolean;
}

export interface AudioStatus {
  volume: number;
  volumePercent: number;
  muted: boolean;
  outputDevice: string;
}

export interface NetworkStatus {
  connected: boolean;
  internet: boolean;
  networkName: string | null;
}

export interface MonitorDescriptor {
  index: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
  isPrimary: boolean;
}

export interface MediaSessionInfo {
  hasSession: boolean;
  title: string;
  artist: string;
  album: string;
  sourceApp: string;
  isPlaying: boolean;
}

export interface SystemSnapshot {
  activeWindow: ActiveWindowInfo;
  battery: BatteryStatus;
  keyboardLayout: string;
  audio: AudioStatus;
  network: NetworkStatus;
  isFullscreenActive: boolean;
  media: MediaSessionInfo;
}

export interface AppState {
  activePanel: PanelName | null;
  heldFiles: HeldFile[];
  tasks: TaskItem[];
  reminders: ReminderItem[];
  notifications: NotificationItem[];
  settings: Record<string, string>;
  system: SystemSnapshot | null;
  monitors: MonitorDescriptor[];
  autostart: boolean;
  focusMode: boolean;
  dndMode: boolean;
  timerRemainingSec: number;
  timerRunning: boolean;
}
