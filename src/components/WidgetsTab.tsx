import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CloudSun, Plus, Trash2, Play, RotateCcw, Check } from "lucide-react";
import { AppConfig, HardwareStats, TodoItem } from "../types";

interface Props {
  config: AppConfig;
  hw: HardwareStats;
  todos: TodoItem[];
  onRefreshTodos: () => void;
  onToast: (msg: string) => void;
}

export const WidgetsTab: React.FC<Props> = ({
  config,
  hw,
  todos,
  onRefreshTodos,
  onToast,
}) => {
  const [newTodo, setNewTodo] = useState("");
  const [pomoSecs, setPomoSecs] = useState(25 * 60);
  const [pomoRunning, setPomoRunning] = useState(false);
  const [weather, setWeather] = useState<{ temp?: number; wind?: number; hum?: number } | null>(null);
  const [calcExpr, setCalcExpr] = useState("");
  const [calcResult, setCalcResult] = useState<string>("");

  useEffect(() => {
    invoke<{ current?: { temperature_2m?: number; wind_speed_10m?: number; relative_humidity_2m?: number } }>(
      "fetch_weather",
      { lat: config.weatherLat, lon: config.weatherLon }
    )
      .then((res) => {
        if (res.current) {
          setWeather({
            temp: res.current.temperature_2m,
            wind: res.current.wind_speed_10m,
            hum: res.current.relative_humidity_2m,
          });
        }
      })
      .catch(() => {});
  }, [config.weatherLat, config.weatherLon]);

  useEffect(() => {
    if (!pomoRunning) return;
    const id = setInterval(() => {
      setPomoSecs((s) => {
        if (s <= 1) {
          setPomoRunning(false);
          onToast("Focus timer completed!");
          return 0;
        }
        return s - 1;
      });
    }, 1000);
    return () => clearInterval(id);
  }, [pomoRunning, onToast]);

  const evalCalc = (expr: string) => {
    setCalcExpr(expr);
    if (!/^[0-9+\-*/().\s]+$/.test(expr) || !expr.trim()) {
      setCalcResult("");
      return;
    }
    try {
      // Safe arithmetic evaluator restricted to digits & operators above
      const val = Function(`"use strict"; return (${expr})`)();
      if (typeof val === "number" && Number.isFinite(val)) {
        setCalcResult(`= ${Math.round(val * 10000) / 10000}`);
      }
    } catch {
      setCalcResult("");
    }
  };

  const now = new Date();
  const dayPct = Math.round(((now.getHours() * 60 + now.getMinutes()) / 1440) * 100);
  const pomoMin = Math.floor(pomoSecs / 60);
  const pomoSec = pomoSecs % 60;

  return (
    <div className="grid-2">
      {/* Weather & Day Progress */}
      <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div className="row-between">
          <span className="row" style={{ fontWeight: 600, fontSize: 12 }}>
            <CloudSun size={15} /> {config.weatherCity}
          </span>
          <span className="mono" style={{ fontSize: 16, fontWeight: 700 }}>
            {weather?.temp !== undefined ? `${Math.round(weather.temp)}°C` : "--°C"}
          </span>
        </div>
        <div className="muted">
          {weather?.hum !== undefined
            ? `Humidity ${weather.hum}% · Wind ${weather.wind} km/h`
            : "Open-Meteo live weather"}
        </div>
        <div style={{ marginTop: 4 }}>
          <div className="row-between muted">
            <span>Day Progress</span>
            <span className="mono">{dayPct}%</span>
          </div>
          <div style={{ height: 4, background: "rgba(255,255,255,0.12)", borderRadius: 4, marginTop: 3 }}>
            <div style={{ width: `${dayPct}%`, height: "100%", background: "var(--accent)", borderRadius: 4 }} />
          </div>
        </div>
      </div>

      {/* Focus Pomodoro Timer */}
      <div className="glass-card" style={{ display: "flex", flexDirection: "column", justifyContent: "space-between", gap: 6 }}>
        <div className="row-between">
          <span style={{ fontWeight: 600, fontSize: 12 }}>Focus Timer</span>
          <span className="mono" style={{ fontSize: 18, fontWeight: 700 }}>
            {String(pomoMin).padStart(2, "0")}:{String(pomoSec).padStart(2, "0")}
          </span>
        </div>
        <div className="row">
          <button
            className="btn btn-primary"
            style={{ flex: 1 }}
            onClick={() => setPomoRunning(!pomoRunning)}
          >
            <Play size={13} /> {pomoRunning ? "Pause" : "Start"}
          </button>
          <button
            className="btn"
            onClick={() => {
              setPomoRunning(false);
              setPomoSecs(25 * 60);
            }}
          >
            <RotateCcw size={13} />
          </button>
        </div>
      </div>

      {/* Live PC Stats + Quick Calculator */}
      <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div style={{ fontWeight: 600, fontSize: 12 }}>PC Telemetry & Calc</div>
        <div className="mono" style={{ fontSize: 11, display: "flex", justifyContent: "space-between" }}>
          <span>CPU {hw.cpuPercent.toFixed(0)}%</span>
          <span>RAM {hw.ramUsedGb.toFixed(1)}G</span>
          <span>↓{hw.netDownMbps.toFixed(1)}M</span>
        </div>
        <div className="row">
          <input
            className="input mono"
            placeholder="Calc e.g. 24*60"
            value={calcExpr}
            onChange={(e) => evalCalc(e.target.value)}
          />
          {calcResult && <span className="mono badge">{calcResult}</span>}
        </div>
      </div>

      {/* To-Do List */}
      <div className="glass-card" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div className="row">
          <input
            className="input"
            placeholder="Add quick task..."
            value={newTodo}
            onChange={(e) => setNewTodo(e.target.value)}
            onKeyDown={async (e) => {
              if (e.key === "Enter" && newTodo.trim()) {
                await invoke("add_todo", { text: newTodo.trim() });
                setNewTodo("");
                onRefreshTodos();
              }
            }}
          />
          <button
            className="btn"
            onClick={async () => {
              if (!newTodo.trim()) return;
              await invoke("add_todo", { text: newTodo.trim() });
              setNewTodo("");
              onRefreshTodos();
            }}
          >
            <Plus size={13} />
          </button>
        </div>
        <div style={{ maxHeight: 74, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
          {todos.map((t) => (
            <div key={t.id} className="row-between" style={{ fontSize: 11 }}>
              <span
                style={{
                  cursor: "pointer",
                  textDecoration: t.completed ? "line-through" : "none",
                  opacity: t.completed ? 0.5 : 1,
                  display: "flex",
                  alignItems: "center",
                  gap: 5,
                }}
                onClick={async () => {
                  await invoke("toggle_todo", { id: t.id });
                  onRefreshTodos();
                }}
              >
                <Check size={12} /> {t.text}
              </span>
              <button
                className="btn"
                style={{ padding: "2px 4px" }}
                onClick={async () => {
                  await invoke("delete_todo", { id: t.id });
                  onRefreshTodos();
                }}
              >
                <Trash2 size={10} />
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
