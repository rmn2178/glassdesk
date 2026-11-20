import { listen } from "@tauri-apps/api/event";
import { renderCalendarPanel, startLightweightClock } from "./clock";
import {
  refreshSystemSnapshot,
  renderControlsPanel,
  resetFullscreenAutoHide,
  startSystemMonitor,
  syncIslandStatusDot,
} from "./controls";
import { initNativeDragDrop } from "./dragdrop";
import { refreshHeldFiles, renderFilesPanel } from "./files";
import { callCommand, isTauriRuntime } from "./ipc";
import { IslandController } from "./island";
import { renderMusicPanel } from "./media";
import {
  refreshNotifications,
  renderNotificationsPanel,
  startReminderWatcher,
} from "./notifications";
import { initLiquidGlassPhysics } from "./physics";
import {
  applyVisualPreferences,
  loadSettingsAndMonitors,
  renderSettingsPanel,
} from "./settings";
import { refreshTasks, renderTasksPanel } from "./tasks";
import type { AppState, PanelName, ReminderItem } from "./types";

const state: AppState = {
  activePanel: null,
  heldFiles: [],
  tasks: [],
  reminders: [],
  notifications: [],
  settings: {},
  system: null,
  monitors: [],
  autostart: false,
  focusMode: false,
  dndMode: false,
  timerRemainingSec: 0,
  timerRunning: false,
};

async function refreshReminders(): Promise<void> {
  try {
    state.reminders = await callCommand<ReminderItem[]>("list_reminders");
  } catch {
    state.reminders = [];
  }
}

async function bootstrap(): Promise<void> {
  await loadSettingsAndMonitors(state);
  await Promise.all([
    refreshHeldFiles(state),
    refreshTasks(state),
    refreshReminders(),
    refreshNotifications(state),
  ]);

  const onFilesMutated = async () => {
    await refreshHeldFiles(state);
    if (state.activePanel === "files") {
      renderFilesPanel(state, onFilesMutated);
    } else if (state.activePanel === "hub") {
      islandCtrl.renderHubPanel();
    }
  };

  const onTasksMutated = async () => {
    await refreshTasks(state);
    if (state.activePanel === "tasks") {
      renderTasksPanel(state, onTasksMutated);
    } else if (state.activePanel === "hub") {
      islandCtrl.renderHubPanel();
    } else if (state.activePanel === "calendar") {
      renderCalendarPanel(state, onRemindersMutated, () => void islandCtrl.collapse());
    }
  };

  const onRemindersMutated = async () => {
    await refreshReminders();
    if (state.activePanel === "calendar") {
      renderCalendarPanel(state, onRemindersMutated, () => void islandCtrl.collapse());
    } else if (state.activePanel === "hub") {
      islandCtrl.renderHubPanel();
    }
  };

  const onNotificationsMutated = async () => {
    await refreshNotifications(state);
    if (state.activePanel === "notifications") {
      renderNotificationsPanel(state, onNotificationsMutated, () => void islandCtrl.collapse());
    } else if (state.activePanel === "hub") {
      islandCtrl.renderHubPanel();
    }
  };

  const renderPanel = (panel: PanelName) => {
    switch (panel) {
      case "hub":
        islandCtrl.renderHubPanel();
        break;
      case "files":
        renderFilesPanel(state, onFilesMutated);
        break;
      case "calendar":
        renderCalendarPanel(state, onRemindersMutated, () => void islandCtrl.collapse());
        break;
      case "tasks":
        renderTasksPanel(state, onTasksMutated);
        break;
      case "controls":
        renderControlsPanel(
          state,
          (target) => void islandCtrl.openPanel(target),
          () => void islandCtrl.collapse()
        );
        break;
      case "notifications":
        renderNotificationsPanel(state, onNotificationsMutated, () => void islandCtrl.collapse());
        break;
      case "music":
        renderMusicPanel(
          state,
          () => {
            if (state.activePanel === "controls") {
              renderControlsPanel(
                state,
                (target) => void islandCtrl.openPanel(target),
                () => void islandCtrl.collapse()
              );
            }
          },
          () => {
            void islandCtrl.collapse();
          }
        );
        break;
      case "settings":
        renderSettingsPanel(state, islandCtrl, onFilesMutated, onNotificationsMutated);
        break;
    }
  };

  const islandCtrl = new IslandController(state, renderPanel);

  // Apply initial floating pill or top-bar bounds
  await islandCtrl.setOverlayHeight(islandCtrl.getCollapsedHeight(), false, false);
  syncIslandStatusDot(state);

  // Sync system snapshot & shortcut actions from Rust global shortcut handler
  if (isTauriRuntime()) {
    void listen("glassdesk://shortcut-toggle", async () => {
      resetFullscreenAutoHide(3000);
      await refreshSystemSnapshot(state);
    }).catch(() => {});

    void listen<string>("glassdesk://shortcut-action", async (event) => {
      resetFullscreenAutoHide(3000);
      const target = event.payload as PanelName;
      if (target) {
        await islandCtrl.togglePanel(target);
      }
    }).catch(() => {});
  }

  // Wire Dynamic Island capsule & top-bar triggers
  document.getElementById("brand-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("hub");
  });

  document.getElementById("dynamic-island")?.addEventListener("click", (e) => {
    if (state.settings["click_expand"] === "false") return;
    const target = e.target as HTMLElement;
    if (
      target.closest("#island-time-badge") ||
      target.closest("#island-timer-badge") ||
      target.closest("#island-files-badge") ||
      target.closest("#island-tasks-badge") ||
      target.closest("#island-music-badge") ||
      target.closest("#island-notif-badge") ||
      target.closest("#island-controls-badge")
    ) {
      return;
    }
    const defaultMode = (state.settings["default_expanded_mode"] || "hub") as PanelName;
    void islandCtrl.togglePanel(defaultMode);
  });

  document.getElementById("island-time-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("calendar");
  });

  document.getElementById("island-timer-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("island-music-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("music");
  });

  document.getElementById("island-files-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("files");
  });

  document.getElementById("island-tasks-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("tasks");
  });

  document.getElementById("island-notif-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("notifications");
  });

  document.getElementById("island-controls-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("sys-clock-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("calendar");
  });

  document.getElementById("sys-controls-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("sys-volume-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("sys-wifi-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("sys-battery-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("controls");
  });

  document.getElementById("sys-kbd-btn")?.addEventListener("click", () => {
    void callCommand("open_windows_settings", { uri: "ms-settings:regionlanguage" }).catch(() => {});
  });

  document.getElementById("sys-settings-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("settings");
  });

  window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
    applyVisualPreferences(state.settings);
  });

  initLiquidGlassPhysics();
  startLightweightClock(state);
  startSystemMonitor(state);
  startReminderWatcher(state, () => {
    if (state.activePanel === "notifications") {
      renderNotificationsPanel(state, onNotificationsMutated, () => void islandCtrl.collapse());
    }
  });
  await initNativeDragDrop(state, islandCtrl, onFilesMutated);
}

void bootstrap();
