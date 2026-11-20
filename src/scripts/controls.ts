import { callCommand, escapeHtml, ICONS, isTauriRuntime, showToast } from "./ipc";
import type { AppState, AudioStatus, SystemSnapshot } from "./types";

let hiddenByFullscreen = false;
let manualRestoreGraceUntil = 0;

// Throttled volume slider state to avoid flooding Windows Core Audio COM
let isDraggingVolume = false;
let lastVolSentAt = 0;
let lastVolSent: number | null = null;
let pendingVol: number | null = null;
let volTimer: number | null = null;
const VOL_THROTTLE_MS = 50;

export function resetFullscreenAutoHide(graceMs = 3000): void {
  hiddenByFullscreen = false;
  manualRestoreGraceUntil = Date.now() + graceMs;
}

export function syncTopBarSystemIndicators(state: AppState): void {
  const sys = state.system;
  if (!sys) return;

  // Active foreground app (ignore GLASSDESK itself so previous user app stays displayed)
  const appNameEl = document.getElementById("active-app-name");
  if (appNameEl && !sys.activeWindow.isSelf) {
    const label = sys.activeWindow.executable || sys.activeWindow.title;
    if (label) {
      appNameEl.textContent = label;
      appNameEl.title = sys.activeWindow.title || label;
    }
  }

  // Keyboard layout
  const kbdEl = document.getElementById("sys-kbd-code");
  if (kbdEl) kbdEl.textContent = sys.keyboardLayout || "ENG";

  // Battery
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

  // Volume
  const volIcon = document.getElementById("sys-volume-icon");
  if (volIcon) {
    volIcon.innerHTML =
      sys.audio.muted || sys.audio.volumePercent === 0 ? ICONS.volMuted : ICONS.volHigh;
  }

  // Sync open volume slider only if user is not actively dragging
  if (!isDraggingVolume && volTimer === null) {
    const slider = document.getElementById("ctrl-vol-slider") as HTMLInputElement | null;
    const readout = document.getElementById("ctrl-vol-readout");
    if (slider && readout) {
      slider.value = String(sys.audio.volumePercent);
      readout.textContent = sys.audio.muted ? "Muted" : `${sys.audio.volumePercent}%`;
    }
  }

  // Network / Wi-Fi
  const wifiIcon = document.getElementById("sys-wifi-icon");
  if (wifiIcon) {
    wifiIcon.innerHTML = sys.network.connected ? ICONS.wifiOn : ICONS.wifiOff;
  }
}

export async function refreshSystemSnapshot(state: AppState): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const snap = await callCommand<SystemSnapshot>("get_system_snapshot");
    state.system = snap;
    syncTopBarSystemIndicators(state);

    // Optional auto-hide on fullscreen games/apps (respecting manual restore grace window)
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
  void refreshSystemSnapshot(state);
  window.setInterval(() => {
    void refreshSystemSnapshot(state);
  }, 3000);
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

export function renderControlsPanel(state: AppState): void {
  const el = document.getElementById("panel-controls");
  if (!el) return;

  const sys = state.system;
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
  const kbd = sys?.keyboardLayout || "ENG";

  el.innerHTML = `
    <div class="panel-header">
      <div class="panel-title-group">
        <span class="panel-title">Quick Control Centre</span>
      </div>
      <div class="panel-actions">
        <button class="gd-btn" data-ms-settings="ms-settings:" type="button">Windows Settings</button>
      </div>
    </div>

    <div class="controls-grid">
      <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:network-wifi" type="button">
        <div class="ctrl-icon-box">${sys?.network.connected ? ICONS.wifiOn : ICONS.wifiOff}</div>
        <div>
          <div class="ctrl-label">Wi-Fi & Network</div>
          <div class="ctrl-sub">${escapeHtml(netLabel)}</div>
        </div>
      </button>

      <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:bluetooth" type="button">
        <div class="ctrl-icon-box">${ICONS.bluetooth}</div>
        <div>
          <div class="ctrl-label">Bluetooth</div>
          <div class="ctrl-sub">Open Settings</div>
        </div>
      </button>

      <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:batterysaver" type="button">
        <div class="ctrl-icon-box">${sys?.battery.isCharging ? ICONS.batteryCharging : ICONS.battery}</div>
        <div>
          <div class="ctrl-label">Battery</div>
          <div class="ctrl-sub">${escapeHtml(batLabel)}</div>
        </div>
      </button>

      <button class="glass-card ctrl-tile" data-ms-settings="ms-settings:regionlanguage" type="button">
        <div class="ctrl-icon-box" style="font-weight: 700; font-size: 11px;">${escapeHtml(kbd)}</div>
        <div>
          <div class="ctrl-label">Keyboard</div>
          <div class="ctrl-sub">Language Settings</div>
        </div>
      </button>
    </div>

    <div class="glass-card volume-box">
      <div style="display: flex; justify-content: space-between; align-items: center;">
        <span class="ctrl-label">System Volume</span>
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
        <button class="gd-btn" data-ms-settings="ms-settings:sound" type="button">Mixer</button>
      </div>
    </div>
  `;

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
      renderControlsPanel(state);
    } catch (e) {
      showToast(`Audio error: ${String(e)}`);
    }
  });
}
