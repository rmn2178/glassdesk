import { callCommand, escapeHtml, ICONS, showToast } from "./ipc";
import type { AppState, TaskCategory, TaskFilter, TaskItem } from "./types";

let activeFilter: TaskFilter = "today";
let activeCategory: "All" | TaskCategory = "All";
let editingTaskId: string | null = null;

function todayIsoDate(): string {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function toDatetimeLocal(iso: string | null): string {
  if (!iso) return "";
  return iso.replace(" ", "T").slice(0, 16);
}

export function syncTasksBadge(state: AppState): void {
  const pending = state.tasks.filter((t) => !t.completed).length;
  const countEl = document.getElementById("island-tasks-count");
  const badgeEl = document.getElementById("island-tasks-badge");
  if (countEl) countEl.textContent = String(pending);
  badgeEl?.classList.toggle("has-items", pending > 0);
}

export async function refreshTasks(state: AppState): Promise<void> {
  try {
    state.tasks = await callCommand<TaskItem[]>("list_tasks");
  } catch {
    // Keep existing state outside Tauri
  }
  syncTasksBadge(state);
}

function filterTasks(tasks: TaskItem[]): TaskItem[] {
  const today = todayIsoDate();
  return tasks.filter((t) => {
    if (activeCategory !== "All" && t.category !== activeCategory) {
      return false;
    }
    const due = t.dueAt ? t.dueAt.slice(0, 10) : null;
    if (activeFilter === "completed") return t.completed;
    if (t.completed) return false;
    if (activeFilter === "overdue") return due !== null && due < today;
    if (activeFilter === "upcoming") return due !== null && due > today;
    return due === null || due === today;
  });
}

function priorityLabel(p: number): { text: string; cls: string } {
  if (p >= 2) return { text: "High", cls: "prio-high" };
  if (p === 1) return { text: "Medium", cls: "prio-med" };
  return { text: "Low", cls: "" };
}

export function renderTasksPanel(
  state: AppState,
  onMutated: () => Promise<void>
): void {
  const el = document.getElementById("panel-tasks");
  if (!el) return;

  const visible = filterTasks(state.tasks);
  const pendingCount = state.tasks.filter((t) => !t.completed).length;

  const filters: Array<{ id: TaskFilter; label: string }> = [
    { id: "today", label: "Today" },
    { id: "upcoming", label: "Upcoming" },
    { id: "overdue", label: "Overdue" },
    { id: "completed", label: "Completed" },
  ];

  const categories: Array<"All" | TaskCategory> = [
    "All",
    "College",
    "Personal",
    "Projects",
    "Other",
  ];

  el.innerHTML = `
    <div class="panel-header">
      <div class="panel-title-group">
        <span class="panel-title">Tasks</span>
        <span class="panel-subtitle">${pendingCount} pending</span>
      </div>
      <div class="pill-tabs">
        ${categories
          .map(
            (c) => `
          <button class="pill-tab ${activeCategory === c ? "is-active" : ""}" data-task-cat="${c}" type="button">${c}</button>`
          )
          .join("")}
      </div>
    </div>

    <div class="pill-tabs">
      ${filters
        .map(
          (f) => `
        <button class="pill-tab ${activeFilter === f.id ? "is-active" : ""}" data-task-filter="${f.id}" type="button">${f.label}</button>`
        )
        .join("")}
    </div>

    <form id="task-add-form" style="display: flex; flex-direction: column; gap: 6px;">
      <div class="task-form">
        <input id="task-title-input" class="gd-input" type="text" placeholder="Add a new task..." required />
        <select id="task-cat-input" class="gd-select" aria-label="Category">
          <option value="Personal">Personal</option>
          <option value="College">College</option>
          <option value="Projects">Projects</option>
          <option value="Other">Other</option>
        </select>
        <select id="task-prio-input" class="gd-select" aria-label="Priority">
          <option value="1">Med</option>
          <option value="2">High</option>
          <option value="0">Low</option>
        </select>
        <input id="task-due-input" class="gd-input" type="date" title="Deadline" />
        <button class="gd-btn gd-btn-primary" type="submit">Add</button>
      </div>
      <div class="task-form-extra">
        <span>Optional reminder:</span>
        <input id="task-rem-input" class="gd-input" style="padding: 3px 7px; font-size: 11px;" type="datetime-local" aria-label="Reminder time" />
      </div>
    </form>

    ${
      visible.length === 0
        ? `<div class="empty-state">No tasks in this view.</div>`
        : `<div class="tasks-list">
            ${visible
              .map((task) => {
                if (task.id === editingTaskId) {
                  return `
                  <div class="glass-card task-row">
                    <form class="task-edit-form" data-edit-form="${task.id}">
                      <input class="gd-input" name="title" type="text" value="${escapeHtml(task.title)}" required />
                      <div class="task-edit-row">
                        <select class="gd-select" name="category">
                          <option value="Personal" ${task.category === "Personal" ? "selected" : ""}>Personal</option>
                          <option value="College" ${task.category === "College" ? "selected" : ""}>College</option>
                          <option value="Projects" ${task.category === "Projects" ? "selected" : ""}>Projects</option>
                          <option value="Other" ${task.category === "Other" ? "selected" : ""}>Other</option>
                        </select>
                        <select class="gd-select" name="priority">
                          <option value="0" ${task.priority === 0 ? "selected" : ""}>Low</option>
                          <option value="1" ${task.priority === 1 ? "selected" : ""}>Med</option>
                          <option value="2" ${task.priority >= 2 ? "selected" : ""}>High</option>
                        </select>
                        <input class="gd-input" name="dueAt" type="date" value="${escapeHtml((task.dueAt ?? "").slice(0, 10))}" />
                        <input class="gd-input" name="reminderAt" type="datetime-local" value="${escapeHtml(toDatetimeLocal(task.reminderAt))}" />
                      </div>
                      <div class="task-edit-actions">
                        <button class="gd-btn" data-cancel-edit type="button">Cancel</button>
                        <button class="gd-btn gd-btn-primary" type="submit">Save</button>
                      </div>
                    </form>
                  </div>`;
                }

                const prio = priorityLabel(task.priority);
                return `
                <div class="glass-card task-row chip-animate ${task.completed ? "is-completed" : ""}">
                  <button class="task-check" data-toggle-task="${task.id}" type="button" aria-label="Toggle task complete">
                    ${task.completed ? ICONS.check : ""}
                  </button>
                  <div class="task-body">
                    <div class="task-title">${escapeHtml(task.title)}</div>
                    <div class="task-meta">
                      <span class="tag-chip">${escapeHtml(task.category)}</span>
                      <span class="${prio.cls}">${prio.text}</span>
                      ${task.dueAt ? `<span>Due ${escapeHtml(task.dueAt.slice(0, 10))}</span>` : ""}
                      ${task.reminderAt ? `<span>· Remind ${escapeHtml(toDatetimeLocal(task.reminderAt).replace("T", " "))}</span>` : ""}
                    </div>
                  </div>
                  <button class="gd-btn" style="padding: 3px 7px; font-size: 10.5px;" data-edit-task="${task.id}" type="button">Edit</button>
                  <button class="icon-mini-btn danger" data-delete-task="${task.id}" title="Delete task" type="button">${ICONS.trash}</button>
                </div>`;
              })
              .join("")}
          </div>`
    }
  `;

  el.querySelectorAll<HTMLElement>("[data-task-filter]").forEach((btn) => {
    btn.addEventListener("click", () => {
      activeFilter = btn.dataset.taskFilter as TaskFilter;
      editingTaskId = null;
      renderTasksPanel(state, onMutated);
    });
  });

  el.querySelectorAll<HTMLElement>("[data-task-cat]").forEach((btn) => {
    btn.addEventListener("click", () => {
      activeCategory = btn.dataset.taskCat as "All" | TaskCategory;
      editingTaskId = null;
      renderTasksPanel(state, onMutated);
    });
  });

  const form = el.querySelector<HTMLFormElement>("#task-add-form");
  form?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const titleEl = el.querySelector<HTMLInputElement>("#task-title-input");
    const catEl = el.querySelector<HTMLSelectElement>("#task-cat-input");
    const prioEl = el.querySelector<HTMLSelectElement>("#task-prio-input");
    const dueEl = el.querySelector<HTMLInputElement>("#task-due-input");
    const remEl = el.querySelector<HTMLInputElement>("#task-rem-input");

    const title = titleEl?.value.trim() ?? "";
    if (!title) return;

    try {
      await callCommand("create_task", {
        title,
        notes: null,
        category: catEl?.value || "Personal",
        priority: Number.parseInt(prioEl?.value || "1", 10),
        dueAt: dueEl?.value || null,
        reminderAt: remEl?.value || null,
      });
      showToast("Task added");
      await onMutated();
    } catch (err) {
      showToast(`Could not add task: ${String(err)}`);
    }
  });

  el.querySelectorAll<HTMLElement>("[data-edit-task]").forEach((btn) => {
    btn.addEventListener("click", () => {
      editingTaskId = btn.dataset.editTask ?? null;
      renderTasksPanel(state, onMutated);
    });
  });

  el.querySelector("[data-cancel-edit]")?.addEventListener("click", () => {
    editingTaskId = null;
    renderTasksPanel(state, onMutated);
  });

  el.querySelector<HTMLFormElement>("[data-edit-form]")?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const editForm = e.currentTarget as HTMLFormElement;
    const id = editForm.dataset.editForm;
    if (!id) return;
    const fd = new FormData(editForm);
    const title = String(fd.get("title") ?? "").trim();
    if (!title) return;

    await callCommand("update_task", {
      id,
      title,
      notes: null,
      category: String(fd.get("category") ?? "Personal"),
      completed: null,
      priority: Number.parseInt(String(fd.get("priority") ?? "1"), 10),
      dueAt: String(fd.get("dueAt") ?? ""),
      reminderAt: String(fd.get("reminderAt") ?? ""),
    });
    editingTaskId = null;
    showToast("Task updated");
    await onMutated();
  });

  el.querySelectorAll<HTMLElement>("[data-toggle-task]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.toggleTask;
      const found = state.tasks.find((t) => t.id === id);
      if (!found) return;
      await callCommand("update_task", {
        id: found.id,
        title: null,
        notes: null,
        category: null,
        completed: !found.completed,
        priority: null,
        dueAt: null,
        reminderAt: null,
      });
      await onMutated();
    });
  });

  el.querySelectorAll<HTMLElement>("[data-delete-task]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.deleteTask;
      if (!id) return;
      await callCommand("delete_task", { id });
      showToast("Task deleted");
      await onMutated();
    });
  });
}
