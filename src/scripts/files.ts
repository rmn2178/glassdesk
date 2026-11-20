import {
  callCommand,
  escapeHtml,
  formatBytes,
  formatRelativeTime,
  ICONS,
  showToast,
} from "./ipc";
import type { AppState, FileCategory, HeldFile } from "./types";

let activeCategory: FileCategory = "all";
let searchQuery = "";
let sortMode: "date" | "name" = "date";
let confirmClear = false;
let inspectedFileId: string | null = null;
let shellMounted = false;

export function resetFilesPanelTransientState(): void {
  confirmClear = false;
  inspectedFileId = null;
  shellMounted = false;
}

function iconForFile(file: HeldFile): string {
  if (file.thumbnailB64) {
    return `<img src="${file.thumbnailB64}" alt="" loading="lazy" />`;
  }
  if (file.isDir) return ICONS.folder;
  if (file.category === "images") return ICONS.image;
  if (file.category === "videos") return ICONS.video;
  if (file.category === "documents") return ICONS.fileText;
  return ICONS.file;
}

function getFilteredFiles(files: HeldFile[]): HeldFile[] {
  const q = searchQuery.trim().toLowerCase();
  const filtered = files.filter((f) => {
    const catOk = activeCategory === "all" || f.category === activeCategory;
    const queryOk =
      !q ||
      f.displayName.toLowerCase().includes(q) ||
      f.extension.toLowerCase().includes(q) ||
      f.originalPath.toLowerCase().includes(q);
    return catOk && queryOk;
  });

  return filtered.sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
    if (sortMode === "name") {
      return a.displayName.localeCompare(b.displayName);
    }
    return new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime();
  });
}

export function syncFilesBadge(state: AppState): void {
  const countEl = document.getElementById("island-files-count");
  const badgeEl = document.getElementById("island-files-badge");
  const count = state.heldFiles.length;
  if (countEl) countEl.textContent = String(count);
  badgeEl?.classList.toggle("has-items", count > 0);
}

export async function refreshHeldFiles(state: AppState): Promise<void> {
  try {
    state.heldFiles = await callCommand<HeldFile[]>("list_held_files");
  } catch {
    // Keep existing state if outside Tauri
  }
  syncFilesBadge(state);
}

function renderClearControls(
  el: HTMLElement,
  state: AppState,
  onMutated: () => Promise<void>
): void {
  const slot = el.querySelector<HTMLElement>("#files-clear-slot");
  const countLabel = el.querySelector<HTMLElement>("#files-count-label");
  if (countLabel) countLabel.textContent = `${state.heldFiles.length} held`;
  if (!slot) return;

  if (state.heldFiles.length === 0) {
    slot.innerHTML = "";
    return;
  }

  slot.innerHTML = confirmClear
    ? `<button class="gd-btn gd-btn-danger" id="btn-confirm-clear" type="button">Confirm Clear All</button>
       <button class="gd-btn" id="btn-cancel-clear" type="button">Cancel</button>`
    : `<button class="gd-btn gd-btn-danger" id="btn-clear-shelf" type="button">Clear Shelf</button>`;

  slot.querySelector("#btn-clear-shelf")?.addEventListener("click", () => {
    confirmClear = true;
    renderClearControls(el, state, onMutated);
  });
  slot.querySelector("#btn-cancel-clear")?.addEventListener("click", () => {
    confirmClear = false;
    renderClearControls(el, state, onMutated);
  });
  slot.querySelector("#btn-confirm-clear")?.addEventListener("click", async () => {
    confirmClear = false;
    await callCommand("clear_held_files");
    showToast("Cleared FILE HOLD shelf");
    await onMutated();
  });
}

