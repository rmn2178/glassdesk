import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bell, Trash2 } from "lucide-react";
import { NotificationItem } from "../types";

interface Props {
  notifications: NotificationItem[];
  onRefresh: () => void;
}

export const NotificationsTab: React.FC<Props> = ({ notifications, onRefresh }) => {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="row-between">
        <span className="row" style={{ fontSize: 12, fontWeight: 600 }}>
          <Bell size={14} /> Local Notification History
        </span>
        <button
          className="btn"
          onClick={async () => {
            await invoke("clear_notifications");
            onRefresh();
          }}
        >
          <Trash2 size={12} /> Clear All
        </button>
      </div>

      {notifications.length === 0 ? (
        <div className="glass-card muted" style={{ textAlign: "center", padding: 24 }}>
          No notifications recorded yet.
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {notifications.map((n) => (
            <div key={n.id} className="glass-card">
              <div className="row-between">
                <span className="badge">{n.appName}</span>
                <span className="muted mono">
                  {new Date(n.receivedAt * 1000).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </span>
              </div>
              <div style={{ fontSize: 12, fontWeight: 600, marginTop: 4 }}>{n.title}</div>
              {n.body && <div className="muted">{n.body}</div>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
