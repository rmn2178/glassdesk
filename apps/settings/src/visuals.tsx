import type { CSSProperties, ReactNode } from "react";
import type { Appearance, DockDensity, DockPosition, GenieSpeed, IslandStyle, MascotCharacter } from "./config";
import { MASCOT_PALETTE, MASCOT_SPRITES } from "./mascots";

const PATHS = {
  size: <><rect x="3" y="9" width="6" height="6" rx="1.5" /><rect x="13" y="5" width="9" height="14" rx="2" /></>,
  magnification: <><circle cx="10" cy="10" r="6" /><path d="m15 15 5 5M7 10h6m-3-3v6" /></>,
  hide: <><path d="M3 9V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v4M3 15v4a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-4M8 12h8m-3-3 3 3-3 3" /></>,
  bounce: <><rect x="8" y="9" width="8" height="8" rx="2" /><path d="m9 5 3-3 3 3M12 2v5M6 21h12" /></>,
  mascot: <><path d="M6 13a6 6 0 0 0 12 0v-2H6zM8 11V7m8 4V7M4 9l-2-3m18 3 2-3M6 13H3m15 0h3M6 16l-3 3m15-3 3 3" /><circle cx="8" cy="6" r="1" /><circle cx="16" cy="6" r="1" /></>,
  indicators: <><rect x="3" y="4" width="7" height="10" rx="2" /><rect x="14" y="4" width="7" height="10" rx="2" /><circle cx="6.5" cy="19" r="1" fill="currentColor" /><circle cx="17.5" cy="19" r="1" fill="currentColor" /></>,
  recents: <><path d="M4 10a8 8 0 1 1 1 8M4 4v6h6m2-3v5l3 2" /></>,
  jumpList: <><rect x="4" y="3" width="16" height="18" rx="2" /><path d="M8 7h5M8 11h8M8 14.5h8M8 18h8" /></>,
  folder: <path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" />,
  windows: <><rect x="3" y="3" width="13" height="11" rx="2" /><path d="M8 17v2a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-9a2 2 0 0 0-2-2" /><path d="M6 7h7" /></>,
  genie: <><path d="M4 4h16v9c0 4-8 3-8 7M4 4v9c0 4 8 3 8 7M9 21h6M7 8h10" /></>,
  menu: <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 9h18m-5-3h2M6 6h1" /></>,
  music: <><path d="M9 17V5l12-2v12M9 9l12-2" /><ellipse cx="6" cy="18" rx="3" ry="2" /><ellipse cx="18" cy="16" rx="3" ry="2" /></>,
  ai: <><path d="m12 2 2.6 7.4L22 12l-7.4 2.6L12 22l-2.6-7.4L2 12l7.4-2.6z" /></>,
  headphones: <><path d="M4 14v-3a8 8 0 0 1 16 0v3" /><rect x="3" y="12" width="4" height="8" rx="2" /><rect x="17" y="12" width="4" height="8" rx="2" /></>,
  start: <><path d="M12 3v9M6 5a9 9 0 1 0 12 0" /></>,
  language: <><circle cx="12" cy="12" r="10" /><ellipse cx="12" cy="12" rx="4" ry="10" /><path d="M2 12h20" /></>,
  updates: <><path d="M20 8a8 8 0 0 0-14-3L3 8m0-5v5h5m-4 8a8 8 0 0 0 14 3l3-3m-5 0h5v5" /></>,
  reset: <><path d="M3 10a9 9 0 1 1 1 8M3 4v6h6" /></>,
  star: <path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1 6.2-5.5-2.9-5.5 2.9 1-6.2L3 9.6l6.2-.9z" />,
  external: <><path d="M14 3h7v7m0-7L10 14M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" /></>,
} satisfies Record<string, ReactNode>;

export function SettingIcon({ name, size = 18 }: { name: keyof typeof PATHS; size?: number }) {
  return <svg viewBox="0 0 24 24" width={size} height={size} fill={name === "star" ? "#f5c344" : "none"} stroke={name === "star" ? "#d69b16" : "currentColor"} strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{PATHS[name]}</svg>;
}

const MINI_ICONS = <><i /><i /><i /><i /><i /></>;

const DENSITY_PREVIEW_SPACING = {
  compact: { gap: 3, padding: 2.25 },
  normal: { gap: 4.5, padding: 3.375 },
  spacious: { gap: 6, padding: 4.5 },
} satisfies Record<DockDensity, CSSProperties>;

/** A closer view for comparing spacing, independent of the desktop layout. */
export function DockDensityPreview({ density }: { density: DockDensity }) {
  return <span className="preview-density">
    <span className="preview-dock" data-density={density}
      style={DENSITY_PREVIEW_SPACING[density]}>{MINI_ICONS}</span>
  </span>;
}

