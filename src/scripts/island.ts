import { resetFilesPanelTransientState } from "./files";
import { callCommand, escapeHtml } from "./ipc";
import type { AppState, PanelName } from "./types";

const PANEL_HEIGHTS: Record<PanelName, number> = {
  hub: 320,
  files: 490,
  calendar: 455,
  tasks: 470,
  controls: 375,
  settings: 495,
};

const RIGHT_ALIGNED_PANELS = new Set<PanelName>(["calendar", "controls", "settings"]);

export class IslandController {
  private state: AppState;
  private onRenderPanel: (panel: PanelName) => void;
  private collapseTimer: number | null = null;

  constructor(state: AppState, onRenderPanel: (panel: PanelName) => void) {
    this.state = state;
    this.onRenderPanel = onRenderPanel;
    this.bindGlobalDismissal();
  }

  private getMonitorIndex(): number | null {
    const raw = this.state.settings["monitor_index"];
    if (raw === undefined) return null;
    const parsed = Number.parseInt(raw, 10);
    return Number.isFinite(parsed) ? parsed : null;
  }

  private getCollapsedHeight(): number {
    const raw = this.state.settings["collapsed_height"];
    const parsed = raw ? Number.parseInt(raw, 10) : 36;
    return Number.isFinite(parsed) ? Math.max(32, Math.min(44, parsed)) : 36;
  }

  public async setOverlayHeight(height: number, focusable: boolean): Promise<void> {
    try {
      await callCommand("set_overlay_bounds", {
        height: Math.round(height),
        focusable,
        monitorIndex: this.getMonitorIndex(),
      });
    } catch {
      // Safe fallback when previewing in browser outside Tauri
    }
  }

  public async togglePanel(panel: PanelName): Promise<void> {
    if (this.state.activePanel === panel) {
      await this.collapse();
    } else {
      await this.openPanel(panel);
    }
  }

  public async openPanel(panel: PanelName): Promise<void> {
    if (this.collapseTimer !== null) {
      window.clearTimeout(this.collapseTimer);
      this.collapseTimer = null;
    }

    const host = document.getElementById("panel-host");
    const backdrop = document.getElementById("overlay-backdrop");
    const island = document.getElementById("dynamic-island");

    this.state.activePanel = panel;

    // Expand native Windows overlay bounds BEFORE revealing panel
    const targetHeight = PANEL_HEIGHTS[panel] ?? 440;
    const needsFocus = panel === "files" || panel === "tasks" || panel === "settings";
    await this.setOverlayHeight(targetHeight, needsFocus);

    if (host) {
      host.setAttribute("aria-hidden", "false");
      host.classList.toggle("align-right", RIGHT_ALIGNED_PANELS.has(panel));
    }
    backdrop?.classList.add("is-active");
    island?.classList.add("is-expanded");
    island?.setAttribute("aria-expanded", "true");

    // Hide all other panels, show requested panel
    document.querySelectorAll<HTMLElement>(".gd-panel").forEach((el) => {
      const isMatch = el.dataset.panel === panel;
      if (isMatch) {
        el.hidden = false;
        this.onRenderPanel(panel);
        requestAnimationFrame(() => el.classList.add("is-open"));
      } else {
        el.classList.remove("is-open");
        el.hidden = true;
      }
    });

    this.syncActiveButtons(panel);
  }

  public async collapse(): Promise<void> {
    const island = document.getElementById("dynamic-island");
    const backdrop = document.getElementById("overlay-backdrop");
    const host = document.getElementById("panel-host");

    this.state.activePanel = null;
    resetFilesPanelTransientState();
    island?.classList.remove("is-expanded");
    island?.setAttribute("aria-expanded", "false");
    backdrop?.classList.remove("is-active");
    this.syncActiveButtons(null);

    document.querySelectorAll<HTMLElement>(".gd-panel").forEach((el) => {
      el.classList.remove("is-open");
    });

    // Wait for smooth collapse transition before shrinking Win32 window height
    this.collapseTimer = window.setTimeout(async () => {
      document.querySelectorAll<HTMLElement>(".gd-panel").forEach((el) => {
        el.hidden = true;
      });
      host?.setAttribute("aria-hidden", "true");
      await this.setOverlayHeight(this.getCollapsedHeight(), false);
      this.collapseTimer = null;
    }, 220);
  }

