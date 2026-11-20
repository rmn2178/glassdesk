import { resetFilesPanelTransientState } from "./files";
import { callCommand, escapeHtml } from "./ipc";
import type { AppState, PanelName } from "./types";

const PANEL_HEIGHTS: Record<PanelName, number> = {
  hub: 390,
  files: 550,
  calendar: 520,
  tasks: 535,
  controls: 590,
  notifications: 470,
  music: 410,
  settings: 600,
};

const RIGHT_ALIGNED_PANELS = new Set<PanelName>(["calendar", "controls", "settings"]);

export class IslandController {
  private state: AppState;
  private onRenderPanel: (panel: PanelName) => void;
  private collapseTimer: number | null = null;
  private hoverTimer: number | null = null;
  private autoHideTimer: number | null = null;
  private pointerOverShell = false;

  constructor(state: AppState, onRenderPanel: (panel: PanelName) => void) {
    this.state = state;
    this.onRenderPanel = onRenderPanel;
    this.bindGlobalDismissal();
    this.bindMovablePanels();
    this.bindModeDock();
    this.bindHoverExpansion();
    this.bindAutoHide();
  }

  private getMonitorIndex(): number | null {
    const raw = this.state.settings["monitor_index"];
    if (raw === undefined) return null;
    const parsed = Number.parseInt(raw, 10);
    return Number.isFinite(parsed) ? parsed : null;
  }

  public getCollapsedHeight(): number {
    const style = this.state.settings["island_style"] || "bar";
    if (style === "floating") {
      const pillH = Number.parseInt(this.state.settings["island_height"] || "32", 10) || 32;
      const topOff = Number.parseInt(this.state.settings["island_top_offset"] || "8", 10) || 8;
      return Math.max(44, pillH + topOff + 14);
    }
    const raw = this.state.settings["collapsed_height"];
    const parsed = raw ? Number.parseInt(raw, 10) : 36;
    return Number.isFinite(parsed) ? Math.max(32, Math.min(48, parsed)) : 36;
  }

