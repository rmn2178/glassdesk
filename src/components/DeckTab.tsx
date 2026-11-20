import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Smartphone, Check, X, Power } from "lucide-react";
import { PcDeckStatus } from "../types";

interface Props {
  status: PcDeckStatus;
  onUpdate: (s: PcDeckStatus) => void;
  onToast: (msg: string) => void;
}

export const DeckTab: React.FC<Props> = ({ status, onUpdate, onToast }) => {
  const start = async () => {
    try {
      const s = await invoke<PcDeckStatus>("start_pc_deck");
      onUpdate(s);
    } catch (e) {
      onToast(String(e));
    }
  };

  const stop = async () => {
    await invoke("stop_pc_deck");
    onUpdate({
      active: false,
      url: "",
      qrSvg: "",
      expiresAt: 0,
      pendingDevice: null,
      pairedDevice: null,
    });
  };

  const approve = async (allow: boolean) => {
    const s = await invoke<PcDeckStatus>("approve_pc_deck", { allow });
    onUpdate(s);
    onToast(allow ? "Phone approved for PC Deck" : "Pairing denied");
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      {!status.active ? (
        <div className="glass-card" style={{ textAlign: "center", padding: 24 }}>
          <Smartphone size={24} style={{ marginBottom: 6 }} />
          <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 4 }}>
            PC Deck — Smartphone Browser Remote
          </div>
          <div className="muted" style={{ marginBottom: 12 }}>
            Control cursor, keyboard, media, and slides from your phone over local Wi-Fi. Off by default with 2-minute QR pairing and PC approval.
          </div>
          <button className="btn btn-primary" onClick={start}>
            Start PC Deck & Generate QR
          </button>
        </div>
      ) : (
        <div className="glass-card row" style={{ gap: 14 }}>
          <div
            className="qr-box"
            dangerouslySetInnerHTML={{ __html: status.qrSvg }}
          />
          <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
            <div className="row-between">
              <span style={{ fontWeight: 600, fontSize: 13 }}>PC Deck Active</span>
              <button className="btn" onClick={stop}>
                <Power size={12} /> Stop
              </button>
            </div>
            <div className="mono muted" style={{ fontSize: 10, wordBreak: "break-all" }}>
              {status.url}
            </div>

            {status.pendingDevice && !status.pairedDevice && (
              <div className="glass-card row-between" style={{ padding: 8 }}>
                <span style={{ fontSize: 12 }}>Approve {status.pendingDevice}?</span>
                <div className="row" style={{ gap: 5 }}>
                  <button className="btn btn-primary" onClick={() => approve(true)}>
                    <Check size={12} /> Allow
                  </button>
                  <button className="btn" onClick={() => approve(false)}>
                    <X size={12} /> Deny
                  </button>
                </div>
              </div>
            )}

            {status.pairedDevice && (
              <div className="badge" style={{ alignSelf: "flex-start" }}>
                Connected: {status.pairedDevice}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