function updateFilesBody(
  el: HTMLElement,
  state: AppState,
  onMutated: () => Promise<void>,
  animate: boolean
): void {
  renderClearControls(el, state, onMutated);

  const infoSlot = el.querySelector<HTMLElement>("#files-info-slot");
  const gridSlot = el.querySelector<HTMLElement>("#files-grid-slot");
  if (!infoSlot || !gridSlot) return;

  const inspected = inspectedFileId
    ? state.heldFiles.find((f) => f.id === inspectedFileId) ?? null
    : null;

  infoSlot.innerHTML = inspected
    ? `<div class="glass-card" style="padding: 9px 11px; font-size: 11px; display: flex; justify-content: space-between; gap: 10px;">
         <div style="min-width: 0;">
           <div style="font-weight: 650;">${escapeHtml(inspected.displayName)}</div>
           <div style="color: var(--text-secondary); word-break: break-all; margin-top: 2px;">${escapeHtml(inspected.originalPath)}</div>
           <div style="color: var(--text-muted); margin-top: 2px;">
             Type: ${escapeHtml(inspected.extension || "file").toUpperCase()} · Size: ${formatBytes(inspected.sizeBytes)} · Added ${formatRelativeTime(inspected.createdAt)}
           </div>
         </div>
         <button class="gd-btn" id="btn-close-info" type="button" style="align-self: flex-start;">Close</button>
       </div>`
    : "";

  infoSlot.querySelector("#btn-close-info")?.addEventListener("click", () => {
    inspectedFileId = null;
    updateFilesBody(el, state, onMutated, false);
  });

  const visible = getFilteredFiles(state.heldFiles);
  if (visible.length === 0) {
    gridSlot.innerHTML = `<div class="empty-state">
      ${
        state.heldFiles.length === 0
          ? "Drag any files or folders from Windows Explorer onto the top bar to hold them here."
          : "No held files match your current filter."
      }
    </div>`;
    return;
  }

  const animCls = animate ? " chip-animate" : "";
  gridSlot.innerHTML = `<div class="files-list">
    ${visible
      .map(
        (file, i) => `
      <div class="glass-card file-chip${animCls} ${file.available ? "" : "is-missing"}" ${
          animate ? `style="animation-delay: ${Math.min(i * 22, 140)}ms"` : ""
        } data-file-id="${file.id}">
        <div class="file-thumb">${iconForFile(file)}</div>
        <div class="file-meta" data-info-id="${file.id}" style="cursor: pointer;" title="Click for file info">
          <div class="file-name">${escapeHtml(file.displayName)}</div>
          <div class="file-sub">
            <span>${file.isDir ? "Folder" : formatBytes(file.sizeBytes)}</span>
            <span>·</span>
            <span>${formatRelativeTime(file.createdAt)}</span>
            ${file.isVaultCopy ? `<span class="badge-vault">Vault</span>` : ""}
            ${!file.available ? `<span class="badge-missing">Missing</span>` : ""}
          </div>
        </div>
        <div class="file-actions">
          <button class="icon-mini-btn" data-open-path="${escapeHtml(file.vaultPath || file.originalPath)}" title="Open with default app" type="button">${ICONS.external}</button>
          <button class="icon-mini-btn" data-reveal-path="${escapeHtml(file.originalPath)}" title="Reveal in Windows Explorer" type="button">${ICONS.reveal}</button>
          <button class="icon-mini-btn" data-copy-path="${escapeHtml(file.originalPath)}" title="Copy file path" type="button">${ICONS.copy}</button>
          ${
            !file.isDir && !file.isVaultCopy && file.available
              ? `<button class="icon-mini-btn" data-vault-id="${file.id}" title="Copy to Vault (independent copy)" type="button">${ICONS.vault}</button>`
              : ""
          }
          <button class="icon-mini-btn danger" data-remove-id="${file.id}" title="Remove from shelf (original stays safe)" type="button">${ICONS.trash}</button>
        </div>
      </div>`
      )
      .join("")}
  </div>`;

  gridSlot.querySelectorAll<HTMLElement>("[data-info-id]").forEach((node) => {
    node.addEventListener("click", () => {
      const id = node.dataset.infoId ?? null;
      inspectedFileId = inspectedFileId === id ? null : id;
      updateFilesBody(el, state, onMutated, false);
    });
  });

  gridSlot.querySelectorAll<HTMLElement>("[data-open-path]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const p = btn.dataset.openPath;
      if (!p) return;
      try {
        await callCommand("open_file", { path: p });
      } catch (e) {
        showToast(`Cannot open file: ${String(e)}`);
      }
    });
  });

  gridSlot.querySelectorAll<HTMLElement>("[data-reveal-path]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const p = btn.dataset.revealPath;
      if (!p) return;
      try {
        await callCommand("reveal_file_in_explorer", { path: p });
      } catch (e) {
        showToast(`Explorer error: ${String(e)}`);
      }
    });
  });

  gridSlot.querySelectorAll<HTMLElement>("[data-copy-path]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const p = btn.dataset.copyPath;
      if (!p) return;
      try {
        await navigator.clipboard.writeText(p);
        showToast("Copied file path");
      } catch {
        showToast("Clipboard unavailable");
      }
    });
  });

  gridSlot.querySelectorAll<HTMLElement>("[data-vault-id]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.vaultId;
      if (!id) return;
      try {
        await callCommand("copy_held_file_to_vault", { id });
        showToast("Copied safely to Vault");
        await onMutated();
      } catch (e) {
        showToast(`Vault copy failed: ${String(e)}`);
      }
    });
  });

  gridSlot.querySelectorAll<HTMLElement>("[data-remove-id]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.removeId;
      if (!id) return;
      await callCommand("remove_held_file", { id });
      if (inspectedFileId === id) inspectedFileId = null;
      showToast("Removed from shelf");
      await onMutated();
    });
  });
}