  public async setOverlayHeight(
    height: number,
    focusable: boolean,
    expanded = false
  ): Promise<void> {
    const style = this.state.settings["island_style"] || "bar";
    const floatingCompact = style === "floating" && !expanded;
    const islandWidth =
      Number.parseInt(this.state.settings["island_width"] || "380", 10) || 380;
    const horizontalOffset =
      Number.parseInt(this.state.settings["island_horizontal_offset"] || "0", 10) || 0;
    const alwaysOnTop = this.state.settings["always_on_top"] !== "false";

    try {
      await callCommand("set_overlay_bounds", {
        height: Math.round(height),
        focusable,
        monitorIndex: this.getMonitorIndex(),
        floatingWidth: floatingCompact ? Math.max(360, islandWidth + 80) : null,
        horizontalOffset: floatingCompact ? horizontalOffset : 0,
        alwaysOnTop,
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
    const dock = document.getElementById("island-mode-dock");
    const backdrop = document.getElementById("overlay-backdrop");
    const island = document.getElementById("dynamic-island");

    this.wakeIsland();
    this.state.activePanel = panel;

    const targetHeight = PANEL_HEIGHTS[panel] ?? 500;
    const needsFocus =
      panel === "files" ||
      panel === "tasks" ||
      panel === "settings" ||
      panel === "calendar";
    await this.setOverlayHeight(targetHeight, needsFocus, true);

    if (host) {
      host.setAttribute("aria-hidden", "false");
      host.classList.toggle("align-right", RIGHT_ALIGNED_PANELS.has(panel));
    }
    if (dock) {
      dock.hidden = false;
    }
    backdrop?.classList.add("is-active");
    island?.classList.add("is-expanded");
    island?.setAttribute("aria-expanded", "true");

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
    const dock = document.getElementById("island-mode-dock");
    const backdrop = document.getElementById("overlay-backdrop");
    const host = document.getElementById("panel-host");

    this.state.activePanel = null;
    resetFilesPanelTransientState();
    island?.classList.remove("is-expanded");
    island?.setAttribute("aria-expanded", "false");
    backdrop?.classList.remove("is-active");
    if (dock) dock.hidden = true;
    this.syncActiveButtons(null);

    document.querySelectorAll<HTMLElement>(".gd-panel").forEach((el) => {
      el.classList.remove("is-open");
    });

    this.collapseTimer = window.setTimeout(async () => {
      document.querySelectorAll<HTMLElement>(".gd-panel").forEach((el) => {
        el.hidden = true;
        el.innerHTML = "";
        el.style.translate = "";
      });
      host?.setAttribute("aria-hidden", "true");
      await this.setOverlayHeight(this.getCollapsedHeight(), false, false);
      void callCommand("trim_memory").catch(() => {});
      this.collapseTimer = null;
      this.scheduleAutoHide();
    }, 220);
  }

  public renderHubPanel(): void {
    const el = document.getElementById("panel-hub");
    if (!el) return;

    const heldCount = this.state.heldFiles.length;
    const pendingTasks = this.state.tasks.filter((t) => !t.completed).length;
    const unreadNotifs = this.state.notifications.filter((n) => !n.read).length;
    const net = this.state.system?.network;
    const bat = this.state.system?.battery;
    const audio = this.state.system?.audio;
    const recentFiles = this.state.heldFiles.slice(0, 3);

    const netText = net?.internet
      ? net.networkName || "Connected"
      : net?.connected
        ? "Local network"
        : "Offline";

    const batText =
      bat?.hasBattery && bat.percent !== null
        ? `${bat.percent}%${bat.isCharging ? " · Charging" : ""}`
        : "AC Power";

    const volText = audio
      ? audio.muted
        ? "Muted"
        : `${audio.volumePercent}%`
      : "--";

    el.innerHTML = `
      <div class="panel-header gd-drag-handle" title="Drag to move panel · Double-click to recenter">
        <div class="panel-title-group">
          <div class="mac-lights">
            <button class="mac-dot mac-close" data-close-panel type="button" aria-label="Close panel"></button>
            <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
          </div>
          <span class="panel-title">GLASSDESK Dynamic Island</span>
          <span class="panel-subtitle">Quick Overview</span>
        </div>
        <div class="panel-actions">
          <button class="gd-btn" data-open-panel="settings" type="button">Settings</button>
        </div>
      </div>

      <div class="hub-grid">
        <button class="glass-card hub-card" data-open-panel="files" type="button">
          <span class="hub-card-label">File Drop Shelf</span>
          <span class="hub-card-value">${heldCount} ${heldCount === 1 ? "file" : "files"}</span>
          <span class="hub-card-meta">${
            recentFiles.length > 0
              ? `Latest: ${escapeHtml(recentFiles[0].displayName)}`
              : "Drag files from Explorer onto the island"
          }</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="tasks" type="button">
          <span class="hub-card-label">Tasks & Reminders</span>
          <span class="hub-card-value">${pendingTasks} pending</span>
          <span class="hub-card-meta">${
            pendingTasks > 0 ? "Click to review & manage" : "All caught up"
          }</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="controls" type="button">
          <span class="hub-card-label">Control Center</span>
          <span class="hub-card-value">${escapeHtml(volText)} Vol</span>
          <span class="hub-card-meta">${escapeHtml(netText)} · ${escapeHtml(batText)}</span>
        </button>

        <button class="glass-card hub-card" data-open-panel="notifications" type="button">
          <span class="hub-card-label">Notifications</span>
          <span class="hub-card-value">${unreadNotifs} unread</span>
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
      "island-music-badge": "music",
      "island-notif-badge": "notifications",
      "island-controls-badge": "controls",
    };
    for (const [id, panel] of Object.entries(map)) {
      document.getElementById(id)?.classList.toggle("is-active", active === panel);
    }

    document.querySelectorAll<HTMLElement>(".gd-mode-tab").forEach((tab) => {
      tab.classList.toggle("is-active", tab.dataset.mode === active);
    });
  }

  private bindModeDock(): void {
    const dock = document.getElementById("island-mode-dock");
    if (!dock) return;
    dock.querySelectorAll<HTMLElement>("[data-mode]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const mode = btn.dataset.mode as PanelName | undefined;
        if (mode) void this.openPanel(mode);
      });
    });
  }


  public wakeIsland(): void {
    if (this.autoHideTimer !== null) {
      window.clearTimeout(this.autoHideTimer);
      this.autoHideTimer = null;
    }
    document.getElementById("top-bar")?.classList.remove("is-auto-hidden");
  }

  public scheduleAutoHide(delayMs = 1400): void {
    if (this.autoHideTimer !== null) {
      window.clearTimeout(this.autoHideTimer);
      this.autoHideTimer = null;
    }
    if (this.state.settings["auto_hide_idle"] === "false") {
      this.wakeIsland();
      return;
    }
    if (this.state.activePanel || this.pointerOverShell) return;
    this.autoHideTimer = window.setTimeout(() => {
      this.autoHideTimer = null;
      const island = document.getElementById("dynamic-island");
      const isDragging = island?.classList.contains("is-drag-over") ?? false;
      if (!this.state.activePanel && !this.pointerOverShell && !isDragging) {
        document.getElementById("top-bar")?.classList.add("is-auto-hidden");
      }
    }, delayMs);
  }

  private bindAutoHide(): void {
    const hotspot = document.getElementById("top-edge-hotspot");
    const topBar = document.getElementById("top-bar");
    const onEnter = () => {
      this.pointerOverShell = true;
      this.wakeIsland();
    };
    const onLeave = () => {
      this.pointerOverShell = false;
      this.scheduleAutoHide();
    };
    hotspot?.addEventListener("pointerenter", onEnter);
    topBar?.addEventListener("pointerenter", onEnter);
    topBar?.addEventListener("pointerleave", onLeave);
    window.addEventListener("glassdesk:autohide-sync", () => {
      if (this.state.settings["auto_hide_idle"] === "false") {
        this.wakeIsland();
      } else {
        this.scheduleAutoHide(900);
      }
    });
    this.scheduleAutoHide(1800);
  }

  private bindHoverExpansion(): void {
    const island = document.getElementById("dynamic-island");
    if (!island) return;
    island.addEventListener("pointerenter", () => {
      if (this.state.settings["hover_expand"] !== "true") return;
      if (this.state.activePanel) return;
      if (this.hoverTimer !== null) window.clearTimeout(this.hoverTimer);
      this.hoverTimer = window.setTimeout(() => {
        this.hoverTimer = null;
        if (!this.state.activePanel) {
          const defaultMode = (this.state.settings["default_expanded_mode"] || "hub") as PanelName;
          void this.openPanel(defaultMode);
        }
      }, 280);
    });
    island.addEventListener("pointerleave", () => {
      if (this.hoverTimer !== null) {
        window.clearTimeout(this.hoverTimer);
        this.hoverTimer = null;
      }
    });
  }

  private bindMovablePanels(): void {
    const host = document.getElementById("panel-host");
    if (!host) return;

    let activePanel: HTMLElement | null = null;
    let startX = 0;
    let startY = 0;
    let baseX = 0;
    let baseY = 0;

    host.addEventListener("pointerdown", (e) => {
      const target = e.target as HTMLElement;
      if (target.closest("button, input, select")) return;
      const header = target.closest<HTMLElement>(".panel-header");
      const panel = header?.closest<HTMLElement>(".gd-panel");
      if (!panel) return;

      activePanel = panel;
      startX = e.clientX;
      startY = e.clientY;
      baseX = Number(panel.dataset.dragX || "0");
      baseY = Number(panel.dataset.dragY || "0");
      panel.setPointerCapture(e.pointerId);
    });

    host.addEventListener("pointermove", (e) => {
      if (!activePanel) return;
      const dx = Math.max(-420, Math.min(420, baseX + (e.clientX - startX)));
      const dy = Math.max(-6, Math.min(180, baseY + (e.clientY - startY)));
      activePanel.dataset.dragX = String(dx);
      activePanel.dataset.dragY = String(dy);
      activePanel.style.translate = `${dx}px ${dy}px`;
    });

    const endDrag = () => {
      activePanel = null;
    };
    host.addEventListener("pointerup", endDrag);
    host.addEventListener("pointercancel", endDrag);

    host.addEventListener("dblclick", (e) => {
      const header = (e.target as HTMLElement).closest<HTMLElement>(".panel-header");
      const panel = header?.closest<HTMLElement>(".gd-panel");
      if (!panel) return;
      panel.dataset.dragX = "0";
      panel.dataset.dragY = "0";
      panel.style.translate = "";
    });

    host.addEventListener("click", (e) => {
      const resetBtn = (e.target as HTMLElement).closest<HTMLElement>("[data-mac-reset]");
      const panel = resetBtn?.closest<HTMLElement>(".gd-panel");
      if (!panel) return;
      panel.dataset.dragX = "0";
      panel.dataset.dragY = "0";
      panel.style.translate = "";
    });
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
