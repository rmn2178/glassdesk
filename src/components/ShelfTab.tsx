import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderUp, QrCode, Trash2, X, Copy } from "lucide-react";
import { PhoneTransferSession, StagedFile } from "../types";

interface Props {
  files: StagedFile[];
  onAddPath: (path: string) => void;
  onRemove: (id: string) => void;
  onClear: () => void;
  onToast: (msg: string) => void;
}

export const ShelfTab: React.FC<Props> = ({
  files,
  onAddPath,
  onRemove,
  onClear,
  onToast,
}) => {
  const [manualPath, setManualPath] = useState("");
  const [transfer, setTransfer] = useState<PhoneTransferSession | null>(null);

  const startPhoneTransfer = async () => {
    try {
      const session = await invoke<PhoneTransferSession>("start_phone_transfer", {
        filePaths: files.map((f) => f.path),
      });
      setTransfer(session);
    } catch (err) {
      onToast(String(err));
    }
  };

  const stopTransfer = async () => {
    await invoke("stop_phone_transfer");
    setTransfer(null);
  };

  const addManual = () => {
    if (!manualPath.trim()) return;
    onAddPath(manualPath.trim().replace(/^"|"$/g, ""));
    setManualPath("");
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div className="row-between">
        <div className="row" style={{ flex: 1 }}>
          <input
            className="input"
            placeholder="Drop files onto notch or paste a file path (max 12)..."
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addManual()}
          />
          <button className="btn" onClick={addManual}>
            <FolderUp size={14} /> Add
          </button>
        </div>

        {transfer?.active ? (
          <button className="btn btn-primary" onClick={stopTransfer}>
            <X size={14} /> Stop QR
          </button>
        ) : (
          <button
            className="btn btn-primary"
            disabled={files.length === 0}
            onClick={startPhoneTransfer}
          >
            <QrCode size={14} /> Send to Phone
          </button>
        )}
      </div>

      {transfer?.active ? (
        <div className="glass-card row" style={{ gap: 14 }}>
          <div
            className="qr-box"
            dangerouslySetInnerHTML={{ __html: transfer.qrSvg }}
          />
          <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
            <div style={{ fontWeight: 600, fontSize: 13 }}>
              Scan on Phone ({transfer.fileCount} files)
            </div>
            <div className="muted">
              Same Wi-Fi required. Zero app or account needed. Ephemeral server stops the instant you close this view.
            </div>
            <div className="mono" style={{ fontSize: 11, wordBreak: "break-all", opacity: 0.85 }}>
              {transfer.url}
            </div>
            <div className="row">
              <button
                className="btn"
                onClick={async () => {
                  await invoke("copy_clipboard_text", { text: transfer.url });
                  onToast("Transfer link copied");
                }}
              >
                <Copy size={13} /> Copy URL
              </button>
              <button className="btn" onClick={stopTransfer}>
                Close Server
              </button>
            </div>
          </div>
        </div>
      ) : files.length === 0 ? (
        <div
          className="glass-card"
          style={{ textAlign: "center", padding: "28px 12px" }}
        >
          <div style={{ fontWeight: 600, marginBottom: 4 }}>File Shelf is Empty</div>
          <div className="muted">
            Drag and drop up to 12 files from File Explorer onto the island to hold them temporarily or send them to your phone via QR.
          </div>
        </div>
      ) : (
        <>
          <div className="grid-2">
            {files.map((f) => (
              <div key={f.id} className="glass-card row-between" style={{ padding: "8px 10px" }}>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ fontSize: 12, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {f.name}
                  </div>
                  <div className="muted mono" style={{ fontSize: 10, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {f.path}
                  </div>
                </div>
                <div className="row" style={{ gap: 4 }}>
                  <button
                    className="btn"
                    style={{ padding: "4px 6px" }}
                    title="Copy path"
                    onClick={async () => {
                      await invoke("copy_clipboard_text", { text: f.path });
                      onToast("Copied path");
                    }}
                  >
                    <Copy size={12} />
                  </button>
                  <button
                    className="btn"
                    style={{ padding: "4px 6px" }}
                    title="Remove"
                    onClick={() => onRemove(f.id)}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
          <div className="row-between">
            <span className="muted">{files.length} / 12 staged</span>
            <button className="btn" onClick={onClear}>
              Clear Shelf
            </button>
          </div>
        </>
      )}
    </div>
  );
};
