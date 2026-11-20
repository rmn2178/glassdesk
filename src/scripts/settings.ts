import { callCommand, escapeHtml, showToast } from "./ipc";
import type { IslandController } from "./island";
import type { AccentName, AppState, MonitorDescriptor, ThemeMode } from "./types";

const ACCENTS: Array<{ id: AccentName; color: string; label: string }> = [
  { id: "silver", color: "#e5e7eb", label: "Silver" },
  { id: "blue", color: "#60a5fa", label: "Ice Blue" },
  { id: "emerald", color: "#34d399", label: "Emerald" },
  { id: "amber", color: "#fbbf24", label: "Amber" },
  { id: "rose", color: "#fb7185", label: "Rose" },
  { id: "violet", color: "#a78bfa", label: "Violet" },
];

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
}

export async function loadSettingsAndMonitors(state: AppState): Promise<void> {
  try {
    state.settings = await callCommand<Record<string, string>>("get_all_settings");
  } catch {
    state.settings = {
      enabled: "true",
      auto_hide_fullscreen: "true",
      theme: "dark",
      clock_format: "12h",
      accent_color: "silver",
      monitor_index: "0",
      global_shortcut: "Alt+Space",
      auto_collapse_after_drop: "true",
      vault_copy_by_default: "false",
    };
  }

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

export function renderSettingsPanel(
  state: AppState,
  islandCtrl: IslandController,
  onFilesCleared: () => Promise<void>
): void {
  const el = document.getElementById("panel-settings");
  if (!el) return;

  const s = state.settings;
  const theme = s["theme"] || "dark";
  const clockFormat = s["clock_format"] || "12h";
  const accent = s["accent_color"] || "silver";
  const autoHide = s["auto_hide_fullscreen"] !== "false";
  const autoCollapse = s["auto_collapse_after_drop"] !== "false";
  const vaultDefault = s["vault_copy_by_default"] === "true";
  const shortcut = s["global_shortcut"] || "Alt+Space";
  const monitorIdx = s["monitor_index"] || "0";

  el.innerHTML = `
    <div class="panel-header">
      <div class="panel-title-group">
        <span class="panel-title">GLASSDESK Settings</span>
      </div>
      <div class="panel-actions">
        <button class="gd-btn" id="btn-restore-defaults" type="button">Restore Defaults</button>
      </div>
    </div>

    <div class="settings-list">
      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">GLASSDESK Overlay</div>
          <div class="setting-desc">Hide now (use system tray or shortcut to restore)</div>
        </div>
        <button class="gd-btn" id="btn-hide-overlay" type="button">Hide</button>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Start with Windows</div>
          <div class="setting-desc">Launch GLASSDESK automatically on sign-in</div>
        </div>
        <button class="gd-switch ${state.autostart ? "is-on" : ""}" id="sw-autostart" type="button" aria-label="Toggle Start with Windows"></button>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Hide on Full-Screen Apps</div>
          <div class="setting-desc">Auto-hide during games & fullscreen videos</div>
        </div>
        <button class="gd-switch ${autoHide ? "is-on" : ""}" data-toggle-key="auto_hide_fullscreen" type="button" aria-label="Toggle Hide on Fullscreen"></button>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Theme</div>
          <div class="setting-desc">Adapts to dark or light desktop wallpaper</div>
        </div>
        <select class="gd-select" id="sel-theme" aria-label="Theme">
          <option value="dark" ${theme === "dark" ? "selected" : ""}>Dark</option>
          <option value="light" ${theme === "light" ? "selected" : ""}>Light</option>
          <option value="auto" ${theme === "auto" ? "selected" : ""}>Automatic</option>
        </select>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Clock Format</div>
          <div class="setting-desc">12-hour or 24-hour display</div>
        </div>
        <select class="gd-select" id="sel-clock" aria-label="Clock format">
          <option value="12h" ${clockFormat === "12h" ? "selected" : ""}>12-Hour</option>
          <option value="24h" ${clockFormat === "24h" ? "selected" : ""}>24-Hour</option>
        </select>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Accent Colour</div>
          <div class="setting-desc">Highlights and active pills</div>
        </div>
        <div class="swatches">
          ${ACCENTS.map(
            (a) => `
            <button
              class="swatch ${accent === a.id ? "is-active" : ""}"
              style="background: ${a.color};"
              title="${a.label}"
              data-accent-pick="${a.id}"
              type="button"
            ></button>`
          ).join("")}
        </div>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Display Monitor</div>
          <div class="setting-desc">Choose which screen hosts GLASSDESK</div>
        </div>
        <select class="gd-select" id="sel-monitor" aria-label="Monitor">
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
          <div class="setting-label">Toggle Shortcut</div>
          <div class="setting-desc">Show or hide GLASSDESK anytime</div>
        </div>
        <div style="display: flex; gap: 5px;">
          <input id="inp-shortcut" class="gd-input" style="width: 105px;" value="${escapeHtml(shortcut)}" />
          <button class="gd-btn" id="btn-save-shortcut" type="button">Save</button>
        </div>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Auto-Collapse After File Drop</div>
          <div class="setting-desc">Return to slim bar after holding files</div>
        </div>
        <button class="gd-switch ${autoCollapse ? "is-on" : ""}" data-toggle-key="auto_collapse_after_drop" type="button" aria-label="Toggle Auto-Collapse"></button>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">Copy Dropped Files to Vault</div>
          <div class="setting-desc">Keep independent copies automatically</div>
        </div>
        <button class="gd-switch ${vaultDefault ? "is-on" : ""}" data-toggle-key="vault_copy_by_default" type="button" aria-label="Toggle Vault Default"></button>
      </div>

      <div class="glass-card setting-row">
        <div>
          <div class="setting-label">FILE HOLD References</div>
          <div class="setting-desc">Clear all references (original files stay safe)</div>
        </div>
        <button class="gd-btn gd-btn-danger" id="btn-settings-clear-files" type="button">Clear All</button>
      </div>
    </div>
  `;

  const saveKey = async (key: string, value: string) => {
    state.settings[key] = value;
    try {
      await callCommand("set_setting", { key, value });
    } catch {
      // Offline preview
    }
    applyVisualPreferences(state.settings);
  };

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
      renderSettingsPanel(state, islandCtrl, onFilesCleared);
    } catch (e) {
      showToast(`Autostart error: ${String(e)}`);
    }
  });

  el.querySelectorAll<HTMLElement>("[data-toggle-key]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const key = btn.dataset.toggleKey;
      if (!key) return;
      const current = state.settings[key] === "true" || (key !== "vault_copy_by_default" && state.settings[key] !== "false");
      await saveKey(key, current ? "false" : "true");
      renderSettingsPanel(state, islandCtrl, onFilesCleared);
    });
  });

  el.querySelector<HTMLSelectElement>("#sel-theme")?.addEventListener("change", async (e) => {
    await saveKey("theme", (e.target as HTMLSelectElement).value);
  });

  el.querySelector<HTMLSelectElement>("#sel-clock")?.addEventListener("change", async (e) => {
    await saveKey("clock_format", (e.target as HTMLSelectElement).value);
  });

  el.querySelectorAll<HTMLElement>("[data-accent-pick]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const pick = btn.dataset.accentPick;
      if (!pick) return;
      await saveKey("accent_color", pick);
      renderSettingsPanel(state, islandCtrl, onFilesCleared);
    });
  });

  el.querySelector<HTMLSelectElement>("#sel-monitor")?.addEventListener("change", async (e) => {
    const val = (e.target as HTMLSelectElement).value;
    await saveKey("monitor_index", val);
    await islandCtrl.setOverlayHeight(495, true);
    showToast("Moved GLASSDESK to selected monitor");
  });

  el.querySelector("#btn-save-shortcut")?.addEventListener("click", async () => {
    const inp = el.querySelector<HTMLInputElement>("#inp-shortcut");
    const val = inp?.value.trim() ?? "";
    if (!val) return;
    try {
      await callCommand("update_global_shortcut", { shortcut: val });
      state.settings["global_shortcut"] = val;
      showToast(`Shortcut updated to ${val}`);
    } catch (e) {
      showToast(`Shortcut failed: ${String(e)}`);
    }
  });

  el.querySelector("#btn-settings-clear-files")?.addEventListener("click", async () => {
    await callCommand("clear_held_files");
    showToast("Cleared all held file references");
    await onFilesCleared();
  });

  el.querySelector("#btn-restore-defaults")?.addEventListener("click", async () => {
    try {
      state.settings = await callCommand<Record<string, string>>("restore_default_settings");
      applyVisualPreferences(state.settings);
      showToast("Restored default settings");
      renderSettingsPanel(state, islandCtrl, onFilesCleared);
    } catch (e) {
      showToast(`Reset failed: ${String(e)}`);
    }
  });
}
