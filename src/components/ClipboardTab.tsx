import React, { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pin, Trash2, Copy, Search } from "lucide-react";
import { ClipboardItem } from "../types";

interface Props {
  items: ClipboardItem[];
  onRefresh: () => void;
  onToast: (msg: string) => void;
}

export const ClipboardTab: React.FC<Props> = ({ items, onRefresh, onToast }) => {
  const [filter, setFilter] = useState<"all" | "text" | "link" | "file">("all");
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    return items.filter((item) => {
      if (filter !== "all" && item.kind !== filter) return false;
      if (query.trim() && !item.content.toLowerCase().includes(query.toLowerCase())) {
        return false;
      }
      return true;
    });
  }, [items, filter, query]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="row-between">
        <div className="row" style={{ gap: 4 }}>
          {(["all", "text", "link", "file"] as const).map((k) => (
            <button
              key={k}
              className={`btn ${filter === k ? "btn-primary" : ""}`}
              style={{ padding: "4px 9px", textTransform: "capitalize" }}
              onClick={() => setFilter(k)}
            >
              {k}
            </button>
          ))}
        </div>
        <div className="row" style={{ width: 210 }}>
          <Search size={13} className="muted" />
          <input
            className="input"
            placeholder="Search clipboard..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <button
          className="btn"
          onClick={async () => {
            await invoke("clear_unpinned_clipboard");
            onRefresh();
            onToast("Cleared unpinned clipboard");
          }}
        >
          Clear
        </button>
      </div>

      {filtered.length === 0 ? (
        <div className="glass-card muted" style={{ textAlign: "center", padding: 24 }}>
          No clipboard entries yet. Copy any text, URL, or file path to capture it automatically.
        </div>
      ) : (
        <div className="grid-2">
          {filtered.map((item) => (
            <div
              key={item.id}
              className="glass-card"
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                cursor: "pointer",
              }}
              onClick={async () => {
                await invoke("copy_clipboard_text", { text: item.content });
                onToast("Copied to clipboard");
              }}
            >
              <div className="row-between">
                <span className="badge" style={{ textTransform: "uppercase" }}>
                  {item.kind}
                </span>
                <div className="row" style={{ gap: 4 }} onClick={(e) => e.stopPropagation()}>
                  <button
                    className={`btn ${item.isPinned ? "btn-primary" : ""}`}
                    style={{ padding: "3px 5px" }}
                    onClick={async () => {
                      await invoke("toggle_clipboard_pin", { id: item.id });
                      onRefresh();
                    }}
                    title="Pin"
                  >
                    <Pin size={11} />
                  </button>
                  <button
                    className="btn"
                    style={{ padding: "3px 5px" }}
                    onClick={async () => {
                      await invoke("copy_clipboard_text", { text: item.content });
                      onToast("Copied");
                    }}
                    title="Copy"
                  >
                    <Copy size={11} />
                  </button>
                  <button
                    className="btn"
                    style={{ padding: "3px 5px" }}
                    onClick={async () => {
                      await invoke("delete_clipboard_item", { id: item.id });
                      onRefresh();
                    }}
                    title="Delete"
                  >
                    <Trash2 size={11} />
                  </button>
                </div>
              </div>
              <div
                style={{
                  fontSize: 12,
                  overflow: "hidden",
                  display: "-webkit-box",
                  WebkitLineClamp: 2,
                  WebkitBoxOrient: "vertical",
                  wordBreak: "break-word",
                }}
              >
                {item.content}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
