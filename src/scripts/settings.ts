import { DEFAULT_CC_MODULES, getControlCenterModules } from "./controls";
import { callCommand, escapeHtml, formatBytes, ICONS, showToast } from "./ipc";
import type { IslandController } from "./island";
import type {
  AccentName,
  AppState,
  MonitorDescriptor,
  SettingsCategory,
  ThemeMode,
} from "./types";

let activeCategory: SettingsCategory = "appearance";

const ACCENTS: Array<{ id: AccentName; color: string; label: string }> = [
  { id: "silver", color: "#e5e7eb", label: "Silver" },
  { id: "blue", color: "#60a5fa", label: "Ice Blue" },
  { id: "emerald", color: "#34d399", label: "Emerald" },
  { id: "amber", color: "#fbbf24", label: "Amber" },
  { id: "rose", color: "#fb7185", label: "Rose" },
  { id: "violet", color: "#a78bfa", label: "Violet" },
];

const SIDEBAR_ITEMS: Array<{ id: SettingsCategory; label: string; icon: string }> = [
  { id: "general", label: "General", icon: ICONS.settings },
  { id: "appearance", label: "Appearance", icon: ICONS.palette },
  { id: "island", label: "Dynamic Island", icon: ICONS.island },
  { id: "controls", label: "Control Center", icon: ICONS.sliders },
  { id: "tasks", label: "Tasks", icon: ICONS.tasks },
  { id: "files", label: "Files", icon: ICONS.folder },
  { id: "notifications", label: "Notifications", icon: ICONS.bell },
  { id: "shortcuts", label: "Shortcuts", icon: ICONS.keyboard },
  { id: "performance", label: "Performance", icon: ICONS.zap },
  { id: "about", label: "About", icon: ICONS.info },
];

const CC_MODULE_LABELS: Record<string, string> = {
  status: "Quick Status & Battery",
  connectivity: "Connectivity (Wi-Fi, Bluetooth, Airplane)",
  display: "Display & Glass Theme",
  audio: "System Audio & Volume Slider",
  focus: "Focus Mode, DND & Quick Timer",
  apps: "Workspace Quick Controls",
};

export function applyVisualPreferences(settings: Record<string, string>): void {
  const root = document.documentElement;
  const themeSetting = (settings["theme"] || "dark") as ThemeMode;
  const resolvedTheme =
    themeSetting === "auto"
      ? window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : themeSetting;

  root.setAttribute("data-theme", resolvedTheme);
  root.setAttribute("data-accent", settings["accent_color"] || "silver");
  root.setAttribute("data-island-style", settings["island_style"] || "bar");

  const reduceMotion = settings["reduce_motion"] === "true";
  const motionIntensity = reduceMotion
    ? "reduced"
    : settings["animation_intensity"] || "normal";
  root.setAttribute("data-motion", motionIntensity);
  root.setAttribute("data-low-gpu", settings["low_gpu_fallback"] === "true" ? "true" : "false");

  const opacityPct = Number.parseInt(settings["glass_transparency"] || "24", 10);
  const blurPx = Number.parseInt(settings["blur_intensity"] || "22", 10);
  const radiusPx = Number.parseInt(settings["corner_roundness"] || "22", 10);
  const shadowPct = Number.parseInt(settings["shadow_intensity"] || "100", 10);
  const islandWidth = Number.parseInt(settings["island_width"] || "340", 10);
  const islandHeight = Number.parseInt(settings["island_height"] || "32", 10);
  const topOffset = Number.parseInt(settings["island_top_offset"] || "8", 10);

  if (Number.isFinite(opacityPct)) {
    root.style.setProperty("--glass-opacity", (Math.max(10, Math.min(92, opacityPct)) / 100).toFixed(2));
  }
  if (Number.isFinite(blurPx)) {
    root.style.setProperty("--glass-blur", `${Math.max(0, Math.min(60, blurPx))}px`);
  }
  if (Number.isFinite(radiusPx)) {
    const r = Math.max(10, Math.min(30, radiusPx));
    root.style.setProperty("--glass-radius", `${r}px`);
    root.style.setProperty("--glass-card-radius", `${Math.max(8, r - 8)}px`);
  }
  if (Number.isFinite(shadowPct)) {
    root.style.setProperty("--shadow-intensity", (Math.max(0, Math.min(150, shadowPct)) / 100).toFixed(2));
  }
  if (Number.isFinite(islandWidth)) {
    root.style.setProperty("--island-width", `${Math.max(260, Math.min(540, islandWidth))}px`);
  }
  if (Number.isFinite(islandHeight)) {
    root.style.setProperty("--island-height", `${Math.max(26, Math.min(44, islandHeight))}px`);
  }
  if (Number.isFinite(topOffset)) {
    root.style.setProperty("--island-top-offset", `${Math.max(0, Math.min(28, topOffset))}px`);
  }
}

