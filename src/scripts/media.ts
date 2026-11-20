import { callCommand, escapeHtml, ICONS, showToast } from "./ipc";
import type { AppState, AudioStatus, MediaSessionInfo } from "./types";

const MEDIA_ICONS = {
  prev: `<svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor"><polygon points="19 20 9 12 19 4 19 20"/><rect x="5" y="4" width="2.2" height="16" rx="1"/></svg>`,
  next: `<svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 4 15 12 5 20 5 4"/><rect x="16.8" y="4" width="2.2" height="16" rx="1"/></svg>`,
  play: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 3 20 12 6 21 6 3"/></svg>`,
  pause: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1.2"/><rect x="14" y="4" width="4" height="16" rx="1.2"/></svg>`,
  note: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`,
};

export function syncMusicPill(state: AppState): void {
  const badge = document.getElementById("island-music-badge");
  const label = document.getElementById("island-music-label");
  const eq = document.getElementById("island-music-eq");
  if (!badge || !label || !eq) return;

  const media = state.system?.media;
  if (media?.hasSession) {
    badge.classList.add("has-items");
    badge.classList.toggle("is-playing", media.isPlaying);
    eq.classList.toggle("is-animating", media.isPlaying);
    const shortTitle =
      media.title.length > 16 ? `${media.title.slice(0, 15)}…` : media.title;
    label.textContent = shortTitle || "Music";
    badge.title = `${media.title}${media.artist ? ` — ${media.artist}` : ""}`;
  } else {
    badge.classList.remove("has-items", "is-playing");
    eq.classList.remove("is-animating");
    label.textContent = "Music";
    badge.title = "Open Music Controls";
  }
}

export function renderMusicPanel(
  state: AppState,
  onMediaUpdated: () => void,
  onClose: () => void
): void {
  const el = document.getElementById("panel-music");
  if (!el) return;

  const media = state.system?.media;
  const audio = state.system?.audio;
  const hasSession = Boolean(media?.hasSession);
  const isPlaying = Boolean(media?.isPlaying);
  const volPct = audio?.volumePercent ?? 50;
  const isMuted = Boolean(audio?.muted);

  el.innerHTML = `
    <div class="panel-header gd-drag-handle" title="Drag to move panel · Double-click to recenter">
      <div class="panel-title-group">
        <div class="mac-lights">
          <button class="mac-dot mac-close" data-mac-close type="button" aria-label="Close panel"></button>
          <button class="mac-dot mac-reset" data-mac-reset type="button" aria-label="Recenter panel"></button>
        </div>
        <span class="panel-title">Now Playing</span>
        ${
          hasSession && media?.sourceApp
            ? `<span class="tag-chip">${escapeHtml(media.sourceApp)}</span>`
            : ""
        }
      </div>
      <div class="panel-actions">
        <span class="eq-bars ${isPlaying ? "is-animating" : ""}" aria-hidden="true">
          <span></span><span></span><span></span>
        </span>
      </div>
    </div>

    <div class="glass-card music-stage">
      <div class="music-art">
        ${MEDIA_ICONS.note}
      </div>
      <div class="music-meta">
        <div class="music-title">${
          hasSession ? escapeHtml(media!.title || "Unknown Track") : "Nothing Playing"
        }</div>
        <div class="music-artist">${
          hasSession
            ? escapeHtml(
                [media!.artist, media!.album].filter(Boolean).join(" · ") ||
                  "Windows Media Session"
              )
            : "Start playback in Spotify, Chrome, Edge, or VLC"
        }</div>
      </div>
    </div>

    <div class="music-transport">
      <button class="gd-btn music-btn" data-media-action="previous" type="button" aria-label="Previous track">
        ${MEDIA_ICONS.prev}
      </button>
      <button class="gd-btn gd-btn-primary music-btn music-btn-main" data-media-action="play_pause" type="button" aria-label="Play or Pause">
        ${isPlaying ? MEDIA_ICONS.pause : MEDIA_ICONS.play}
      </button>
      <button class="gd-btn music-btn" data-media-action="next" type="button" aria-label="Next track">
        ${MEDIA_ICONS.next}
      </button>
    </div>

    <div class="glass-card volume-box">
      <div style="display: flex; justify-content: space-between; align-items: center;">
        <span class="ctrl-label">Output Volume</span>
        <span class="ctrl-sub" id="music-vol-readout">${isMuted ? "Muted" : `${volPct}%`}</span>
      </div>
      <div class="volume-row">
        <button class="gd-btn" id="music-mute-btn" type="button" title="Toggle Mute">
          ${isMuted ? ICONS.volMuted : ICONS.volHigh}
        </button>
        <input id="music-vol-slider" class="gd-range" type="range" min="0" max="100" value="${volPct}" aria-label="Music Volume" />
      </div>
    </div>
  `;

  el.querySelector("[data-mac-close]")?.addEventListener("click", (e) => {
    e.stopPropagation();
    onClose();
  });

  el.querySelectorAll<HTMLElement>("[data-media-action]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const action = btn.dataset.mediaAction;
      if (!action) return;
      try {
        const updated = await callCommand<MediaSessionInfo>("media_control", { action });
        if (state.system) {
          state.system.media = updated;
        }
        syncMusicPill(state);
        renderMusicPanel(state, onMediaUpdated, onClose);
        onMediaUpdated();
      } catch (err) {
        showToast(`Media control error: ${String(err)}`);
      }
    });
  });

  const slider = el.querySelector<HTMLInputElement>("#music-vol-slider");
  const readout = el.querySelector<HTMLElement>("#music-vol-readout");
  slider?.addEventListener("change", async () => {
    const pct = Math.max(0, Math.min(100, Math.round(Number(slider.value))));
    if (readout) readout.textContent = `${pct}%`;
    try {
      const updated = await callCommand<AudioStatus>("set_system_volume", { level: pct / 100 });
      if (state.system) state.system.audio = updated;
      onMediaUpdated();
    } catch {
      // Offline preview
    }
  });
  slider?.addEventListener("input", () => {
    if (readout) readout.textContent = `${slider.value}%`;
  });

  el.querySelector("#music-mute-btn")?.addEventListener("click", async () => {
    try {
      const updated = await callCommand<AudioStatus>("toggle_system_mute");
      if (state.system) state.system.audio = updated;
      renderMusicPanel(state, onMediaUpdated, onClose);
      onMediaUpdated();
    } catch (e) {
      showToast(`Audio error: ${String(e)}`);
    }
  });
}
