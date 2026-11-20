import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Music,
  FolderUp,
  ClipboardList,
  LayoutGrid,
  BellRing,
  Sliders,
  Rocket,
  Bot,
  Smartphone,
  Bell,
  Settings,
} from "lucide-react";
import {
  AgentSessionState,
  AppConfig,
  AudioState,
  ClipboardItem,
  HardwareStats,
  LauncherSlot,
  LiveTilePayload,
  MediaInfo,
  NotificationItem,
  PcDeckStatus,
  PrivacySensors,
  ReminderItem,
  StagedFile,
  TabId,
  TodoItem,
} from "./types";
import { RestingPill } from "./components/RestingPill";
import { MusicTab } from "./components/MusicTab";
import { ShelfTab } from "./components/ShelfTab";
import { ClipboardTab } from "./components/ClipboardTab";
import { WidgetsTab } from "./components/WidgetsTab";
import { RemindersTab } from "./components/RemindersTab";
import { ControlsTab } from "./components/ControlsTab";
import { LauncherTab } from "./components/LauncherTab";
import { AgentsTab } from "./components/AgentsTab";
import { DeckTab } from "./components/DeckTab";
import { NotificationsTab } from "./components/NotificationsTab";
import { SettingsTab } from "./components/SettingsTab";

const DEFAULT_CONFIG: AppConfig = {
  shape: "island",
  position: "center",
  theme: "liquid-glass",
  glassBlur: 20,
  cornerRadius: 22,
  hoverDelayMs: 140,
  autoHide: false,
  hideFromCapture: false,
  fullscreenMode: "slim",
  restingFace: "clock",
  visualizerStyle: "rounded",
  lastActiveTab: "music",
  clock24h: false,
  weatherCity: "Chennai",
  weatherLat: 13.0827,
  weatherLon: 80.2707,
  clipboardRetentionHours: 10,
};

const TABS: { id: TabId; label: string; icon: React.ReactNode }[] = [
  { id: "music", label: "Music", icon: <Music size={12} /> },
  { id: "shelf", label: "Shelf", icon: <FolderUp size={12} /> },
  { id: "clipboard", label: "Clip", icon: <ClipboardList size={12} /> },
  { id: "widgets", label: "Widgets", icon: <LayoutGrid size={12} /> },
  { id: "reminders", label: "Remind", icon: <BellRing size={12} /> },
  { id: "controls", label: "Controls", icon: <Sliders size={12} /> },
  { id: "launcher", label: "Apps", icon: <Rocket size={12} /> },
  { id: "agents", label: "Agents", icon: <Bot size={12} /> },
  { id: "deck", label: "Deck", icon: <Smartphone size={12} /> },
  { id: "notifications", label: "Inbox", icon: <Bell size={12} /> },
  { id: "settings", label: "Settings", icon: <Settings size={12} /> },
];

