import type { ImageMetadata } from "astro";
import downloads from "@/assets/icons/downloads.webp";
import fileExplorer from "@/assets/icons/file-explorer.webp";
import microsoftPhotos from "@/assets/icons/microsoft-photos.webp";
import microsoftStore from "@/assets/icons/microsoft-store.webp";
import microsoftTeams from "@/assets/icons/microsoft-teams.webp";
import msnWeather from "@/assets/icons/msn-weather.webp";
import recycleBin from "@/assets/icons/recycle-bin.webp";
import settings from "@/assets/icons/settings.webp";
import windowsMediaPlayer from "@/assets/icons/windows-media-player.webp";
import windowsTerminal from "@/assets/icons/windows-terminal.webp";

/** Folders and the recycle bin are drawn with a different shape than running apps. */
export type DockItemKind = "app" | "folder" | "trash";

export interface DockItem {
  id: string;
  label: string;
  icon: ImageMetadata;
  kind: DockItemKind;
  /** Apps that show an open indicator dot in the mockup. */
  open?: boolean;
}

export const dockItems: DockItem[] = [
  { id: "explorer", label: "File Explorer", icon: fileExplorer, kind: "app", open: true },
  { id: "store", label: "Microsoft Store", icon: microsoftStore, kind: "app" },
  { id: "terminal", label: "Windows Terminal", icon: windowsTerminal, kind: "app", open: true },
  { id: "photos", label: "Microsoft Photos", icon: microsoftPhotos, kind: "app" },
  { id: "media-player", label: "Windows Media Player", icon: windowsMediaPlayer, kind: "app" },
  { id: "teams", label: "Microsoft Teams", icon: microsoftTeams, kind: "app" },
  { id: "weather", label: "MSN Weather", icon: msnWeather, kind: "app" },
  { id: "settings", label: "Settings", icon: settings, kind: "app" },
  { id: "downloads", label: "Downloads", icon: downloads, kind: "folder" },
  { id: "trash", label: "Recycle Bin", icon: recycleBin, kind: "trash" },
];
