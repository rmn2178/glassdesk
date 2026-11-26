import {
  CRAB_WIDTH,
  crabBody,
  crabColors,
  crabEmotes,
  crabFaces,
  crabLegs,
  type CrabEmote,
  type CrabFace,
} from "@/data/crab";

/** One horizontal run of same-coloured cells: few rects, crisp at any size. */
export interface CellRun {
  x: number;
  y: number;
  w: number;
  fill: string;
}

/** Runs of a sprite grid, `top` rows down. `.` cells are left out. */
export function runs(grid: string[], top = 0): CellRun[] {
  const out: CellRun[] = [];
  grid.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const cell = row[x];
      let end = x + 1;
      while (end < row.length && row[end] === cell) end += 1;
      const fill = crabColors[cell];
      if (fill) out.push({ x, y: y + top, w: end - x, fill });
      x = end;
    }
  });
  return out;
}

/** `grid` with a one-cell dark outline around what is drawn (the app's `outlined`). */
export function outlined(grid: string[]): string[] {
  const height = grid.length + 2;
  const width = Math.max(...grid.map((row) => row.length)) + 2;
  const drawn = (y: number, x: number) => {
    const cell = grid[y - 1]?.[x - 1];
    return cell !== undefined && cell !== ".";
  };
  return Array.from({ length: height }, (_, y) =>
    Array.from({ length: width }, (_, x) => {
      if (drawn(y, x)) return grid[y - 1][x - 1];
      for (let ny = y - 1; ny <= y + 1; ny += 1)
        for (let nx = x - 1; nx <= x + 1; nx += 1) if (drawn(ny, nx)) return "k";
      return ".";
    }).join(""),
  );
}

export const bodyRuns = runs(crabBody);
export const legRuns = crabLegs.map((legs) => runs(legs, crabBody.length));
export const CRAB_HEIGHT = crabBody.length + crabLegs[0].length;
export { CRAB_WIDTH };

export const faceRuns = Object.fromEntries(
  Object.entries(crabFaces).map(([face, marks]) => [
    face,
    marks.map(([y, x, cell]) => ({ x, y, w: 1, fill: crabColors[cell] })),
  ]),
) as Record<CrabFace, CellRun[]>;

export const emoteSprites = Object.fromEntries(
  Object.entries(crabEmotes).map(([name, rows]) => {
    const grid = name === "sleep" ? outlined(rows) : rows;
    return [name, { width: Math.max(...grid.map((r) => r.length)), height: grid.length, runs: runs(grid) }];
  }),
) as Record<CrabEmote, { width: number; height: number; runs: CellRun[] }>;
