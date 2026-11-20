import React from "react";
import { AppConfig } from "../types";

interface Props {
  config: AppConfig;
  onChange: (next: AppConfig) => void;
}

const THEMES: { id: AppConfig["theme"]; label: string }[] = [
  { id: "liquid-glass", label: "Liquid Glass" },
  { id: "pure-black", label: "Pure Black" },
  { id: "black", label: "Black" },
  { id: "graphite", label: "Graphite" },
  { id: "acrylic", label: "Acrylic" },
  { id: "candy-glass", label: "Candy Glass" },
  { id: "rose", label: "Rosé" },
  { id: "lavender", label: "Lavender" },
  { id: "peach", label: "Peach" },
  { id: "sakura", label: "Sakura" },
  { id: "pure-white", label: "Pure White" },
];

export const SettingsTab: React.FC<Props> = ({ config, onChange }) => {
  const patch = (partial: Partial<AppConfig>) => onChange({ ...config, ...partial });

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="grid-2">
        <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ fontSize: 12, fontWeight: 600 }}>Island Shape & Position</div>
          <div className="row">
            <button
              className={`btn ${config.shape === "island" ? "btn-primary" : ""}`}
              style={{ flex: 1 }}
              onClick={() => patch({ shape: "island" })}
            >
              Dynamic Island
            </button>
            <button
              className={`btn ${config.shape === "notch" ? "btn-primary" : ""}`}
              style={{ flex: 1 }}
              onClick={() => patch({ shape: "notch" })}
            >
              Mac Notch
            </button>
          </div>
          <div className="row">
            {(["left", "center", "right"] as const).map((pos) => (
              <button
                key={pos}
                className={`btn ${config.position === pos ? "btn-primary" : ""}`}
                style={{ flex: 1, textTransform: "capitalize" }}
                onClick={() => patch({ position: pos })}
              >
                {pos}
              </button>
            ))}
          </div>
        </div>

        <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div style={{ fontSize: 12, fontWeight: 600 }}>Theme ({config.theme})</div>
          <select
            className="input"
            value={config.theme}
            onChange={(e) => patch({ theme: e.target.value as AppConfig["theme"] })}
          >
            {THEMES.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </select>
          <div className="row-between muted">
            <span>Glass Blur ({config.glassBlur}px)</span>
            <input
              type="range"
              min={0}
              max={32}
              value={config.glassBlur}
              onChange={(e) => patch({ glassBlur: Number(e.target.value) })}
            />
          </div>
        </div>
      </div>

      <div className="grid-2">
        <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div className="row-between muted">
            <span>Hover Delay ({config.hoverDelayMs}ms)</span>
            <input
              type="range"
              min={0}
              max={1500}
              step={50}
              value={config.hoverDelayMs}
              onChange={(e) => patch({ hoverDelayMs: Number(e.target.value) })}
            />
          </div>
          <div className="row-between" style={{ fontSize: 12 }}>
            <span>Hide from Screenshots</span>
            <input
              type="checkbox"
              checked={config.hideFromCapture}
              onChange={(e) => patch({ hideFromCapture: e.target.checked })}
            />
          </div>
          <div className="row-between" style={{ fontSize: 12 }}>
            <span>Auto-Hide Until Hover</span>
            <input
              type="checkbox"
              checked={config.autoHide}
              onChange={(e) => patch({ autoHide: e.target.checked })}
            />
          </div>
        </div>

        <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <div className="row-between" style={{ fontSize: 12 }}>
            <span>Resting Face</span>
            <select
              className="input"
              style={{ width: 110 }}
              value={config.restingFace}
              onChange={(e) => patch({ restingFace: e.target.value as AppConfig["restingFace"] })}
            >
              <option value="clock">Clock</option>
              <option value="stats">PC Stats</option>
              <option value="media">Media</option>
              <option value="pet">Pixel Pet</option>
            </select>
          </div>
          <div className="row-between" style={{ fontSize: 12 }}>
            <span>Fullscreen Games</span>
            <select
              className="input"
              style={{ width: 110 }}
              value={config.fullscreenMode}
              onChange={(e) => patch({ fullscreenMode: e.target.value as AppConfig["fullscreenMode"] })}
            >
              <option value="slim">Game Strip</option>
              <option value="hide">Hide</option>
              <option value="normal">Normal</option>
            </select>
          </div>
          <div className="row-between" style={{ fontSize: 12 }}>
            <span>Visualizer</span>
            <select
              className="input"
              style={{ width: 110 }}
              value={config.visualizerStyle}
              onChange={(e) => patch({ visualizerStyle: e.target.value as AppConfig["visualizerStyle"] })}
            >
              <option value="rounded">Rounded</option>
              <option value="pixel">Pixel Bars</option>
            </select>
          </div>
        </div>
      </div>
    </div>
  );
};
