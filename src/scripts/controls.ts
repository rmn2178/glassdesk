import { syncMusicPill } from "./media";
import { callCommand, escapeHtml, ICONS, isTauriRuntime, showToast } from "./ipc";
import { pushAppNotification } from "./notifications";
import { applyVisualPreferences } from "./settings";
import type { AppState, AudioStatus, PanelName, SystemSnapshot } from "./types";

let hiddenByFullscreen = false;
let manualRestoreGraceUntil = 0;

let isDraggingVolume = false;
let lastVolSentAt = 0;
let lastVolSent: number | null = null;
let pendingVol: number | null = null;
let volTimer: number | null = null;
const VOL_THROTTLE_MS = 50;

let focusTimerHandle: number | null = null;

export const DEFAULT_CC_MODULES = [
  "status",
  "connectivity",
  "display",
  "audio",
  "focus",
  "apps",
];

export function getControlCenterModules(settings: Record<string, string>): string[] {
  const raw = settings["control_center_modules"];
  if (!raw) return [...DEFAULT_CC_MODULES];
  const parsed = raw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => DEFAULT_CC_MODULES.includes(s));
  return parsed.length > 0 ? parsed : [...DEFAULT_CC_MODULES];
}

export function resetFullscreenAutoHide(graceMs = 3000): void {
  hiddenByFullscreen = false;
  manualRestoreGraceUntil = Date.now() + graceMs;
}

function formatTimer(sec: number): string {
  const clamped = Math.max(0, Math.floor(sec));
  const m = String(Math.floor(clamped / 60)).padStart(2, "0");
  const s = String(clamped % 60).padStart(2, "0");
  return `${m}:${s}`;
}

export function syncIslandStatusDot(state: AppState): void {
  const dot = document.getElementById("island-indicator");
  if (!dot) return;
  dot.classList.toggle("is-focus", state.focusMode && !state.dndMode);
  dot.classList.toggle("is-dnd", state.dndMode);
  dot.title = state.dndMode
    ? "Do Not Disturb Active"
    : state.focusMode
      ? "Focus Mode Active"
      : "GLASSDESK Active";

  const timerBadge = document.getElementById("island-timer-badge");
  const timerLabel = document.getElementById("island-timer-label");
  if (timerBadge && timerLabel) {
    timerBadge.hidden = !state.timerRunning && state.timerRemainingSec <= 0;
    timerLabel.textContent = formatTimer(state.timerRemainingSec);
  }
}

export function syncTopBarSystemIndicators(state: AppState): void {
  syncIslandStatusDot(state);
  const sys = state.system;
  if (!sys) return;

  const appNameEl = document.getElementById("active-app-name");
  if (appNameEl && !sys.activeWindow.isSelf) {
    const label = sys.activeWindow.executable || sys.activeWindow.title;
    if (label) {
      appNameEl.textContent = label;
      appNameEl.title = sys.activeWindow.title || label;
    }
  }

  const kbdEl = document.getElementById("sys-kbd-code");
  if (kbdEl) kbdEl.textContent = sys.keyboardLayout || "ENG";

  const batIcon = document.getElementById("sys-battery-icon");
  const batPct = document.getElementById("sys-battery-pct");
  if (batIcon) {
    batIcon.innerHTML = sys.battery.isCharging ? ICONS.batteryCharging : ICONS.battery;
  }
  if (batPct) {
    batPct.textContent =
      sys.battery.hasBattery && sys.battery.percent !== null
        ? `${sys.battery.percent}%`
        : "AC";
  }

  const volIcon = document.getElementById("sys-volume-icon");
  if (volIcon) {
    volIcon.innerHTML =
      sys.audio.muted || sys.audio.volumePercent === 0 ? ICONS.volMuted : ICONS.volHigh;
  }

  if (!isDraggingVolume && volTimer === null) {
    const slider = document.getElementById("ctrl-vol-slider") as HTMLInputElement | null;
    const readout = document.getElementById("ctrl-vol-readout");
    if (slider && readout) {
      slider.value = String(sys.audio.volumePercent);
      readout.textContent = sys.audio.muted ? "Muted" : `${sys.audio.volumePercent}%`;
    }
  }

  syncMusicPill(state);

  const wifiIcon = document.getElementById("sys-wifi-icon");
  if (wifiIcon) {
    wifiIcon.innerHTML = sys.network.connected ? ICONS.wifiOn : ICONS.wifiOff;
  }
}