export async function loadSettingsAndMonitors(state: AppState): Promise<void> {
  try {
    state.settings = await callCommand<Record<string, string>>("get_all_settings");
  } catch {
    state.settings = {
      enabled: "true",
      auto_hide_fullscreen: "true",
      auto_hide_idle: "true",
      theme: "dark",
      clock_format: "12h",
      clock_show_seconds: "false",
      accent_color: "silver",
      glass_transparency: "24",
      blur_intensity: "22",
      corner_roundness: "22",
      shadow_intensity: "100",
      animation_intensity: "normal",
      reduce_motion: "false",
      low_gpu_fallback: "false",
      island_style: "bar",
      island_width: "340",
      island_height: "32",
      island_top_offset: "8",
      island_horizontal_offset: "0",
      default_expanded_mode: "hub",
      hover_expand: "false",
      click_expand: "true",
      always_on_top: "true",
      collapsed_height: "36",
      monitor_index: "0",
      global_shortcut: "Alt+Space",
      shortcut_controls: "Alt+C",
      shortcut_tasks: "Alt+T",
      shortcut_files: "Alt+F",
      shortcut_settings: "Alt+,",
      vault_copy_by_default: "false",
      auto_collapse_after_drop: "true",
      control_center_modules: DEFAULT_CC_MODULES.join(","),
      notifications_enabled: "true",
      dnd_enabled: "false",
      default_task_view: "today",
      show_completed_tasks: "true",
      poll_interval_ms: "5000",
    };
  }

  state.dndMode = state.settings["dnd_enabled"] === "true";

  try {
    state.monitors = await callCommand<MonitorDescriptor[]>("list_monitors");
  } catch {
    state.monitors = [];
  }

  try {
    state.autostart = await callCommand<boolean>("get_autostart_enabled");
  } catch {
    state.autostart = false;
  }

  applyVisualPreferences(state.settings);
}

function toggleRow(key: string, label: string, desc: string, isOn: boolean): string {
  return `
    <div class="glass-card setting-row">
      <div>
        <div class="setting-label">${escapeHtml(label)}</div>
        <div class="setting-desc">${escapeHtml(desc)}</div>
      </div>
      <button class="gd-switch ${isOn ? "is-on" : ""}" data-toggle-key="${escapeHtml(key)}" type="button" aria-label="${escapeHtml(label)}"></button>
    </div>`;
}

function rangeRow(
  key: string,
  label: string,
  desc: string,
  val: number,
  min: number,
  max: number,
  unit: string
): string {
  return `
    <div class="glass-card setting-row">
      <div style="min-width: 170px;">
        <div class="setting-label">${escapeHtml(label)}</div>
        <div class="setting-desc">${escapeHtml(desc)}</div>
      </div>
      <div style="display:flex;align-items:center;gap:8px;min-width:170px;">
        <input class="gd-range" data-range-key="${escapeHtml(key)}" type="range" min="${min}" max="${max}" value="${val}" aria-label="${escapeHtml(label)}" />
        <span class="ctrl-sub" data-range-val="${escapeHtml(key)}" style="min-width:38px;text-align:right;font-variant-numeric:tabular-nums;">${val}${unit}</span>
      </div>
    </div>`;
}

