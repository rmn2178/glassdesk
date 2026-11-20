import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BellPlus, Check, Clock, Trash2 } from "lucide-react";
import { ReminderItem } from "../types";

interface Props {
  reminders: ReminderItem[];
  onRefresh: () => void;
  onToast: (msg: string) => void;
}

export const RemindersTab: React.FC<Props> = ({ reminders, onRefresh, onToast }) => {
  const [title, setTitle] = useState("");
  const [minutes, setMinutes] = useState(15);
  const [repeatRule, setRepeatRule] = useState("once");

  const create = async () => {
    if (!title.trim()) return;
    const dueAt = Math.floor(Date.now() / 1000) + minutes * 60;
    await invoke("add_reminder", {
      title: title.trim(),
      dueAt,
      repeatRule,
    });
    setTitle("");
    onRefresh();
    onToast(`Reminder set for ${minutes}m`);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="glass-card row" style={{ flexWrap: "nowrap" }}>
        <input
          className="input"
          placeholder="Remind me to..."
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && create()}
        />
        <select
          className="input"
          style={{ width: 92 }}
          value={minutes}
          onChange={(e) => setMinutes(Number(e.target.value))}
        >
          <option value={5}>5 min</option>
          <option value={15}>15 min</option>
          <option value={30}>30 min</option>
          <option value={60}>1 hour</option>
        </select>
        <select
          className="input"
          style={{ width: 96 }}
          value={repeatRule}
          onChange={(e) => setRepeatRule(e.target.value)}
        >
          <option value="once">Once</option>
          <option value="daily">Daily</option>
          <option value="weekdays">Weekdays</option>
          <option value="weekly">Weekly</option>
        </select>
        <button className="btn btn-primary" onClick={create}>
          <BellPlus size={14} /> Add
        </button>
      </div>

      {reminders.length === 0 ? (
        <div className="glass-card muted" style={{ textAlign: "center", padding: 24 }}>
          No reminders scheduled.
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {reminders.map((r) => {
            const dueStr = new Date(r.dueAt * 1000).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            });
            return (
              <div key={r.id} className="glass-card row-between" style={{ opacity: r.isDone ? 0.55 : 1 }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 600, textDecoration: r.isDone ? "line-through" : "none" }}>
                    {r.title}
                  </div>
                  <div className="muted mono">
                    Due {dueStr} · {r.repeatRule}
                  </div>
                </div>
                <div className="row" style={{ gap: 5 }}>
                  <button
                    className="btn"
                    title="Snooze 10m"
                    onClick={async () => {
                      await invoke("snooze_reminder", { id: r.id, minutes: 10 });
                      onRefresh();
                      onToast("Snoozed 10 min");
                    }}
                  >
                    <Clock size={12} /> +10m
                  </button>
                  <button
                    className={`btn ${r.isDone ? "" : "btn-primary"}`}
                    onClick={async () => {
                      await invoke("set_reminder_done", { id: r.id, done: !r.isDone });
                      onRefresh();
                    }}
                  >
                    <Check size={12} />
                  </button>
                  <button
                    className="btn"
                    onClick={async () => {
                      await invoke("delete_reminder", { id: r.id });
                      onRefresh();
                    }}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