  public renderHubPanel(): void {
    const el = document.getElementById("panel-hub");
    if (!el) return;

    const heldCount = this.state.heldFiles.length;
    const pendingTasks = this.state.tasks.filter((t) => !t.completed).length;
    const net = this.state.system?.network;
    const bat = this.state.system?.battery;
    const audio = this.state.system?.audio;
    const recentFiles = this.state.heldFiles.slice(0, 3);

    const netText = net?.internet
      ? net.networkName || "Connected"
      : net?.connected
        ? "Local network"
        : "Disconnected";

    const batText = bat?.hasBattery && bat.percent !== null
      ? `${bat.percent}%${bat.isCharging ? " · Charging" : ""}`
      : "AC Power";

    const volText = audio
      ? audio.muted
        ? "Muted"
        : `${audio.volumePercent}%`
      : "--";

    el.innerHTML = `
      <div class="panel-header">
        <div class="panel-title-group">
          <span class="panel-title">GLASSDESK Dynamic Island</span>
          <span class="panel-subtitle">Quick Overview</span>
        </div>
        <div class="panel-actions">
          <button class="gd-btn" data-open-panel="settings" type="button">Settings</button>
          <button class="gd-btn" data-close-panel type="button">Esc</button>
        </div>
      </div>

      <div class="hub-grid">
        <button class="glass-card hub-card" data-open-panel="files" type="button">
          <span class="hub-card-label">FILE HOLD Shelf</span>
          <span class="hub-card-value">${heldCount} ${heldCount === 1 ? "file" : "files"}</span>
          <span class="hub-card-meta">${
            recentFiles.length > 0
              ? `Latest: ${escapeHtml(recentFiles[0].displayName)}`
              : "Drag files from Explorer onto top bar"
          }</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="tasks" type="button">
          <span class="hub-card-label">Pending Tasks</span>
          <span class="hub-card-value">${pendingTasks}</span>
          <span class="hub-card-meta">${
            pendingTasks > 0 ? "Click to review & manage" : "All caught up"
          }</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="controls" type="button">
          <span class="hub-card-label">System Status</span>
          <span class="hub-card-value">${escapeHtml(volText)} Vol</span>
          <span class="hub-card-meta">${escapeHtml(netText)} · ${escapeHtml(batText)}</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="calendar" type="button">
          <span class="hub-card-label">Calendar & Reminders</span>
          <span class="hub-card-value">${new Date().toLocaleDateString(undefined, {
            weekday: "short",
            day: "numeric",
            month: "short",
          })}</span>
          <span class="hub-card-meta">${this.state.reminders.filter((r) => !r.dismissed).length} active reminders</span>
        </button>
      </div>
    `;

    el.querySelectorAll<HTMLElement>("[data-open-panel]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const target = btn.dataset.openPanel as PanelName | undefined;
        if (target) void this.openPanel(target);
      });
    });

    el.querySelector("[data-close-panel]")?.addEventListener("click", () => {
      void this.collapse();
    });
  }

  private syncActiveButtons(active: PanelName | null): void {
    const map: Record<string, PanelName> = {
      "sys-controls-btn": "controls",
      "sys-settings-btn": "settings",
      "sys-clock-btn": "calendar",
      "island-files-badge": "files",
      "island-tasks-badge": "tasks",
    };
    for (const [id, panel] of Object.entries(map)) {
      document.getElementById(id)?.classList.toggle("is-active", active === panel);
    }
  }

  private bindGlobalDismissal(): void {
    const backdrop = document.getElementById("overlay-backdrop");
    backdrop?.addEventListener("click", () => {
      if (this.state.activePanel) {
        void this.collapse();
      }
    });

    window.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.state.activePanel) {
        e.preventDefault();
        void this.collapse();
      }
    });
  }
}