export function renderFilesPanel(
  state: AppState,
  onMutated: () => Promise<void>
): void {
  const el = document.getElementById("panel-files");
  if (!el) return;

  if (!shellMounted || !el.querySelector("#files-grid-slot")) {
    const categories: Array<{ id: FileCategory; label: string }> = [
      { id: "all", label: "Recently Added" },
      { id: "documents", label: "Documents" },
      { id: "images", label: "Images" },
      { id: "videos", label: "Videos" },
      { id: "other", label: "Other" },
    ];

    el.innerHTML = `
      <div class="panel-header">
        <div class="panel-title-group">
          <span class="panel-title">FILE HOLD Shelf</span>
          <span class="panel-subtitle" id="files-count-label">${state.heldFiles.length} held</span>
        </div>
        <div class="panel-actions" id="files-clear-slot"></div>
      </div>

      <div class="pill-tabs" id="files-cat-tabs">
        ${categories
          .map(
            (c) => `
          <button
            class="pill-tab ${activeCategory === c.id ? "is-active" : ""}"
            data-file-cat="${c.id}"
            type="button"
          >${c.label}</button>`
          )
          .join("")}
      </div>

      <div class="files-toolbar">
        <input
          id="files-search-input"
          class="gd-input files-search"
          type="search"
          placeholder="Search held files..."
          autocomplete="off"
          value="${escapeHtml(searchQuery)}"
        />
        <select id="files-sort-select" class="gd-select" aria-label="Sort files">
          <option value="date" ${sortMode === "date" ? "selected" : ""}>Date Added</option>
          <option value="name" ${sortMode === "name" ? "selected" : ""}>Name (A–Z)</option>
        </select>
      </div>

      <div id="files-info-slot"></div>
      <div id="files-grid-slot"></div>
    `;

    el.querySelectorAll<HTMLElement>("[data-file-cat]").forEach((btn) => {
      btn.addEventListener("click", () => {
        activeCategory = btn.dataset.fileCat as FileCategory;
        el.querySelectorAll<HTMLElement>("[data-file-cat]").forEach((b) =>
          b.classList.toggle("is-active", b === btn)
        );
        updateFilesBody(el, state, onMutated, true);
      });
    });

    const searchInput = el.querySelector<HTMLInputElement>("#files-search-input");
    searchInput?.addEventListener("input", () => {
      searchQuery = searchInput.value;
      updateFilesBody(el, state, onMutated, false);
    });

    const sortSelect = el.querySelector<HTMLSelectElement>("#files-sort-select");
    sortSelect?.addEventListener("change", () => {
      sortMode = sortSelect.value === "name" ? "name" : "date";
      updateFilesBody(el, state, onMutated, false);
    });

    shellMounted = true;
    updateFilesBody(el, state, onMutated, true);
    return;
  }

  updateFilesBody(el, state, onMutated, false);
}
