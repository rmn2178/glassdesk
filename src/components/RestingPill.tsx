import React, { useEffect, useState } from "react";
import {
  AppConfig,
  HardwareStats,
  LiveTilePayload,
  MediaInfo,
  PrivacySensors,
} from "../types";

interface Props {
  config: AppConfig;
  hw: HardwareStats;
  media: MediaInfo;
  sensors: PrivacySensors;
  liveTiles: LiveTilePayload[];
  isGameStrip: boolean;
}

export const RestingPill: React.FC<Props> = ({
  config,
  hw,
  media,
  sensors,
  liveTiles,
  isGameStrip,
}) => {
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(id);
  }, []);

  if (isGameStrip) {
    return (
      <div className="resting-pill mono" style={{ fontSize: 11 }}>
        <span>CPU {hw.cpuPercent.toFixed(0)}%</span>
        <span>
          RAM {hw.ramUsedGb.toFixed(1)}/{hw.ramTotalGb.toFixed(0)}G
        </span>
        <span>DN {hw.netDownMbps.toFixed(1)}M</span>
        <span>UP {hw.netUpMbps.toFixed(1)}M</span>
      </div>
    );
  }

  const timeStr = now.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: !config.clock24h,
  });

  const activeTile = liveTiles[0];
  const micActive = sensors.micActiveApps.length > 0;
  const camActive = sensors.camActiveApps.length > 0;

  return (
    <div className="resting-pill">
      <div className="pill-left">
        {micActive && <span className="sensor-dot mic" title="Microphone in use" />}
        {camActive && <span className="sensor-dot cam" title="Camera in use" />}

        {activeTile ? (
          <span className="pill-title">
            {activeTile.label}: {activeTile.value}
          </span>
        ) : media.isPlaying && media.title ? (
          <>
            <div className="mini-bars">
              <span />
              <span />
              <span />
            </div>
            <span className="pill-title">{media.title}</span>
          </>
        ) : config.restingFace === "stats" ? (
          <span className="mono">
            CPU {hw.cpuPercent.toFixed(0)}% · RAM {hw.ramPercent.toFixed(0)}%
          </span>
        ) : config.restingFace === "pet" ? (
          <span className="mono">(=^･ω･^=) {timeStr}</span>
        ) : (
          <span className="mono">{timeStr}</span>
        )}
      </div>

      <div className="pill-right">
        <span className="muted mono">{hw.cpuPercent.toFixed(0)}%</span>
      </div>
    </div>
  );
};
