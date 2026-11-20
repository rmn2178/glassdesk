import { getCurrentWebview } from "@tauri-apps/api/webview";
import { callCommand, isTauriRuntime, showToast } from "./ipc";
import type { IslandController } from "./island";
import type { AppState, HoldFilesBatchResult } from "./types";

export async function initNativeDragDrop(
  state: AppState,
  islandCtrl: IslandController,
  onFilesUpdated: () => Promise<void>
): Promise<void> {
  if (!isTauriRuntime()) return;

  const island = document.getElementById("dynamic-island");
  const dropSheet = document.getElementById("drop-zone-sheet");
  const feedbackText = document.getElementById("island-feedback-text");

  let isDragging = false;
  let feedbackTimer: number | null = null;

  const showDragState = async () => {
    if (isDragging) return;
    isDragging = true;
    island?.classList.remove("is-feedback");
    island?.classList.add("is-drag-over");
    dropSheet?.classList.add("is-visible");
    dropSheet?.setAttribute("aria-hidden", "false");

    if (!state.activePanel) {
      await islandCtrl.setOverlayHeight(136, false);
    }
  };

  const hideDragState = async () => {
    if (!isDragging) return;
    isDragging = false;
    island?.classList.remove("is-drag-over");
    dropSheet?.classList.remove("is-visible");
    dropSheet?.setAttribute("aria-hidden", "true");

    if (!state.activePanel) {
      const collapsed = Number.parseInt(state.settings["collapsed_height"] || "36", 10) || 36;
      await islandCtrl.setOverlayHeight(collapsed, false);
    }
  };

  const showDropSuccess = (message: string) => {
    if (feedbackText) feedbackText.textContent = message;
    island?.classList.add("is-feedback");
    if (feedbackTimer !== null) window.clearTimeout(feedbackTimer);
    feedbackTimer = window.setTimeout(() => {
      island?.classList.remove("is-feedback");
      feedbackTimer = null;
    }, 1500);
  };

  try {
    const webview = getCurrentWebview();
    await webview.onDragDropEvent(async (event) => {
      try {
      const payload = event.payload;

      if (payload.type === "enter" || payload.type === "over") {
        await showDragState();
      } else if (payload.type === "leave") {
        await hideDragState();
      } else if (payload.type === "drop") {
        const paths = payload.paths ?? [];
        island?.classList.remove("is-drag-over");
        dropSheet?.classList.remove("is-visible");
        dropSheet?.setAttribute("aria-hidden", "true");
        isDragging = false;

        if (paths.length === 0) {
          await hideDragState();
          return;
        }

        const vaultDefault = state.settings["vault_copy_by_default"] === "true";

        try {
          const result = await callCommand<HoldFilesBatchResult>("hold_files_batch", {
            paths,
            copyToVault: vaultDefault,
          });

          await onFilesUpdated();

          const addedCount = result.added.length;
          const dupCount = result.skippedDuplicates.length;
          const failCount = result.failed.length;

          if (addedCount > 0) {
            showDropSuccess(`Held ${addedCount} ${addedCount === 1 ? "file" : "files"}`);
          } else if (dupCount > 0) {
            showToast(`Already in FILE HOLD (${dupCount} skipped)`);
          }

          if (failCount > 0) {
            showToast(`${failCount} path(s) could not be accessed`);
          }

          const autoCollapse = state.settings["auto_collapse_after_drop"] !== "false";
          if (!state.activePanel && autoCollapse) {
            window.setTimeout(async () => {
              if (!state.activePanel) {
                const collapsed =
                  Number.parseInt(state.settings["collapsed_height"] || "36", 10) || 36;
                await islandCtrl.setOverlayHeight(collapsed, false);
              }
            }, 950);
          } else if (!autoCollapse) {
            await islandCtrl.openPanel("files");
          }
        } catch (err) {
          showToast(`Drop failed: ${String(err)}`);
          if (!state.activePanel) {
            const collapsed =
              Number.parseInt(state.settings["collapsed_height"] || "36", 10) || 36;
            await islandCtrl.setOverlayHeight(collapsed, false);
          }
        }
      }
      } catch (err) {
        console.error('Drag-drop callback error:', err);
      }
    });
  } catch (err) {
    console.error("Failed to attach Tauri native drag-drop listener:", err);
  }
}