export function renderSettingsPanel(
  state: AppState,
  islandCtrl: IslandController,
  onFilesCleared: () => Promise<void>,
  onNotificationsCleared?: () => Promise<void>
): void {
  const el = document.getElementById("panel-settings");
  if (!el) return;

  const s = state.settings;
  const bool = (k: string, fallback = false) =>
    s[k] === undefined ? fallback : s[k] === "true";
  const num = (k: string, fallback: number) => {
    const p = Number.parseInt(s[k] ?? "", 10);
    return Number.isFinite(p) ? p : fallback;
  };

  const renderCategoryBody = (): string => {
    switch (activeCategory) {
      case "general": {
        const monitorIdx = s["monitor_index"] || "0";
        return `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Start with Windows</div>
              <div class="setting-desc">Launch GLASSDESK automatically on sign-in</div>
            </div>
            <button class="gd-switch ${state.autostart ? "is-on" : ""}" id="sw-autostart" type="button" aria-label="Toggle Start with Windows"></button>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Display Monitor</div>
              <div class="setting-desc">Choose which screen hosts GLASSDESK</div>
            </div>
            <select class="gd-select" data-select-key="monitor_index" id="sel-monitor" aria-label="Monitor">
              ${
                state.monitors.length === 0
                  ? `<option value="0">Primary Monitor</option>`
                  : state.monitors
                      .map(
                        (m) => `
                      <option value="${m.index}" ${String(m.index) === monitorIdx ? "selected" : ""}>
                        ${escapeHtml(m.isPrimary ? `Primary (${m.width}×${m.height})` : `Display ${m.index + 1} (${m.width}×${m.height})`)}
                      </option>`
                      )
                      .join("")
              }
            </select>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Clock Format</div>
              <div class="setting-desc">12-hour or 24-hour display across Island and Control Center</div>
            </div>
            <select class="gd-select" data-select-key="clock_format" aria-label="Clock format">
              <option value="12h" ${(s["clock_format"] || "12h") === "12h" ? "selected" : ""}>12-Hour</option>
              <option value="24h" ${s["clock_format"] === "24h" ? "selected" : ""}>24-Hour</option>
            </select>
          </div>

          ${toggleRow("clock_show_seconds", "Show Seconds on Clock", "Display live seconds in the Dynamic Island pill", bool("clock_show_seconds", false))}
          ${toggleRow("auto_hide_fullscreen", "Auto-Hide on Full-Screen Apps", "Pause overlay during fullscreen games and videos", bool("auto_hide_fullscreen", true))}

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Hide Overlay Now</div>
              <div class="setting-desc">Restore anytime from the system tray or global shortcut</div>
            </div>
            <button class="gd-btn" id="btn-hide-overlay" type="button">Hide</button>
          </div>`;
      }

      case "appearance": {
        const theme = s["theme"] || "dark";
        const accent = s["accent_color"] || "silver";
        const motion = s["animation_intensity"] || "normal";
        return `
          <div class="settings-live-preview">
            <div class="preview-pill glass-primary">
              <span class="gd-island-indicator"></span>
              <span>Live Liquid Glass Preview</span>
            </div>
            <div class="preview-card glass-panel">
              <div>
                <div class="setting-label">Layered Glass Surface</div>
                <div class="setting-desc">Transparency, blur, radius & shadows update live</div>
              </div>
              <button class="gd-btn gd-btn-primary" type="button">Accent</button>
            </div>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Glass Theme</div>
              <div class="setting-desc">Dark glass, light glass, or follow Windows theme</div>
            </div>
            <select class="gd-select" data-select-key="theme" aria-label="Theme">
              <option value="dark" ${theme === "dark" ? "selected" : ""}>Dark Glass</option>
              <option value="light" ${theme === "light" ? "selected" : ""}>Light Glass</option>
              <option value="auto" ${theme === "auto" ? "selected" : ""}>Automatic</option>
            </select>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Accent Colour</div>
              <div class="setting-desc">Highlights, active controls, and focus rings</div>
            </div>
            <div class="swatches">
              ${ACCENTS.map(
                (a) => `
                <button class="swatch ${accent === a.id ? "is-active" : ""}" style="background: ${a.color};" title="${a.label}" data-accent-pick="${a.id}" type="button"></button>`
              ).join("")}
            </div>
          </div>

          ${rangeRow("glass_transparency", "Glass Opacity", "Balance contrast over bright or dark wallpapers", num("glass_transparency", 24), 10, 90, "%")}
          ${rangeRow("blur_intensity", "Blur Intensity", "Backdrop frosted blur radius", num("blur_intensity", 22), 0, 56, "px")}
          ${rangeRow("corner_roundness", "Corner Roundness", "Concentric panel & card radius", num("corner_roundness", 22), 12, 28, "px")}
          ${rangeRow("shadow_intensity", "Panel Shadow Intensity", "Ambient depth separation from desktop", num("shadow_intensity", 100), 20, 140, "%")}

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Animation Intensity</div>
              <div class="setting-desc">Spring expansion speed and motion feel</div>
            </div>
            <select class="gd-select" data-select-key="animation_intensity" aria-label="Animation intensity">
              <option value="subtle" ${motion === "subtle" ? "selected" : ""}>Subtle</option>
              <option value="normal" ${motion === "normal" ? "selected" : ""}>Balanced</option>
              <option value="expressive" ${motion === "expressive" ? "selected" : ""}>Expressive</option>
            </select>
          </div>

          ${toggleRow("reduce_motion", "Reduce Motion", "Minimize transitions and spring physics", bool("reduce_motion", false))}`;
      }

      case "island": {
        const style = s["island_style"] || "bar";
        const defMode = s["default_expanded_mode"] || "hub";
        return `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Shell Style</div>
              <div class="setting-desc">Pure floating Dynamic Island pill (click-through) or full Top Menu Bar</div>
            </div>
            <select class="gd-select" data-select-key="island_style" aria-label="Shell style">
              <option value="floating" ${style === "floating" ? "selected" : ""}>Floating Dynamic Island</option>
              <option value="bar" ${style === "bar" ? "selected" : ""}>Top Menu Bar + Island</option>
            </select>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Default Expanded Mode</div>
              <div class="setting-desc">Panel opened when clicking or hovering the island</div>
            </div>
            <select class="gd-select" data-select-key="default_expanded_mode" aria-label="Default expanded mode">
              <option value="hub" ${defMode === "hub" ? "selected" : ""}>Overview Hub</option>
              <option value="calendar" ${defMode === "calendar" ? "selected" : ""}>Time & Calendar</option>
              <option value="tasks" ${defMode === "tasks" ? "selected" : ""}>Tasks</option>
              <option value="files" ${defMode === "files" ? "selected" : ""}>Files</option>
              <option value="controls" ${defMode === "controls" ? "selected" : ""}>Control Center</option>
              <option value="notifications" ${defMode === "notifications" ? "selected" : ""}>Notifications</option>
            </select>
          </div>

          ${rangeRow("island_top_offset", "Top Alignment Offset", "Distance from top edge of monitor", num("island_top_offset", 8), 0, 24, "px")}
          ${rangeRow("island_horizontal_offset", "Horizontal Position Offset", "Shift island left or right from top center", num("island_horizontal_offset", 0), -300, 300, "px")}
          ${rangeRow("island_width", "Island Width", "Minimum width of the compact capsule", num("island_width", 340), 260, 520, "px")}
          ${rangeRow("island_height", "Island Height", "Height of the compact capsule", num("island_height", 32), 26, 42, "px")}
          ${toggleRow("auto_hide_idle", "Auto-Hide When Idle", "Tuck Dynamic Island into the top edge and reveal on hover", bool("auto_hide_idle", true))}
          ${toggleRow("hover_expand", "Hover Expansion", "Smoothly expand the island when hovering briefly", bool("hover_expand", false))}
          ${toggleRow("click_expand", "Click Expansion", "Expand the island on click", bool("click_expand", true))}
          ${toggleRow("always_on_top", "Always on Top", "Keep Dynamic Island floating above other windows", bool("always_on_top", true))}`;
      }

      case "controls": {
        const activeModules = getControlCenterModules(s);
        return `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Control Center Modules</div>
              <div class="setting-desc">Enable, disable, or reorder sections shown in Control Center</div>
            </div>
            <button class="gd-btn" id="btn-reset-cc" type="button">Restore Controls</button>
          </div>

          ${DEFAULT_CC_MODULES.map((mod) => {
            const idx = activeModules.indexOf(mod);
            const enabled = idx !== -1;
            return `
              <div class="glass-card setting-row">
                <div>
                  <div class="setting-label">${escapeHtml(CC_MODULE_LABELS[mod] || mod)}</div>
                  <div class="setting-desc">${enabled ? `Position #${idx + 1}` : "Hidden"}</div>
                </div>
                <div style="display:flex;align-items:center;gap:5px;">
                  ${
                    enabled
                      ? `<button class="gd-btn" data-cc-move="${mod}:-1" ${idx === 0 ? "disabled" : ""} type="button" title="Move Up">↑</button>
                         <button class="gd-btn" data-cc-move="${mod}:1" ${idx === activeModules.length - 1 ? "disabled" : ""} type="button" title="Move Down">↓</button>`
                      : ""
                  }
                  <button class="gd-switch ${enabled ? "is-on" : ""}" data-cc-toggle="${mod}" type="button" aria-label="Toggle ${escapeHtml(mod)}"></button>
                </div>
              </div>`;
          }).join("")}`;
      }

      case "tasks": {
        const defView = s["default_task_view"] || "today";
        return `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Default Task View</div>
              <div class="setting-desc">Initial filter shown when opening Tasks</div>
            </div>
            <select class="gd-select" data-select-key="default_task_view" aria-label="Default task view">
              <option value="today" ${defView === "today" ? "selected" : ""}>Today</option>
              <option value="upcoming" ${defView === "upcoming" ? "selected" : ""}>Upcoming</option>
              <option value="overdue" ${defView === "overdue" ? "selected" : ""}>Overdue</option>
              <option value="completed" ${defView === "completed" ? "selected" : ""}>Completed</option>
            </select>
          </div>
          ${toggleRow("show_completed_tasks", "Show Completed Tab", "Allow reviewing completed tasks in the task manager", bool("show_completed_tasks", true))}
          ${toggleRow("notifications_enabled", "Task & Reminder Alerts", "Deliver glass notifications when reminders become due", bool("notifications_enabled", true))}`;
      }

      case "files": {
        return `
          ${toggleRow("auto_collapse_after_drop", "Auto-Collapse After File Drop", "Return to compact island after holding dropped files", bool("auto_collapse_after_drop", true))}
          ${toggleRow("vault_copy_by_default", "Copy Dropped Files to Vault", "Store independent copies automatically in app data vault", bool("vault_copy_by_default", false))}
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Clear Held File References</div>
              <div class="setting-desc">Clears shelf list only — original files on disk are never deleted</div>
            </div>
            <button class="gd-btn gd-btn-danger" id="btn-settings-clear-files" type="button">Clear All</button>
          </div>`;
      }

      case "notifications": {
        return `
          ${toggleRow("notifications_enabled", "Enable Notifications", "Show reminders, timer alerts, and file-drop confirmations", bool("notifications_enabled", true))}
          ${toggleRow("dnd_enabled", "Do Not Disturb", "Silence toast popups while keeping notifications in history", bool("dnd_enabled", false))}
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Notification History</div>
              <div class="setting-desc">Clear all stored notification cards</div>
            </div>
            <button class="gd-btn gd-btn-danger" id="btn-settings-clear-notifs" type="button">Clear History</button>
          </div>`;
      }

      case "shortcuts": {
        const shortcuts: Array<{ key: string; label: string; desc: string; def: string }> = [
          { key: "global_shortcut", label: "Toggle / Show Dynamic Island", desc: "Global Windows shortcut to show or hide GLASSDESK", def: "Alt+Space" },
          { key: "shortcut_controls", label: "Open Control Center", desc: "Jump directly to macOS-inspired Control Center", def: "Alt+C" },
          { key: "shortcut_tasks", label: "Open Tasks", desc: "Jump directly to today's tasks and reminders", def: "Alt+T" },
          { key: "shortcut_files", label: "Open File Drop Shelf", desc: "Open held files and drag-out shelf", def: "Alt+F" },
          { key: "shortcut_settings", label: "Open Settings", desc: "Open GLASSDESK System Settings", def: "Alt+," },
        ];
        return shortcuts
          .map(
            (sc) => `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">${escapeHtml(sc.label)}</div>
              <div class="setting-desc">${escapeHtml(sc.desc)}</div>
            </div>
            <div style="display:flex;gap:5px;">
              <input class="gd-input" data-shortcut-input="${sc.key}" style="width:110px;" value="${escapeHtml(s[sc.key] || sc.def)}" />
              <button class="gd-btn" data-shortcut-save="${sc.key}" type="button">Save</button>
            </div>
          </div>`
          )
          .join("");
      }

      case "performance": {
        const pollMs = s["poll_interval_ms"] || "5000";
        return `
          ${toggleRow("low_gpu_fallback", "Low-GPU Fallback Appearance", "Disable backdrop blur for older GPUs or remote desktop sessions", bool("low_gpu_fallback", false))}
          ${toggleRow("reduce_motion", "Reduce Motion", "Disable spring physics loop for minimal CPU/GPU usage", bool("reduce_motion", false))}

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">System Polling Interval</div>
              <div class="setting-desc">How often battery, Wi-Fi, audio, and media status refresh</div>
            </div>
            <select class="gd-select" data-select-key="poll_interval_ms" aria-label="Polling interval">
              <option value="2000" ${pollMs === "2000" ? "selected" : ""}>2 seconds (Responsive)</option>
              <option value="5000" ${pollMs === "5000" ? "selected" : ""}>5 seconds (Balanced)</option>
              <option value="10000" ${pollMs === "10000" ? "selected" : ""}>10 seconds (Battery Saver)</option>
            </select>
          </div>

          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Optimize Storage & Trim Memory</div>
              <div class="setting-desc">Compact SQLite DB, prune orphan vault copies, and trim Windows RAM working set</div>
            </div>
            <button class="gd-btn" id="btn-optimize-storage" type="button">Optimize Now</button>
          </div>`;
      }

      case "about": {
        return `
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">GLASSDESK for Windows 11</div>
              <div class="setting-desc">Version 2.0.0 · Rust + Tauri 2 + SQLite · Liquid Glass Desktop Companion</div>
            </div>
            <span class="tag-chip">v2.0.0</span>
          </div>
          <div class="glass-card setting-row">
            <div>
              <div class="setting-label">Restore All Factory Defaults</div>
              <div class="setting-desc">Reset appearance, island layout, control center, and shortcuts</div>
            </div>
            <button class="gd-btn gd-btn-danger" id="btn-restore-defaults" type="button">Restore Defaults</button>
          </div>`;
      }
    }
  };

  const activeMeta = SIDEBAR_ITEMS.find((i) => i.id === activeCategory) ?? SIDEBAR_ITEMS[0];

  el.innerHTML = `
    <div class="settings-shell">
      <aside class="settings-sidebar">
        <div class="settings-sidebar-header panel-header gd-drag-handle" style="border-bottom:1px solid var(--divider);">
          <div class="mac-lights">
            <button class="mac-dot mac-close" data-mac-close type="button" aria-label="Close Settings"></button>
            <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
          </div>
          <span class="panel-title" style="font-size:12.5px;">Settings</span>
        </div>
        ${SIDEBAR_ITEMS.map(
          (item) => `
          <button class="settings-nav-item ${activeCategory === item.id ? "is-active" : ""}" data-settings-cat="${item.id}" type="button">
            <span class="gd-icon">${item.icon}</span>
            <span>${escapeHtml(item.label)}</span>
          </button>`
        ).join("")}
      </aside>

      <div class="settings-content">
        <div class="settings-content-header panel-header gd-drag-handle">
          <div class="panel-title-group">
            <span class="gd-icon" style="color:var(--accent);">${activeMeta.icon}</span>
            <span class="panel-title">${escapeHtml(activeMeta.label)}</span>
          </div>
          <div class="panel-actions">
            <button class="gd-btn" id="btn-close-settings" type="button">Done</button>
          </div>
        </div>
        <div class="settings-content-body">
          ${renderCategoryBody()}
        </div>
      </div>
    </div>
  `;

  const rerender = () =>
    renderSettingsPanel(state, islandCtrl, onFilesCleared, onNotificationsCleared);

  const saveKey = async (key: string, value: string) => {
    state.settings[key] = value;
    if (key === "dnd_enabled") {
      state.dndMode = value === "true";
    }
    try {
      await callCommand("set_setting", { key, value });
    } catch {
      // Offline preview
    }
    applyVisualPreferences(state.settings);
    if (key === "clock_format" || key === "clock_show_seconds") {
      window.dispatchEvent(new Event("glassdesk:clock-sync"));
    }
    if (key === "auto_hide_idle") {
      window.dispatchEvent(new Event("glassdesk:autohide-sync"));
    }
  };

  el.querySelectorAll<HTMLElement>("[data-mac-close], #btn-close-settings").forEach((btn) => {
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      void islandCtrl.collapse();
    });
  });

  el.querySelectorAll<HTMLElement>("[data-settings-cat]").forEach((btn) => {
    btn.addEventListener("click", () => {
      activeCategory = btn.dataset.settingsCat as SettingsCategory;
      rerender();
    });
  });

  el.querySelector("#btn-hide-overlay")?.addEventListener("click", async () => {
    await islandCtrl.collapse();
    await callCommand("set_overlay_visible", { visible: false }).catch(() => {});
  });

  el.querySelector("#sw-autostart")?.addEventListener("click", async () => {
    try {
      state.autostart = await callCommand<boolean>("set_autostart_enabled", {
        enabled: !state.autostart,
      });
      showToast(state.autostart ? "Start with Windows enabled" : "Start with Windows disabled");
      rerender();
    } catch (e) {
      showToast(`Autostart error: ${String(e)}`);
    }
  });

  el.querySelectorAll<HTMLElement>("[data-toggle-key]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const key = btn.dataset.toggleKey;
      if (!key) return;
      const next = !btn.classList.contains("is-on");
      await saveKey(key, next ? "true" : "false");
      rerender();
    });
  });

  el.querySelectorAll<HTMLSelectElement>("[data-select-key]").forEach((sel) => {
    sel.addEventListener("change", async () => {
      const key = sel.dataset.selectKey;
      if (!key) return;
      await saveKey(key, sel.value);
      if (key === "monitor_index") {
        await islandCtrl.setOverlayHeight(600, true, true);
        showToast("Moved GLASSDESK to selected monitor");
      }
    });
  });

  el.querySelectorAll<HTMLInputElement>("[data-range-key]").forEach((inp) => {
    inp.addEventListener("input", () => {
      const key = inp.dataset.rangeKey;
      if (!key) return;
      const valSpan = el.querySelector<HTMLElement>(`[data-range-val="${key}"]`);
      if (valSpan) {
        const unit = valSpan.textContent?.replace(/[-\d]/g, "") ?? "";
        valSpan.textContent = `${inp.value}${unit}`;
      }
      state.settings[key] = inp.value;
      applyVisualPreferences(state.settings);
    });
    inp.addEventListener("change", async () => {
      const key = inp.dataset.rangeKey;
      if (!key) return;
      await saveKey(key, inp.value);
    });
  });

  el.querySelectorAll<HTMLElement>("[data-accent-pick]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const pick = btn.dataset.accentPick;
      if (!pick) return;
      await saveKey("accent_color", pick);
      rerender();
    });
  });

  // Control Center module toggles & reordering
  el.querySelectorAll<HTMLElement>("[data-cc-toggle]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const mod = btn.dataset.ccToggle;
      if (!mod) return;
      const mods = getControlCenterModules(state.settings);
      const next = mods.includes(mod) ? mods.filter((m) => m !== mod) : [...mods, mod];
      if (next.length === 0) {
        showToast("Keep at least one Control Center module enabled");
        return;
      }
      await saveKey("control_center_modules", next.join(","));
      rerender();
    });
  });

  el.querySelectorAll<HTMLElement>("[data-cc-move]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const [mod, deltaStr] = (btn.dataset.ccMove || "").split(":");
      const delta = Number.parseInt(deltaStr || "0", 10);
      const mods = getControlCenterModules(state.settings);
      const idx = mods.indexOf(mod);
      const nextIdx = idx + delta;
      if (idx < 0 || nextIdx < 0 || nextIdx >= mods.length) return;
      [mods[idx], mods[nextIdx]] = [mods[nextIdx], mods[idx]];
      await saveKey("control_center_modules", mods.join(","));
      rerender();
    });
  });

  el.querySelector("#btn-reset-cc")?.addEventListener("click", async () => {
    await saveKey("control_center_modules", DEFAULT_CC_MODULES.join(","));
    showToast("Restored default Control Center modules");
    rerender();
  });

  // Shortcuts persistence & registration
  el.querySelectorAll<HTMLElement>("[data-shortcut-save]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const key = btn.dataset.shortcutSave;
      if (!key) return;
      const inp = el.querySelector<HTMLInputElement>(`[data-shortcut-input="${key}"]`);
      const val = inp?.value.trim() ?? "";
      if (!val) return;
      try {
        if (key === "global_shortcut") {
          await callCommand("update_global_shortcut", { shortcut: val });
          state.settings[key] = val;
        } else {
          await callCommand("update_action_shortcut", { actionKey: key, shortcut: val });
          state.settings[key] = val;
        }
        showToast(`Shortcut saved: ${val}`);
      } catch (e) {
        showToast(`Shortcut failed: ${String(e)}`);
      }
    });
  });

  el.querySelector("#btn-optimize-storage")?.addEventListener("click", async () => {
    try {
      const report = await callCommand<{
        removedOrphanVaultFiles: number;
        reclaimedBytes: number;
        dbSizeBytes: number;
      }>("optimize_storage");
      showToast(
        `Optimized: DB ${formatBytes(report.dbSizeBytes)}${
          report.removedOrphanVaultFiles > 0
            ? ` · pruned ${report.removedOrphanVaultFiles} orphan(s)`
            : ""
        }`
      );
    } catch (e) {
      showToast(`Optimization failed: ${String(e)}`);
    }
  });

  el.querySelector("#btn-settings-clear-files")?.addEventListener("click", async () => {
    await callCommand("clear_held_files").catch(() => {});
    showToast("Cleared all held file references");
    await onFilesCleared();
  });

  el.querySelector("#btn-settings-clear-notifs")?.addEventListener("click", async () => {
    await callCommand("clear_notifications").catch(() => {});
    state.notifications = [];
    showToast("Cleared notification history");
    if (onNotificationsCleared) await onNotificationsCleared();
  });

  el.querySelector("#btn-restore-defaults")?.addEventListener("click", async () => {
    try {
      state.settings = await callCommand<Record<string, string>>("restore_default_settings");
      state.dndMode = state.settings["dnd_enabled"] === "true";
      applyVisualPreferences(state.settings);
      showToast("Restored default settings");
      rerender();
    } catch (e) {
      showToast(`Reset failed: ${String(e)}`);
    }
  });
}
