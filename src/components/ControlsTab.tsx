import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Mic, MicOff, Pipette, Lock, Volume2, ShieldAlert } from "lucide-react";
import { AudioState, PrivacySensors } from "../types";

interface Props {
  audio: AudioState;
  sensors: PrivacySensors;
  onRefreshAudio: () => void;
  onToast: (msg: string) => void;
}

export const ControlsTab: React.FC<Props> = ({
  audio,
  sensors,
  onRefreshAudio,
  onToast,
}) => {
  const [pickedHex, setPickedHex] = useState<string | null>(null);

  const handleVolume = async (val: number) => {
    await invoke("set_master_volume", { percent: val });
    onRefreshAudio();
  };

  const handleMicToggle = async () => {
    const muted = await invoke<boolean>("toggle_mic_mute");
    onRefreshAudio();
    onToast(muted ? "Microphone Muted (Global)" : "Microphone Live");
  };

  const handleColorPick = async () => {
    try {
      const hex = await invoke<string>("pick_screen_color");
      setPickedHex(hex);
      onToast(`Copied ${hex} under cursor`);
    } catch (e) {
      onToast(String(e));
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div className="row-between">
          <span className="row" style={{ fontSize: 12, fontWeight: 600 }}>
            <Volume2 size={15} /> Master Volume
          </span>
          <span className="mono badge">{audio.volumePercent}%</span>
        </div>
        <input
          type="range"
          min={0}
          max={100}
          value={audio.volumePercent}
          onChange={(e) => handleVolume(Number(e.target.value))}
          style={{ width: "100%", accentColor: "var(--accent)" }}
        />
      </div>

      <div className="grid-3">
        <button
          className={`btn ${audio.micMuted ? "btn-primary" : ""}`}
          style={{ padding: "14px 10px", flexDirection: "column" }}
          onClick={handleMicToggle}
        >
          {audio.micMuted ? <MicOff size={18} /> : <Mic size={18} />}
          <span>{audio.micMuted ? "Mic Muted" : "Mic Live"}</span>
          <span className="muted mono">Ctrl+Alt+M</span>
        </button>

        <button
          className="btn"
          style={{ padding: "14px 10px", flexDirection: "column" }}
          onClick={handleColorPick}
        >
          <Pipette size={18} />
          <span>{pickedHex ? pickedHex : "Color Picker"}</span>
          <span className="muted mono">Ctrl+Alt+C</span>
        </button>

        <button
          className="btn"
          style={{ padding: "14px 10px", flexDirection: "column" }}
          onClick={() => invoke("lock_pc")}
        >
          <Lock size={18} />
          <span>Lock PC</span>
          <span className="muted mono">Win32 Lock</span>
        </button>
      </div>

      <div className="glass-card row-between">
        <div className="row">
          <ShieldAlert size={15} />
          <div style={{ fontSize: 12 }}>
            <div style={{ fontWeight: 600 }}>Privacy Sensors</div>
            <div className="muted">
              Mic: {sensors.micActiveApps.length ? sensors.micActiveApps.join(", ") : "Idle"} · Cam:{" "}
              {sensors.camActiveApps.length ? sensors.camActiveApps.join(", ") : "Idle"}
            </div>
          </div>
        </div>
        <div className="row">
          {sensors.micActiveApps.length > 0 && <span className="sensor-dot mic" />}
          {sensors.camActiveApps.length > 0 && <span className="sensor-dot cam" />}
        </div>
      </div>
    </div>
  );
};