export default function App() {
  const [config, setConfig] = useState<AppConfig>(DEFAULT_CONFIG);
  const [expanded, setExpanded] = useState(false);
  const [hidden, setHidden] = useState(false);
  const [activeTab, setActiveTab] = useState<TabId>("music");
  const [toast, setToast] = useState<string | null>(null);

  const [hw, setHw] = useState<HardwareStats>({
    cpuPercent: 0,
    ramUsedGb: 0,
    ramTotalGb: 16,
    ramPercent: 0,
    netDownMbps: 0,
    netUpMbps: 0,
    isFullscreen: false,
  });
  const [media, setMedia] = useState<MediaInfo>({
    hasSession: false,
    title: "",
    artist: "",
    album: "",
    sourceApp: "",
    isPlaying: false,
    positionSecs: 0,
    durationSecs: 0,
  });
  const [audio, setAudio] = useState<AudioState>({
    volumePercent: 50,
    outputMuted: false,
    micMuted: false,
  });
  const [sensors, setSensors] = useState<PrivacySensors>({
    micActiveApps: [],
    camActiveApps: [],
  });
  const [clipboard, setClipboard] = useState<ClipboardItem[]>([]);
  const [reminders, setReminders] = useState<ReminderItem[]>([]);
  const [todos, setTodos] = useState<TodoItem[]>([]);
  const [notifications, setNotifications] = useState<NotificationItem[]>([]);
  const [slots, setSlots] = useState<LauncherSlot[]>([]);
  const [agents, setAgents] = useState<AgentSessionState[]>([]);
  const [liveTiles, setLiveTiles] = useState<LiveTilePayload[]>([]);
  const [shelfFiles, setShelfFiles] = useState<StagedFile[]>([]);
  const [deckStatus, setDeckStatus] = useState<PcDeckStatus>({
    active: false,
    url: "",
    qrSvg: "",
    expiresAt: 0,
    pendingDevice: null,
    pairedDevice: null,
  });

  const hoverTimer = useRef<number | null>(null);
  const toastTimer = useRef<number | null>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3200);
  }, []);

  const refreshClipboard = useCallback(async () => {
    try {
      setClipboard(await invoke<ClipboardItem[]>("list_clipboard"));
    } catch {}
  }, []);

  const refreshReminders = useCallback(async () => {
    try {
      setReminders(await invoke<ReminderItem[]>("list_reminders"));
    } catch {}
  }, []);

  const refreshTodos = useCallback(async () => {
    try {
      setTodos(await invoke<TodoItem[]>("list_todos"));
    } catch {}
  }, []);

  const refreshNotifications = useCallback(async () => {
    try {
      setNotifications(await invoke<NotificationItem[]>("list_notifications"));
    } catch {}
  }, []);

  const refreshSlots = useCallback(async () => {
    try {
      setSlots(await invoke<LauncherSlot[]>("list_launcher_slots"));
    } catch {}
  }, []);

  const refreshMedia = useCallback(async () => {
    try {
      setMedia(await invoke<MediaInfo>("get_media_info"));
    } catch {}
  }, []);

  const refreshAudio = useCallback(async () => {
    try {
      setAudio(await invoke<AudioState>("get_audio_state"));
    } catch {}
  }, []);

  // Initial load
  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((c) => {
        setConfig(c);
        setActiveTab(c.lastActiveTab || "music");
      })
      .catch(() => {});
    refreshClipboard();
    refreshReminders();
    refreshTodos();
    refreshNotifications();
    refreshSlots();
    refreshMedia();
    refreshAudio();
    invoke<AgentSessionState[]>("list_agents").then(setAgents).catch(() => {});
    invoke<LiveTilePayload[]>("list_live_tiles").then(setLiveTiles).catch(() => {});
    invoke<PcDeckStatus>("get_pc_deck_status").then(setDeckStatus).catch(() => {});
  }, [
    refreshAudio,
    refreshClipboard,
    refreshMedia,
    refreshNotifications,
    refreshReminders,
    refreshSlots,
    refreshTodos,
  ]);

  // Periodic polling for hardware, media, privacy sensors & due reminders
  useEffect(() => {
    const id = window.setInterval(async () => {
      try {
        const [h, m, s] = await Promise.all([
          invoke<HardwareStats>("get_hardware_stats"),
          invoke<MediaInfo>("get_media_info"),
          invoke<PrivacySensors>("get_privacy_sensors"),
        ]);
        setHw(h);
        setMedia(m);
        setSensors(s);
      } catch {}
    }, 1500);
    return () => window.clearInterval(id);
  }, []);

  // Tauri event listeners (hotkeys, clipboard, agents, live tiles, PC Deck pairing, drag-drop)
  useEffect(() => {
    const unlisteners: Promise<() => void>[] = [
      listen("hotkey-toggle-expand", () => {
        setHidden(false);
        setExpanded((prev) => !prev);
      }),
      listen("hotkey-toggle-hide", () => {
        setHidden((prev) => !prev);
        setExpanded(false);
      }),
      listen<string>("hud-toast", (e) => {
        showToast(e.payload);
        refreshAudio();
      }),
      listen("clipboard-updated", () => {
        refreshClipboard();
      }),
      listen<AgentSessionState>("agent-notify", (e) => {
        showToast(`${e.payload.agent}: ${e.payload.title}`);
        invoke<AgentSessionState[]>("list_agents").then(setAgents).catch(() => {});
        refreshNotifications();
      }),
      listen<LiveTilePayload>("live-tile", () => {
        invoke<LiveTilePayload[]>("list_live_tiles").then(setLiveTiles).catch(() => {});
      }),
      listen<string>("deck-pair-request", (e) => {
        showToast(`PC Deck pair request: ${e.payload}`);
        invoke<PcDeckStatus>("get_pc_deck_status").then(setDeckStatus).catch(() => {});
        setActiveTab("deck");
        setExpanded(true);
      }),
      listen<{ paths: string[] }>("tauri://drag-drop", (e) => {
        if (e.payload?.paths?.length) {
          setShelfFiles((prev) => {
            const next = [...prev];
            for (const p of e.payload.paths) {
              if (next.length >= 12) break;
              const name = p.split(/[\\/]/).pop() || p;
              next.push({
                id: `${Date.now()}-${Math.random()}`,
                name,
                path: p,
                sizeBytes: 0,
              });
            }
            return next;
          });
          setActiveTab("shelf");
          setExpanded(true);
          showToast(`Added ${e.payload.paths.length} file(s) to Shelf`);
        }
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((u) => u()));
    };
  }, [refreshAudio, refreshClipboard, refreshNotifications, showToast]);

  const persistConfig = async (next: AppConfig) => {
    setConfig(next);
    try {
      await invoke("update_config", { newConfig: next });
    } catch {}
  };

  const selectTab = (tab: TabId) => {
    setActiveTab(tab);
    persistConfig({ ...config, lastActiveTab: tab });
  };

  const isGameStrip = hw.isFullscreen && config.fullscreenMode === "slim" && !expanded;
  const isFullscreenHidden = hw.isFullscreen && config.fullscreenMode === "hide" && !expanded;

  const handleMouseEnter = () => {
    if (hw.isFullscreen && config.fullscreenMode !== "normal") return;
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    hoverTimer.current = window.setTimeout(() => {
      setExpanded(true);
    }, config.hoverDelayMs);
  };

  const handleMouseLeave = () => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    setExpanded(false);
  };

  if (hidden || isFullscreenHidden) {
    return <div className="stage" />;
  }

  const stateClass = expanded
    ? "state-expanded"
    : isGameStrip
    ? "state-gamestrip"
    : config.autoHide
    ? "state-hidden"
    : "state-resting";

  return (
    <div
      className={`stage theme-${config.theme}`}
      style={
        {
          "--glass-blur": `${config.glassBlur}px`,
          "--radius": `${config.cornerRadius}px`,
        } as React.CSSProperties
      }
    >
      <div
        className={`island-shell shape-${config.shape} ${stateClass}`}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={handleMouseLeave}
        onClick={() => !expanded && setExpanded(true)}
      >
        {!expanded ? (
          <RestingPill
            config={config}
            hw={hw}
            media={media}
            sensors={sensors}
            liveTiles={liveTiles}
            isGameStrip={isGameStrip}
          />
        ) : (
          <>
            <div className="tab-bar">
              {TABS.map((t) => (
                <button
                  key={t.id}
                  className={`tab-btn ${activeTab === t.id ? "active" : ""}`}
                  onClick={() => selectTab(t.id)}
                >
                  {t.icon}
                  <span>{t.label}</span>
                </button>
              ))}
            </div>
            <div className="panel-body">
              {activeTab === "music" && (
                <MusicTab media={media} config={config} onRefresh={refreshMedia} />
              )}
              {activeTab === "shelf" && (
                <ShelfTab
                  files={shelfFiles}
                  onAddPath={(p) =>
                    setShelfFiles((prev) =>
                      prev.length >= 12
                        ? prev
                        : [
                            ...prev,
                            {
                              id: `${Date.now()}-${Math.random()}`,
                              name: p.split(/[\\/]/).pop() || p,
                              path: p,
                              sizeBytes: 0,
                            },
                          ]
                    )
                  }
                  onRemove={(id) =>
                    setShelfFiles((prev) => prev.filter((f) => f.id !== id))
                  }
                  onClear={() => setShelfFiles([])}
                  onToast={showToast}
                />
              )}
              {activeTab === "clipboard" && (
                <ClipboardTab
                  items={clipboard}
                  onRefresh={refreshClipboard}
                  onToast={showToast}
                />
              )}
              {activeTab === "widgets" && (
                <WidgetsTab
                  config={config}
                  hw={hw}
                  todos={todos}
                  onRefreshTodos={refreshTodos}
                  onToast={showToast}
                />
              )}
              {activeTab === "reminders" && (
                <RemindersTab
                  reminders={reminders}
                  onRefresh={refreshReminders}
                  onToast={showToast}
                />
              )}
              {activeTab === "controls" && (
                <ControlsTab
                  audio={audio}
                  sensors={sensors}
                  onRefreshAudio={refreshAudio}
                  onToast={showToast}
                />
              )}
              {activeTab === "launcher" && (
                <LauncherTab
                  slots={slots}
                  onRefresh={refreshSlots}
                  onToast={showToast}
                />
              )}
              {activeTab === "agents" && <AgentsTab agents={agents} />}
              {activeTab === "deck" && (
                <DeckTab
                  status={deckStatus}
                  onUpdate={setDeckStatus}
                  onToast={showToast}
                />
              )}
              {activeTab === "notifications" && (
                <NotificationsTab
                  notifications={notifications}
                  onRefresh={refreshNotifications}
                />
              )}
              {activeTab === "settings" && (
                <SettingsTab config={config} onChange={persistConfig} />
              )}
            </div>
          </>
        )}
      </div>

      {toast && <div className="hud-toast">{toast}</div>}
    </div>
  );
}