export async function refreshSystemSnapshot(state: AppState): Promise<void> {
  if (!isTauriRuntime() || document.hidden) return;
  try {
    const snap = await callCommand<SystemSnapshot>("get_system_snapshot");
    state.system = snap;
    syncTopBarSystemIndicators(state);

    if (Date.now() < manualRestoreGraceUntil) return;

    const autoHide = state.settings["auto_hide_fullscreen"] !== "false";
    const enabled = state.settings["enabled"] !== "false";
    if (enabled && autoHide) {
      if (snap.isFullscreenActive && !hiddenByFullscreen && !state.activePanel) {
        hiddenByFullscreen = true;
        await callCommand("set_overlay_visible", { visible: false });
      } else if (!snap.isFullscreenActive && hiddenByFullscreen) {
        hiddenByFullscreen = false;
        await callCommand("set_overlay_visible", { visible: true });
      }
    }
  } catch {
    // Ignore transient polling errors
  }
}

export function startSystemMonitor(state: AppState): void {
  let pollTimer: number | null = null;
  const scheduleNext = () => {
    if (pollTimer !== null) {
      window.clearTimeout(pollTimer);
      pollTimer = null;
    }
    if (document.hidden) return;
    const baseMs = Number.parseInt(state.settings["poll_interval_ms"] || "5000", 10) || 5000;
    const delayMs = state.activePanel === "controls" ? 2000 : Math.max(2000, baseMs);
    pollTimer = window.setTimeout(async () => {
      await refreshSystemSnapshot(state);
      scheduleNext();
    }, delayMs);
  };
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      void refreshSystemSnapshot(state).finally(scheduleNext);
    } else if (pollTimer !== null) {
      window.clearTimeout(pollTimer);
      pollTimer = null;
    }
  });
  void refreshSystemSnapshot(state).finally(scheduleNext);
}

function commitVolume(state: AppState, pct: number): void {
  if (pct === lastVolSent) return;
  lastVolSent = pct;
  lastVolSentAt = performance.now();
  void callCommand<AudioStatus>("set_system_volume", { level: pct / 100 })
    .then((updated) => {
      if (state.system) {
        state.system.audio = updated;
        syncTopBarSystemIndicators(state);
      }
    })
    .catch(() => {});
}

function startOrPauseTimer(state: AppState, onTick: () => void): void {
  if (state.timerRunning) {
    state.timerRunning = false;
    if (focusTimerHandle !== null) {
      window.clearInterval(focusTimerHandle);
      focusTimerHandle = null;
    }
    syncIslandStatusDot(state);
    onTick();
    return;
  }

  if (state.timerRemainingSec <= 0) {
    state.timerRemainingSec = 25 * 60;
  }
  state.timerRunning = true;
  syncIslandStatusDot(state);
  onTick();

  if (focusTimerHandle !== null) window.clearInterval(focusTimerHandle);
  focusTimerHandle = window.setInterval(() => {
    if (!state.timerRunning) return;
    state.timerRemainingSec = Math.max(0, state.timerRemainingSec - 1);
    syncIslandStatusDot(state);
    const readout = document.getElementById("cc-timer-readout");
    if (readout) readout.textContent = formatTimer(state.timerRemainingSec);

    if (state.timerRemainingSec === 0) {
      state.timerRunning = false;
      if (focusTimerHandle !== null) {
        window.clearInterval(focusTimerHandle);
        focusTimerHandle = null;
      }
      void pushAppNotification(state, "Focus Timer Complete", "Your focus session has ended.", "timer");
      onTick();
    }
  }, 1000);
}

