import { callCommand, escapeHtml, ICONS, showToast } from "./ipc";
import type { AppState } from "./types";

let viewedYear = new Date().getFullYear();
let viewedMonth = new Date().getMonth();

function defaultLocalReminderTime(offsetMinutes = 30): string {
  const d = new Date(Date.now() + offsetMinutes * 60_000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatClockStrings(now: Date, use24h: boolean, showSeconds = false): {
  shortTime: string;
  fullTime: string;
  shortDate: string;
  fullDate: string;
} {
  const shortTime = now.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    ...(showSeconds ? { second: "2-digit" } : {}),
    hour12: !use24h,
  });
  const fullTime = now.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: !use24h,
  });
  const shortDate = now.toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
  });
  const fullDate = now.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });
  return { shortTime, fullTime, shortDate, fullDate };
}

/**
 * Adaptive ultra-low-CPU clock:
 * - Ticks once per minute (aligned to minute boundary) when idle & seconds are hidden.
 * - Ticks once per second only when seconds are enabled or the Calendar hero clock is open.
 * - Suspends automatically while the overlay window is hidden.
 */
export function startLightweightClock(state: AppState): void {
  const islandTime = document.getElementById("island-clock-time");
  const topbarTime = document.getElementById("topbar-time");
  const topbarDate = document.getElementById("topbar-date");
  let timerId: number | null = null;

  const scheduleNext = () => {
    if (timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
    if (document.hidden) return;

    const showSecs = state.settings["clock_show_seconds"] === "true";
    const needsSecondTick = showSecs || state.activePanel === "calendar";
    const now = Date.now();
    const delayMs = needsSecondTick
      ? 1000 - (now % 1000) + 16
      : 60_000 - (now % 60_000) + 50;

    timerId = window.setTimeout(tick, delayMs);
  };

  const tick = () => {
    const use24h = state.settings["clock_format"] === "24h";
    const showSecs = state.settings["clock_show_seconds"] === "true";
    const now = new Date();
    const { shortTime, fullTime, shortDate } = formatClockStrings(now, use24h, showSecs);

    if (islandTime && islandTime.textContent !== shortTime) {
      islandTime.textContent = shortTime;
    }
    if (topbarTime && topbarTime.textContent !== shortTime) {
      topbarTime.textContent = shortTime;
    }
    if (topbarDate && topbarDate.textContent !== shortDate) {
      topbarDate.textContent = shortDate;
    }

    const calHero = document.getElementById("cal-live-clock");
    if (calHero && calHero.textContent !== fullTime) {
      calHero.textContent = fullTime;
    }

    scheduleNext();
  };

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      tick();
    } else if (timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  });

  window.addEventListener("glassdesk:clock-sync", () => tick());
  tick();
}

