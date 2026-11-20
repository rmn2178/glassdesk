import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Rocket, Edit3, Check } from "lucide-react";
import { LauncherSlot } from "../types";

interface Props {
  slots: LauncherSlot[];
  onRefresh: () => void;
  onToast: (msg: string) => void;
}

export const LauncherTab: React.FC<Props> = ({ slots, onRefresh, onToast }) => {
  const [editing, setEditing] = useState<number | null>(null);
  const [label, setLabel] = useState("");
  const [targetPath, setTargetPath] = useState("");

  const startEdit = (slot: LauncherSlot) => {
    setEditing(slot.slotIndex);
    setLabel(slot.label);
    setTargetPath(slot.targetPath);
  };

  const saveEdit = async (slotIndex: number) => {
    await invoke("update_launcher_slot", { slotIndex, label, targetPath });
    setEditing(null);
    onRefresh();
    onToast(`Saved slot #${slotIndex}`);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="muted">6 configurable quick-launch slots (apps, executables, or URLs)</div>
      <div className="grid-3">
        {slots.map((slot) =>
          editing === slot.slotIndex ? (
            <div key={slot.slotIndex} className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 5 }}>
              <input
                className="input"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                placeholder="Label"
              />
              <input
                className="input mono"
                value={targetPath}
                onChange={(e) => setTargetPath(e.target.value)}
                placeholder="wt.exe or URL"
              />
              <button className="btn btn-primary" onClick={() => saveEdit(slot.slotIndex)}>
                <Check size={12} /> Save
              </button>
            </div>
          ) : (
            <div key={slot.slotIndex} className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div className="row-between">
                <span className="badge mono">#{slot.slotIndex}</span>
                <button
                  className="btn"
                  style={{ padding: "2px 5px" }}
                  onClick={() => startEdit(slot)}
                  title="Edit slot"
                >
                  <Edit3 size={11} />
                </button>
              </div>
              <button
                className="btn btn-primary"
                style={{ width: "100%" }}
                onClick={async () => {
                  try {
                    await invoke("launch_target", { targetPath: slot.targetPath });
                    onToast(`Launched ${slot.label}`);
                  } catch (e) {
                    onToast(String(e));
                  }
                }}
              >
                <Rocket size={13} /> {slot.label}
              </button>
              <div className="muted mono" style={{ fontSize: 10, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {slot.targetPath}
              </div>
            </div>
          )
        )}
      </div>
    </div>
  );
};
