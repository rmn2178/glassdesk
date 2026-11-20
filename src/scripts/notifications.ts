import { callCommand, escapeHtml, formatRelativeTime, ICONS, showToast } from "./ipc";
import type { AppState, NotificationItem } from "./types";

const notifiedReminderIds = new Set<string>();

export function syncNotificationsBadge(state: AppState): void {
  const unread = state.notifications.filter((n) => !n.read).length;
  const countEl = document.getElementById("island-notif-count");
  const badgeEl = document.getElementById("island-notif-badge");
  if (countEl) countEl.textContent = String(unread);
  badgeEl?.classList.toggle("has-items", unread > 0);
}

export async function refreshNotifications(state: AppState): Promise<void> {
  try {
    state.notifications = await callCommand<NotificationItem[]>("list_notifications");
  } catch {
    // Keep existing state outside Tauri
  }
  syncNotificationsBadge(state);
}

export async function pushAppNotification(
  state: AppState,
  title: string,
  message: string,
  category = "system",
  onUpdated?: () => void
): Promise<void> {
  const enabled = state.settings["notifications_enabled"] !== "false";
  if (!enabled) return;

  try {
    const item = await callCommand<NotificationItem>("push_notification", {
      title,
      message,
      category,
    });
    state.notifications = [item, ...state.notifications];
  } catch {
    const localItem: NotificationItem = {
      id: `local-${Date.now()}`,
      title,
      message,
      category,
      read: false,
      createdAt: new Date().toISOString(),
    };
    state.notifications = [localItem, ...state.notifications];
  }

  syncNotificationsBadge(state);
  if (!state.dndMode) {
    showToast(`${title}: ${message}`, 2800);
  }
  onUpdated?.();
}

export function startReminderWatcher(
  state: AppState,
  onNotificationsUpdated: () => void
): void {
  const check = () => {
    if (document.hidden || state.settings["notifications_enabled"] === "false") return;
    if (state.reminders.length === 0) return;
    const nowMs = Date.now();
    for (const rem of state.reminders) {
      if (rem.dismissed || notifiedReminderIds.has(rem.id)) continue;
      const dueMs = new Date(rem.remindAt).getTime();
      if (Number.isFinite(dueMs) && dueMs <= nowMs) {
        notifiedReminderIds.add(rem.id);
        void pushAppNotification(
          state,
          "Reminder Due",
          rem.title,
          "reminder",
          onNotificationsUpdated
        );
      }
    }
  };
  window.setInterval(check, 20_000);
}

export function renderNotificationsPanel(
  state: AppState,
  onMutated: () => Promise<void>,
  onClose: () => void
): void {
  const el = document.getElementById("panel-notifications");
  if (!el) return;

  const unreadCount = state.notifications.filter((n) => !n.read).length;

  el.innerHTML = `
    <div class="panel-header gd-drag-handle" title="Drag to move panel · Double-click to recenter">
      <div class="panel-title-group">
        <div class="mac-lights">
          <button class="mac-dot mac-close" data-mac-close type="button" aria-label="Close panel"></button>
          <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
        </div>
        <span class="panel-title">Notifications</span>
        <span class="panel-subtitle">${unreadCount} unread</span>
      </div>
      <div class="panel-actions">
        ${
          state.dndMode
            ? `<span class="tag-chip" style="color: var(--warn);">DND Active</span>`
            : ""
        }
        ${
          state.notifications.length > 0
            ? `<button class="gd-btn gd-btn-danger" id="btn-clear-notifs" type="button">Clear All</button>`
            : ""
        }
      </div>
    </div>

    ${
      state.notifications.length === 0
        ? `<div class="empty-state">No recent notifications. Reminders, focus timer alerts, and file-drop confirmations appear here.</div>`
        : `<div class="notif-list">
            ${state.notifications
              .map(
                (n, i) => `
              <div class="glass-card notif-card ${n.read ? "" : "is-unread"}" style="animation-delay: ${Math.min(i * 25, 140)}ms">
                <span class="notif-dot" aria-hidden="true"></span>
                <div class="notif-body">
                  <div class="notif-title">${escapeHtml(n.title)}</div>
                  <div class="notif-msg">${escapeHtml(n.message)}</div>
                  <div class="notif-time">${escapeHtml(n.category)} · ${formatRelativeTime(n.createdAt)}</div>
                </div>
                <div class="file-actions">
                  ${
                    !n.read
                      ? `<button class="icon-mini-btn" data-read-notif="${n.id}" title="Mark as read" type="button">${ICONS.check}</button>`
                      : ""
                  }
                  <button class="icon-mini-btn danger" data-del-notif="${n.id}" title="Dismiss" type="button">${ICONS.trash}</button>
                </div>
              </div>`
              )
              .join("")}
          </div>`
    }
  `;

  el.querySelector("[data-mac-close]")?.addEventListener("click", (e) => {
    e.stopPropagation();
    onClose();
  });

  el.querySelectorAll<HTMLElement>("[data-read-notif]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.readNotif;
      if (!id) return;
      try {
        await callCommand("mark_notification_read", { id });
      } catch {
        const found = state.notifications.find((n) => n.id === id);
        if (found) found.read = true;
      }
      await onMutated();
    });
  });

  el.querySelectorAll<HTMLElement>("[data-del-notif]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.delNotif;
      if (!id) return;
      try {
        await callCommand("delete_notification", { id });
      } catch {
        state.notifications = state.notifications.filter((n) => n.id !== id);
      }
      await onMutated();
    });
  });

  el.querySelector("#btn-clear-notifs")?.addEventListener("click", async () => {
    try {
      await callCommand("clear_notifications");
    } catch {
      state.notifications = [];
    }
    showToast("Cleared notifications");
    await onMutated();
  });
}
