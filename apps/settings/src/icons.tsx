/**
 * Sidebar icons in the style of macOS System Settings: a white glyph on a
 * rounded tile of the section's colour. Static markup, so it lives outside
 * the components that use it.
 */

import type { ReactNode } from "react";

export type SectionId = "appearance" | "dock" | "windows" | "menubar" | "system";

const TILES: Record<SectionId, [string, string]> = {
  appearance: ["#5c5c62", "#1c1c1e"],
  dock: ["#4aa3ff", "#0a6cf0"],
  windows: ["#b57bff", "#7d3ff0"],
  menubar: ["#ff9f45", "#f0640a"],
  system: ["#a1a1a6", "#6e6e73"],
};

const GLYPHS: Record<SectionId, ReactNode> = {
  // A circle half light, half dark, as macOS draws Appearance.
  appearance: (
    <>
      <circle cx="12" cy="12" r="6.6" fill="none" stroke="#fff" strokeWidth="1.6" />
      <path d="M12 5.4a6.6 6.6 0 0 1 0 13.2z" fill="#fff" />
    </>
  ),
  // A dock: three icons on a shelf.
  dock: (
    <>
      <rect x="4" y="14.5" width="16" height="5" rx="1.6" fill="#fff" opacity="0.55" />
      <rect x="6" y="10.5" width="3.4" height="3.4" rx="0.9" fill="#fff" />
      <rect x="10.3" y="10.5" width="3.4" height="3.4" rx="0.9" fill="#fff" />
      <rect x="14.6" y="10.5" width="3.4" height="3.4" rx="0.9" fill="#fff" />
    </>
  ),
  // Two overlapping windows.
  windows: (
    <>
      <rect x="4.5" y="5.5" width="11" height="8.5" rx="1.4" fill="#fff" opacity="0.55" />
      <rect x="8.5" y="9.5" width="11" height="9" rx="1.4" fill="#fff" />
      <rect x="8.5" y="9.5" width="11" height="2.2" rx="1" fill="#000" opacity="0.18" />
    </>
  ),
  // A bar across the top of a screen.
  menubar: (
    <>
      <rect x="4" y="5" width="16" height="14" rx="2" fill="#fff" opacity="0.55" />
      <rect x="4" y="5" width="16" height="3.6" rx="1.6" fill="#fff" />
      <circle cx="16.8" cy="6.8" r="0.9" fill="#f0640a" />
    </>
  ),
  // A gear.
  system: (
    <path
      fill="#fff"
      fillRule="evenodd"
      d="M10.6 3.5h2.8l.4 2.1c.5.2 1 .4 1.4.8l2-.8 1.4 2.4-1.6 1.4a6 6 0 0 1 0 1.6l1.6 1.4-1.4 2.4-2-.8c-.4.3-.9.6-1.4.8l-.4 2.1h-2.8l-.4-2.1c-.5-.2-1-.5-1.4-.8l-2 .8-1.4-2.4 1.6-1.4a6 6 0 0 1 0-1.6L5.4 8l1.4-2.4 2 .8c.4-.4.9-.6 1.4-.8zM12 9.4a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2z"
      transform="translate(0 0.5)"
    />
  ),
};

export function SectionIcon({ section }: { section: SectionId }) {
  const [top, bottom] = TILES[section];
  const gradient = `tile-${section}`;
  return (
    <svg className="nav-icon" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={top} />
          <stop offset="1" stopColor={bottom} />
        </linearGradient>
      </defs>
      <rect width="24" height="24" rx="5.5" fill={`url(#${gradient})`} />
      {GLYPHS[section]}
    </svg>
  );
}