export function renderCalendarPanel(
  state: AppState,
  onRemindersMutated?: () => Promise<void>,
  onClose?: () => void
): void {
  const el = document.getElementById("panel-calendar");
  if (!el) return;

  const now = new Date();
  const use24h = state.settings["clock_format"] === "24h";
  const { fullTime, fullDate } = formatClockStrings(now, use24h);

  const firstDay = new Date(viewedYear, viewedMonth, 1);
  const startDow = firstDay.getDay();
  const daysInMonth = new Date(viewedYear, viewedMonth + 1, 0).getDate();
  const daysInPrev = new Date(viewedYear, viewedMonth, 0).getDate();

  const monthLabel = firstDay.toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });

  const taskDates = new Set(
    state.tasks
      .filter((t) => !t.completed && t.dueAt)
      .map((t) => (t.dueAt ?? "").slice(0, 10))
  );

  const cells: string[] = [];
  for (let i = startDow - 1; i >= 0; i--) {
    cells.push(`<div class="cal-day is-other-month">${daysInPrev - i}</div>`);
  }

  for (let day = 1; day <= daysInMonth; day++) {
    const isToday =
      day === now.getDate() &&
      viewedMonth === now.getMonth() &&
      viewedYear === now.getFullYear();

    const isoKey = `${viewedYear}-${String(viewedMonth + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    const hasTask = taskDates.has(isoKey);
    cells.push(
      `<div class="cal-day ${isToday ? "is-today" : ""} ${hasTask ? "has-task" : ""}">${day}</div>`
    );
  }

  const remainder = (7 - (cells.length % 7)) % 7;
  for (let d = 1; d <= remainder; d++) {
    cells.push(`<div class="cal-day is-other-month">${d}</div>`);
  }

  const activeReminders = state.reminders.filter((r) => !r.dismissed).slice(0, 4);

  el.innerHTML = `
    <div class="panel-header gd-drag-handle" title="Drag to move panel · Double-click to recenter">
      <div class="panel-title-group">
        <div class="mac-lights">
          <button class="mac-dot mac-close" data-mac-close type="button" aria-label="Close panel"></button>
          <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
        </div>
        <span class="panel-title">Time & Calendar</span>
      </div>
      <div class="panel-actions">
        <button class="gd-btn" id="cal-today-btn" type="button">Today</button>
      </div>
    </div>

    <div class="cal-clock-hero">
      <span class="cal-big-time" id="cal-live-clock">${escapeHtml(fullTime)}</span>
      <span class="panel-subtitle">${escapeHtml(fullDate)}</span>
    </div>

    <div class="cal-nav">
      <button class="gd-btn" id="cal-prev-btn" type="button" aria-label="Previous month">‹</button>
      <span style="font-weight: 600; font-size: 12.5px;">${escapeHtml(monthLabel)}</span>
      <button class="gd-btn" id="cal-next-btn" type="button" aria-label="Next month">›</button>
    </div>

    <div class="cal-grid">
      <div class="cal-dow">Su</div>
      <div class="cal-dow">Mo</div>
      <div class="cal-dow">Tu</div>
      <div class="cal-dow">We</div>
      <div class="cal-dow">Th</div>
      <div class="cal-dow">Fr</div>
      <div class="cal-dow">Sa</div>
      ${cells.join("")}
    </div>

    <div class="cal-reminders-box">
      <span class="hub-card-label">Upcoming Reminders</span>
      <form id="cal-reminder-form" class="cal-reminder-form">
        <input id="cal-rem-title" class="gd-input" style="padding: 4px 8px; font-size: 11px;" type="text" placeholder="Add reminder..." required />
        <input id="cal-rem-time" class="gd-input" style="padding: 4px 6px; font-size: 10.5px; width: 132px;" type="datetime-local" value="${defaultLocalReminderTime(30)}" required />
        <button class="gd-btn gd-btn-primary" style="padding: 4px 9px;" type="submit">+</button>
      </form>
      <div class="cal-reminders-list">
        ${
          activeReminders.length === 0
            ? `<span class="panel-subtitle">No active reminders.</span>`
            : activeReminders
                .map(
                  (r) => `
              <div class="glass-card cal-reminder-row">
                <span style="flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">${escapeHtml(r.title)}</span>
                <span class="panel-subtitle">${escapeHtml(r.remindAt.replace("T", " ").slice(5, 16))}</span>
                <button class="icon-mini-btn" data-dismiss-rem="${r.id}" title="Dismiss" type="button">${ICONS.check}</button>
                <button class="icon-mini-btn danger" data-delete-rem="${r.id}" title="Delete" type="button">${ICONS.trash}</button>
              </div>`
                )
                .join("")
        }
      </div>
    </div>
  `;

  window.dispatchEvent(new Event("glassdesk:clock-sync"));

  el.querySelector("[data-mac-close]")?.addEventListener("click", (e) => {
    e.stopPropagation();
    onClose?.();
  });

  el.querySelector("#cal-prev-btn")?.addEventListener("click", () => {
    viewedMonth -= 1;
    if (viewedMonth < 0) {
      viewedMonth = 11;
      viewedYear -= 1;
    }
    renderCalendarPanel(state, onRemindersMutated, onClose);
  });

  el.querySelector("#cal-next-btn")?.addEventListener("click", () => {
    viewedMonth += 1;
    if (viewedMonth > 11) {
      viewedMonth = 0;
      viewedYear += 1;
    }
    renderCalendarPanel(state, onRemindersMutated, onClose);
  });

  el.querySelector("#cal-today-btn")?.addEventListener("click", () => {
    const t = new Date();
    viewedYear = t.getFullYear();
    viewedMonth = t.getMonth();
    renderCalendarPanel(state, onRemindersMutated, onClose);
  });

  el.querySelector<HTMLFormElement>("#cal-reminder-form")?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const titleEl = el.querySelector<HTMLInputElement>("#cal-rem-title");
    const timeEl = el.querySelector<HTMLInputElement>("#cal-rem-time");
    const title = titleEl?.value.trim() ?? "";
    const remindAt = timeEl?.value ?? "";
    if (!title || !remindAt) return;
    try {
      await callCommand("create_reminder", { title, remindAt, repeatRule: null });
      showToast("Reminder added");
      if (onRemindersMutated) await onRemindersMutated();
    } catch (err) {
      showToast(`Reminder error: ${String(err)}`);
    }
  });

  el.querySelectorAll<HTMLElement>("[data-dismiss-rem]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.dismissRem;
      if (!id) return;
      await callCommand("dismiss_reminder", { id, dismissed: true });
      if (onRemindersMutated) await onRemindersMutated();
    });
  });

  el.querySelectorAll<HTMLElement>("[data-delete-rem]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.deleteRem;
      if (!id) return;
      await callCommand("delete_reminder", { id });
      if (onRemindersMutated) await onRemindersMutated();
    });
  });
}
