import React, { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Music, Pause, Play, SkipBack, SkipForward, FileText } from "lucide-react";
import { AppConfig, LyricsResponse, MediaInfo } from "../types";

interface Props {
  media: MediaInfo;
  config: AppConfig;
  onRefresh: () => void;
}

const fmtTime = (secs: number) => {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
};

export const MusicTab: React.FC<Props> = ({ media, config, onRefresh }) => {
  const [showLyrics, setShowLyrics] = useState(false);
  const [lyrics, setLyrics] = useState<LyricsResponse | null>(null);
  const [loadingLyrics, setLoadingLyrics] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // 30fps visualizer canvas (rounded bars or pixel bars)
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    let raf = 0;
    let phase = 0;

    const draw = () => {
      phase += media.isPlaying ? 0.12 : 0.015;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      const bars = 18;
      const barW = canvas.width / bars - 3;
      for (let i = 0; i < bars; i++) {
        const amp = media.isPlaying
          ? 0.25 + 0.72 * Math.abs(Math.sin(phase + i * 0.55) * Math.cos(phase * 0.6 + i * 0.23))
          : 0.14;
        const h = Math.max(4, amp * canvas.height);
        const x = i * (barW + 3);
        const y = canvas.height - h;
        ctx.fillStyle = "rgba(255, 255, 255, 0.85)";
        if (config.visualizerStyle === "pixel") {
          for (let py = canvas.height - 4; py >= y; py -= 5) {
            ctx.fillRect(x, py, barW, 3);
          }
        } else {
          ctx.beginPath();
          ctx.roundRect(x, y, barW, h, 3);
          ctx.fill();
        }
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [media.isPlaying, config.visualizerStyle]);

  const loadLyrics = async () => {
    if (!showLyrics && media.title) {
      setShowLyrics(true);
      setLoadingLyrics(true);
      try {
        const res = await invoke<LyricsResponse>("get_synced_lyrics", {
          artist: media.artist,
          title: media.title,
        });
        setLyrics(res);
      } finally {
        setLoadingLyrics(false);
      }
    } else {
      setShowLyrics(!showLyrics);
    }
  };

  const sendCmd = async (cmd: string) => {
    await invoke("send_media_command", { cmd });
    setTimeout(onRefresh, 180);
  };

  const pct =
    media.durationSecs > 0
      ? Math.min(100, (media.positionSecs / media.durationSecs) * 100)
      : 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div className="glass-card row-between">
        <div className="row" style={{ minWidth: 0, flex: 1 }}>
          <div
            style={{
              width: 54,
              height: 54,
              borderRadius: 12,
              background: "linear-gradient(135deg, rgba(255,255,255,0.2), rgba(255,255,255,0.04))",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Music size={24} />
          </div>
          <div style={{ minWidth: 0, flex: 1 }}>
            <div style={{ fontWeight: 600, fontSize: 14, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
              {media.hasSession && media.title ? media.title : "No Active Media Session"}
            </div>
            <div className="muted" style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
              {media.artist || "Play Spotify, YouTube, Apple Music, or VLC"}
            </div>
            {media.sourceApp && (
              <span className="badge" style={{ marginTop: 4, display: "inline-block" }}>
                {media.sourceApp.split("!").pop()}
              </span>
            )}
          </div>
        </div>

        <canvas ref={canvasRef} width={140} height={38} />
      </div>

      <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div className="row-between mono muted">
          <span>{fmtTime(media.positionSecs)}</span>
          <div style={{ flex: 1, height: 4, background: "rgba(255,255,255,0.12)", borderRadius: 4, overflow: "hidden" }}>
            <div style={{ width: `${pct}%`, height: "100%", background: "var(--accent)", transition: "width 300ms linear" }} />
          </div>
          <span>{fmtTime(media.durationSecs)}</span>
        </div>

        <div className="row-between">
          <div className="row">
            <button className="btn" onClick={() => sendCmd("prev")} title="Previous">
              <SkipBack size={15} />
            </button>
            <button className="btn btn-primary" onClick={() => sendCmd("play_pause")} title="Play / Pause">
              {media.isPlaying ? <Pause size={15} /> : <Play size={15} />}
              {media.isPlaying ? "Pause" : "Play"}
            </button>
            <button className="btn" onClick={() => sendCmd("next")} title="Next">
              <SkipForward size={15} />
            </button>
          </div>
          <button className="btn" onClick={loadLyrics}>
            <FileText size={14} />
            {showLyrics ? "Hide Lyrics" : "Synced Lyrics"}
          </button>
        </div>
      </div>

      {showLyrics && (
        <div className="glass-card" style={{ maxHeight: 105, overflowY: "auto", fontSize: 12 }}>
          {loadingLyrics ? (
            <div className="muted">Fetching synced lyrics from LRCLIB...</div>
          ) : lyrics?.found ? (
            lyrics.synced ? (
              lyrics.lines.map((line, i) => {
                const isActive =
                  media.positionSecs >= line.timeSecs &&
                  (i === lyrics.lines.length - 1 || media.positionSecs < lyrics.lines[i + 1].timeSecs);
                return (
                  <div
                    key={i}
                    style={{
                      padding: "2px 0",
                      opacity: isActive ? 1 : 0.5,
                      fontWeight: isActive ? 600 : 400,
                    }}
                  >
                    {line.text || "♪"}
                  </div>
                );
              })
            ) : (
              <pre style={{ margin: 0, whiteSpace: "pre-wrap", fontFamily: "inherit" }}>
                {lyrics.plain}
              </pre>
            )
          ) : (
            <div className="muted">No lyrics found on LRCLIB for this track.</div>
          )}
        </div>
      )}
    </div>
  );
};
