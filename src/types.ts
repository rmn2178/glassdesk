export type TabId =
  | "music"
  | "shelf"
  | "clipboard"
  | "widgets"
  | "reminders"
  | "controls"
  | "launcher"
  | "agents"
  | "deck"
  | "notifications"
  | "settings";

export interface AppConfig {
  shape: "notch" | "island";
  position: "center" | "left" | "right";
  theme:
    | "liquid-glass"
    | "pure-black"
    | "black"
    | "graphite"
    | "acrylic"
    | "candy-glass"
    | "rose"
    | "lavender"
    | "peach"
    | "sakura"
    | "pure-white";
  glassBlur: number;
  cornerRadius: number;
  hoverDelayMs: number;
  autoHide: boolean;
  hideFromCapture: boolean;
  fullscreenMode: "slim" | "hide" | "normal";
  restingFace: "clock" | "stats" | "media" | "pet";
  visualizerStyle: "rounded" | "pixel";
  lastActiveTab: TabId;
  clock24h: boolean;
  weatherCity: string;
  weatherLat: number;
  weatherLon: number;
  clipboardRetentionHours: number;
}

export interface HardwareStats {
  cpuPercent: number;
  ramUsedGb: number;
  ramTotalGb: number;
  ramPercent: number;
  netDownMbps: number;
  netUpMbps: number;
  isFullscreen: boolean;
}

export interface MediaInfo {
  hasSession: boolean;
  title: string;
  artist: string;
  album: string;
  sourceApp: string;
  isPlaying: boolean;
  positionSecs: number;
  durationSecs: number;
}

export interface LyricLine {
  timeSecs: number;
  text: string;
}

export interface LyricsResponse {
  found: boolean;
  synced: boolean;
  lines: LyricLine[];
  plain: string;
}

export interface AudioState {
  volumePercent: number;
  outputMuted: boolean;
  micMuted: boolean;
}

export interface PrivacySensors {
  micActiveApps: string[];
  camActiveApps: string[];
}

export interface ClipboardItem {
  id: number;
  kind: "text" | "link" | "file" | string;
  content: string;
  isPinned: boolean;
  createdAt: number;
}

export interface ReminderItem {
  id: number;
  title: string;
  dueAt: number;
  repeatRule: "once" | "daily" | "weekdays" | "weekly" | string;
  isDone: boolean;
}

export interface TodoItem {
  id: number;
  text: string;
  completed: boolean;
}

export interface NotificationItem {
  id: number;
  appName: string;
  title: string;
  body: string;
  accent: string;
  receivedAt: number;
}

export interface LauncherSlot {
  slotIndex: number;
  label: string;
  targetPath: string;
}

export interface AgentSessionState {
  agent: string;
  status: "working" | "needs_input" | "done" | "error" | string;
  title: string;
  body: string;
  accent: string;
  updatedAt: number;
}

export interface LiveTilePayload {
  id: string;
  label: string;
  value: string;
  progress?: number | null;
}

export interface PhoneTransferSession {
  active: boolean;
  url: string;
  qrSvg: string;
  fileCount: number;
}

export interface PcDeckStatus {
  active: boolean;
  url: string;
  qrSvg: string;
  expiresAt: number;
  pendingDevice: string | null;
  pairedDevice: string | null;
}

export interface StagedFile {
  id: string;
  name: string;
  path: string;
  sizeBytes: number;
}