export function renderControlsPanel(
  state: AppState,
  onOpenPanel?: (panel: PanelName) => void,
  onClose?: () => void
): void {
  const el = document.getElementById("panel-controls");
  if (!el) return;

  const sys = state.system;
  const now = new Date();
  const use24h = state.settings["clock_format"] === "24h";
  const timeStr = now.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: !use24h,
  });
  const dateStr = now.toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
  });

  const netLabel = sys?.network.internet
    ? sys.network.networkName || "Connected"
    : sys?.network.connected
      ? "Local Only"
      : "Offline";

  const batLabel =
    sys?.battery.hasBattery && sys.battery.percent !== null
      ? `${sys.battery.percent}%${sys.battery.isCharging ? " · Charging" : ""}`
      : "AC Power";

  const volPct = sys?.audio.volumePercent ?? 0;
  const isMuted = sys?.audio.muted ?? false;
  const outputDevice = sys?.audio.outputDevice || "Default Output";
  const unreadNotifs = state.notifications.filter((n) => !n.read).length;
  const theme = state.settings["theme"] || "dark";
  const modules = getControlCenterModules(state.settings);

  const sectionHtml: Record<string, string> = {
    status: `
      <div class="glass-card cc-status-strip">
        <div>
          <div class="cc-status-time">${escapeHtml(timeStr)}</div>
          <div class="ctrl-sub">${escapeHtml(dateStr)} · ${state.dndMode ? "Do Not Disturb" : state.focusMode ? "Focus Active" : "Ready"}</div>
        </div>
        <div class="cc-status-badges">
          <span class="cc-mini-pill">${sys?.battery.isCharging ? ICONS.batteryCharging : ICONS.battery} ${escapeHtml(batLabel)}</span>
          <button class="cc-mini-pill" data-cc-nav="notifications" type="button" title="Open Notifications">
            ${ICONS.bell} ${unreadNotifs}
          </button>
        </div>
      </div>`,

    connectivity: `
      <div>
        <div class="cc-section-label">Connectivity</div>
        <div class="controls-grid">
          <button class="glass-card ctrl-tile ${sys?.network.connected ? "is-active" : ""}" data-ms-settings="ms-settings:network-wifi" type="button" title="Open Windows Wi-Fi Settings">
            <div class="ctrl-icon-box">${sys?.network.connected ? ICONS.wifiOn : ICONS.wifiOff}</div>
            <div style="min-width:0;">
              <div class="ctrl-label">Wi-Fi</div>
              <div class="ctrl-sub" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(netLabel)}</div>
            </div>
          </button>

          <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:bluetooth" type="button" title="Open Windows Bluetooth Settings">
            <div class="ctrl-icon-box">${ICONS.bluetooth}</div>
            <div>
              <div class="ctrl-label">Bluetooth</div>
              <div class="ctrl-sub">Open Settings</div>
            </div>
          </button>

          <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:network-airplanemode" type="button" title="Open Windows Airplane Mode Settings">
            <div class="ctrl-icon-box">${ICONS.airplane}</div>
            <div>
              <div class="ctrl-label">Airplane Mode</div>
              <div class="ctrl-sub">Open Settings</div>
            </div>
          </button>

          <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:network" type="button" title="Open Windows Network Settings">
            <div class="ctrl-icon-box">${ICONS.globe}</div>
            <div>
              <div class="ctrl-label">Network</div>
              <div class="ctrl-sub">All Adapters</div>
            </div>
          </button>
        </div>
      </div>`,

    display: `
      <div>
        <div class="cc-section-label">Display & Appearance</div>
        <div class="controls-grid">
          <button class="glass-card ctrl-tile" id="cc-toggle-theme" type="button" title="Cycle Dark / Light / Auto Glass Appearance">
            <div class="ctrl-icon-box">${theme === "light" ? ICONS.sun : ICONS.moon}</div>
            <div>
              <div class="ctrl-label">Glass Theme</div>
              <div class="ctrl-sub">${theme === "dark" ? "Dark Glass" : theme === "light" ? "Light Glass" : "Automatic"}</div>
            </div>
          </button>

          <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:display" type="button" title="Open Windows Display & Brightness Settings">
            <div class="ctrl-icon-box">${ICONS.monitor}</div>
            <div>
              <div class="ctrl-label">Display</div>
              <div class="ctrl-sub">Brightness & Scale</div>
            </div>
          </button>
        </div>
      </div>`,

    audio: `
      <div>
        <div class="cc-section-label">Sound</div>
        <div class="glass-card volume-box">
          <div style="display: flex; justify-content: space-between; align-items: center; gap: 8px;">
            <span class="ctrl-label" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;">${escapeHtml(outputDevice)}</span>
            <span class="ctrl-sub" id="ctrl-vol-readout">${isMuted ? "Muted" : `${volPct}%`}</span>
          </div>
          <div class="volume-row">
            <button class="gd-btn" id="ctrl-mute-btn" type="button" title="Toggle Mute">
              ${isMuted ? ICONS.volMuted : ICONS.volHigh}
            </button>
            <input
              id="ctrl-vol-slider"
              class="gd-range"
              type="range"
              min="0"
              max="100"
              value="${volPct}"
              aria-label="Master Volume"
            />
            <button class="gd-btn" data-ms-settings="ms-settings:sound" type="button" title="Open Windows Sound Settings">Mixer</button>
          </div>
        </div>
      </div>`,

    focus: `
      <div>
        <div class="cc-section-label">Focus & Timer</div>
        <div class="controls-grid" style="margin-bottom: 7px;">
          <button class="glass-card ctrl-tile ${state.focusMode ? "is-active" : ""}" id="cc-focus-btn" type="button">
            <div class="ctrl-icon-box">${ICONS.target}</div>
            <div>
              <div class="ctrl-label">Focus Mode</div>
              <div class="ctrl-sub">${state.focusMode ? "Active" : "Off"}</div>
            </div>
          </button>

          <button class="glass-card ctrl-tile ${state.dndMode ? "is-active" : ""}" id="cc-dnd-btn" type="button">
            <div class="ctrl-icon-box">${state.dndMode ? ICONS.bellOff : ICONS.bell}</div>
            <div>
              <div class="ctrl-label">Do Not Disturb</div>
              <div class="ctrl-sub">${state.dndMode ? "Alerts Silenced" : "Allow Alerts"}</div>
            </div>
          </button>
        </div>

        <div class="glass-card cc-timer-bar">
          <div style="display:flex;align-items:center;gap:8px;">
            <span class="gd-icon" style="color:var(--accent);">${ICONS.clock}</span>
            <span class="cc-timer-readout" id="cc-timer-readout">${formatTimer(state.timerRemainingSec || 25 * 60)}</span>
          </div>
          <div style="display:flex;align-items:center;gap:4px;">
            <button class="gd-btn" data-timer-preset="300" type="button">5m</button>
            <button class="gd-btn" data-timer-preset="900" type="button">15m</button>
            <button class="gd-btn" data-timer-preset="1500" type="button">25m</button>
            <button class="gd-btn gd-btn-primary" id="cc-timer-toggle" type="button">${state.timerRunning ? "Pause" : "Start"}</button>
          </div>
        </div>
      </div>`,

    apps: `
      <div>
        <div class="cc-section-label">Workspace Quick Controls</div>
        <div class="cc-quick-row">
          <button class="glass-card cc-quick-btn" data-cc-nav="tasks" type="button">${ICONS.tasks}<span>Tasks</span></button>
          <button class="glass-card cc-quick-btn" data-cc-nav="files" type="button">${ICONS.folder}<span>Files</span></button>
          <button class="glass-card cc-quick-btn" data-cc-nav="calendar" type="button">${ICONS.calendar}<span>Calendar</span></button>
          <button class="glass-card cc-quick-btn" data-cc-nav="settings" type="button">${ICONS.settings}<span>Settings</span></button>
        </div>
      </div>`,
  };

  el.innerHTML = `
    <div class="panel-header gd-drag-handle" title="Drag to move panel · Double-click to recenter">
      <div class="panel-title-group">
        <div class="mac-lights">
          <button class="mac-dot mac-close" data-mac-close type="button" aria-label="Close Control Center"></button>
          <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
        </div>
        <span class="panel-title">Control Center</span>
      </div>
      <div class="panel-actions">
        <button class="gd-btn" data-ms-settings="ms-settings:" type="button">Windows Settings</button>
      </div>
    </div>

    <div class="cc-scroll">
      ${modules.map((m) => sectionHtml[m] ?? "").join("")}
    </div>
  `;

  el.querySelector("[data-mac-close]")?.addEventListener("click", (e) => {
    e.stopPropagation();
    onClose?.();
  });

  el.querySelectorAll<HTMLElement>("[data-cc-nav]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const target = btn.dataset.ccNav as PanelName | undefined;
      if (target) onOpenPanel?.(target);
    });
  });

  el.querySelectorAll<HTMLElement>("[data-ms-settings]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const uri = btn.dataset.msSettings;
      if (!uri) return;
      try {
        await callCommand("open_windows_settings", { uri });
      } catch (e) {
        showToast(`Could not open settings: ${String(e)}`);
      }
    });
  });

  el.querySelector("#cc-toggle-theme")?.addEventListener("click", async () => {
    const order = ["dark", "light", "auto"];
    const next = order[(order.indexOf(theme) + 1) % order.length];
    state.settings["theme"] = next;
    applyVisualPreferences(state.settings);
    void callCommand("set_setting", { key: "theme", value: next }).catch(() => {});
    renderControlsPanel(state, onOpenPanel, onClose);
  });

  el.querySelector("#cc-focus-btn")?.addEventListener("click", () => {
    state.focusMode = !state.focusMode;
    syncIslandStatusDot(state);
    showToast(state.focusMode ? "Focus Mode enabled" : "Focus Mode off");
    renderControlsPanel(state, onOpenPanel, onClose);
  });

  el.querySelector("#cc-dnd-btn")?.addEventListener("click", () => {
    state.dndMode = !state.dndMode;
    state.settings["dnd_enabled"] = state.dndMode ? "true" : "false";
    void callCommand("set_setting", {
      key: "dnd_enabled",
      value: state.dndMode ? "true" : "false",
    }).catch(() => {});
    syncIslandStatusDot(state);
    showToast(state.dndMode ? "Do Not Disturb enabled" : "Do Not Disturb off");
    renderControlsPanel(state, onOpenPanel, onClose);
  });

  el.querySelectorAll<HTMLElement>("[data-timer-preset]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const secs = Number.parseInt(btn.dataset.timerPreset || "1500", 10);
      state.timerRunning = false;
      if (focusTimerHandle !== null) {
        window.clearInterval(focusTimerHandle);
        focusTimerHandle = null;
      }
      state.timerRemainingSec = secs;
      syncIslandStatusDot(state);
      renderControlsPanel(state, onOpenPanel, onClose);
    });
  });

  el.querySelector("#cc-timer-toggle")?.addEventListener("click", () => {
    startOrPauseTimer(state, () => renderControlsPanel(state, onOpenPanel, onClose));
  });

  const slider = el.querySelector<HTMLInputElement>("#ctrl-vol-slider");
  const readout = el.querySelector<HTMLElement>("#ctrl-vol-readout");

  slider?.addEventListener("pointerdown", () => {
    isDraggingVolume = true;
  });

  slider?.addEventListener("input", () => {
    const pct = Math.max(0, Math.min(100, Math.round(Number(slider.value))));
    if (readout) readout.textContent = `${pct}%`;
    pendingVol = pct;

    const elapsed = performance.now() - lastVolSentAt;
    if (elapsed >= VOL_THROTTLE_MS) {
      if (volTimer !== null) {
        window.clearTimeout(volTimer);
        volTimer = null;
      }
      commitVolume(state, pct);
      pendingVol = null;
    } else if (volTimer === null) {
      volTimer = window.setTimeout(() => {
        volTimer = null;
        if (pendingVol !== null) {
          commitVolume(state, pendingVol);
          pendingVol = null;
        }
      }, VOL_THROTTLE_MS - elapsed);
    }
  });

  slider?.addEventListener("change", () => {
    isDraggingVolume = false;
    if (volTimer !== null) {
      window.clearTimeout(volTimer);
      volTimer = null;
    }
    const finalPct = Math.max(0, Math.min(100, Math.round(Number(slider.value))));
    pendingVol = null;
    commitVolume(state, finalPct);
  });

  slider?.addEventListener("pointerup", () => {
    isDraggingVolume = false;
  });

  el.querySelector("#ctrl-mute-btn")?.addEventListener("click", async () => {
    try {
      const updated = await callCommand<AudioStatus>("toggle_system_mute");
      if (state.system) {
        state.system.audio = updated;
        syncTopBarSystemIndicators(state);
      }
      renderControlsPanel(state, onOpenPanel, onClose);
    } catch (e) {
      showToast(`Audio error: ${String(e)}`);
    }
  });
}
