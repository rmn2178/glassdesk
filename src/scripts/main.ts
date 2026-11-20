import { listen } from "@tauri-apps/api/event";
import { renderCalendarPanel, startLightweightClock } from "./clock";
import {
  refreshSystemSnapshot,
  renderControlsPanel,
  resetFullscreenAutoHide,
  startSystemMonitor,
} from "./controls";
import { initNativeDragDrop } from "./dragdrop";
import { refreshHeldFiles, renderFilesPanel } from "./files";
import { callCommand, isTauriRuntime } from "./ipc";
import { IslandController } from "./island";
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
  settings: {},
  system: null,
  monitors: [],
  autostart: false,
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
      renderCalendarPanel(state, onRemindersMutated);
    }
  };

  const onRemindersMutated = async () => {
    await refreshReminders();
    if (state.activePanel === "calendar") {
      renderCalendarPanel(state, onRemindersMutated);
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
        renderCalendarPanel(state, onRemindersMutated);
        break;
      case "tasks":
        renderTasksPanel(state, onTasksMutated);
        break;
      case "controls":
        renderControlsPanel(state);
        break;
      case "settings":
        renderSettingsPanel(state, islandCtrl, onFilesMutated);
        break;
    }
  };

  const islandCtrl = new IslandController(state, renderPanel);

  // Sync system snapshot & fullscreen grace window when restored via global shortcut or tray
  if (isTauriRuntime()) {
    void listen("glassdesk://shortcut-toggle", async () => {
      resetFullscreenAutoHide(3000);
      await refreshSystemSnapshot(state);
    }).catch(() => {});
  }

  // Wire top-bar triggers
  document.getElementById("brand-btn")?.addEventListener("click", () => {
    void islandCtrl.togglePanel("hub");
  });

  document.getElementById("dynamic-island")?.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;
    if (target.closest("#island-files-badge") || target.closest("#island-tasks-badge")) {
      return;
    }
    void islandCtrl.togglePanel("hub");
  });

  document.getElementById("island-files-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("files");
  });

  document.getElementById("island-tasks-badge")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void islandCtrl.togglePanel("tasks");
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

  // React to OS light/dark theme changes when Theme is set to "auto"
  window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
    applyVisualPreferences(state.settings);
  });

  // Start lightweight clock, Windows system monitor, and native Explorer drag-drop
  startLightweightClock(state);
  startSystemMonitor(state);
  await initNativeDragDrop(state, islandCtrl, onFilesMutated);
}

void bootstrap();