/** Illustrations only: no window capture, network, timers or animation loop. */
export function DockPreview({ position, iconSize, wallpaper, density = "normal" }: { position: DockPosition; iconSize: number; wallpaper: string | null; density?: DockDensity }) {
  return <span className="preview-screen">
    {wallpaper ? <img className="preview-wallpaper" src={wallpaper} alt="" /> : null}
    <span className="preview-topbar"><i /><i /><i /></span>
    <span className="preview-window"><i /><i /><i /></span>
    <span className="preview-dock" data-position={position} data-density={density}
      style={{ "--mini-icon": `${6 + (iconSize - 32) / 16}px` } as CSSProperties}>{MINI_ICONS}</span>
  </span>;
}

type Theme = Exclude<Appearance, "auto">;

/** One theme's bar and dock over the wallpaper. Each layer carries its own
 * wallpaper: a clipped layer cannot blur what lies outside it. */
function ThemeLayer({ theme, wallpaper, half, dark }: { theme: Theme; wallpaper: string | null; half?: boolean; dark?: boolean }) {
  return <span className="theme-layer" data-theme={theme} data-half={half ? "" : undefined} data-dark={dark ? "" : undefined}>
    {wallpaper ? <img className="preview-wallpaper" src={wallpaper} alt="" /> : null}
    <span className="theme-bar"><i /><i /><i /><b /></span>
    <span className="theme-dock">{MINI_ICONS}</span>
  </span>;
}

/** CSS material previews. Adaptive choices show both Windows palettes. */
export function ThemePreview({ appearance, wallpaper }: { appearance: Appearance; wallpaper: string | null }) {
  return <span className="preview-screen preview-theme">
    {appearance === "auto"
      ? <><ThemeLayer theme="light" wallpaper={wallpaper} /><ThemeLayer theme="dark" wallpaper={wallpaper} half /></>
      : appearance === "frosted" || appearance === "liquidGlass"
      ? <><ThemeLayer theme={appearance} wallpaper={wallpaper} /><ThemeLayer theme={appearance} wallpaper={wallpaper} half dark /></>
      : <ThemeLayer theme={appearance} wallpaper={wallpaper} />}
  </span>;
}

/** Preview pixels per sprite cell: every mascot at the same scale, as in the dock. */
const MASCOT_CELL = 2.8;

/** The dock's own mascot sprite, cell for cell, over a backdrop of its own world. */
export function MascotPreview({ character }: { character: MascotCharacter }) {
  const rows = MASCOT_SPRITES[character];
  const width = rows[0].length;
  // One rect per run of equal cells keeps the SVG small.
  const runs: { x: number; y: number; length: number; fill: string }[] = [];
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length;) {
      let end = x + 1;
      while (end < row.length && row[end] === row[x]) end += 1;
      const fill = MASCOT_PALETTE[row[x]];
      if (fill) runs.push({ x, y, length: end - x, fill });
      x = end;
    }
  });
  return <span className="preview-screen preview-screen-mascot" data-character={character}>
    <svg className="preview-mascot" viewBox={`0 0 ${width} ${rows.length}`} shapeRendering="crispEdges"
      style={{ height: `${rows.length * MASCOT_CELL}px` }}>
      {runs.map((run) => <rect key={`${run.x}-${run.y}`} x={run.x} y={run.y} width={run.length} height={1} fill={run.fill} />)}
    </svg>
  </span>;
}

export function IslandPreview({ style, wallpaper }: { style: IslandStyle; wallpaper: string | null }) {
  return <span className="preview-screen preview-screen-island">
    {wallpaper ? <img className="preview-wallpaper" src={wallpaper} alt="" /> : null}
    <span className="preview-topbar"><i /><i /><i /></span>
    <span className="preview-island" data-style={style}>
      {style === "notch" ? <svg viewBox="0 0 100 24"><path d="M0 0h100c-12 0-8 24-24 24H24C8 24 12 0 0 0" fill="currentColor" /></svg> : null}
      <span className="preview-island-content">
        <span className="preview-album"><svg viewBox="0 0 16 16"><path d="M6 11V4l6-1v7M6 6l6-1" /><ellipse cx="4" cy="12" rx="2" ry="1.5" /><ellipse cx="10" cy="11" rx="2" ry="1.5" /></svg></span>
        <span className="preview-song"><i /><i /></span>
        <span className="preview-wave"><i /><i /><i /></span>
      </span>
    </span>
    <span className="preview-window"><i /><i /><i /></span>
  </span>;
}

export function SpeedPreview({ speed }: { speed: GenieSpeed }) {
  return <svg className="preview-speed" viewBox="0 0 120 48" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <rect x="20" y="4" width="36" height="24" rx="4" opacity=".65" /><path d="M20 11h36" opacity=".35" />
    <path d="M63 16c17 0 7 23 23 23m-5-4 5 4-5 3" stroke="var(--accent)" />
    {speed !== "normal" ? <path d="M12 12H5m7 6H2m10 6H5" opacity=".4" /> : null}
    {speed === "faster" ? <path d="M8 6H3m5 24H3" opacity=".25" /> : null}
    <rect x="88" y="34" width="16" height="10" rx="2" fill="var(--accent)" stroke="none" />
  </svg>;
}
