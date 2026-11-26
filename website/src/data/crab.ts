/**
 * The crab, cell for cell as the app draws it (`apps/dock/src/mascot.rs`):
 * the same sprites, colours, faces, emotes and acts, so the page shows the
 * mascot users will meet on their desktop.
 */

/** Sprite cells. `.` is transparent. */
export const crabColors: Record<string, string> = {
  k: "#2b1412",
  e: "#2b1412",
  o: "#e87440",
  h: "#ffd6a0",
  d: "#cc542c",
  p: "#f48c8c",
  m: "#e2463c",
  b: "#46a0eb",
  B: "#aad7fa",
  r: "#ec4848",
  R: "#ff9696",
  z: "#ebeef5",
};

export const CRAB_WIDTH = 22;

/** The body, 22 cells wide, without a face. */
export const crabBody = [
  "....kkkkkkkkkkkkkk....",
  "...khhooooooooooook...",
  "...khoooooooooooook...",
  "...kooooooooooooook...",
  "...kooooooooooooook...",
  "kkkkooooooooooooookkkk",
  "kooooooooooooooooooook",
  "kooooooooooooooooooook",
  "kkkkooooooooooooookkkk",
  "...kooooooooooooook...",
  "...kooooooooooooook...",
  "...kddddddddddddddk...",
];

/** The legs standing, and mid-step (the first and third lifted). */
export const crabLegs = [
  ["...kddkkddkkddkkddk...", "...kddkkddkkddkkddk...", "...kkkkkkkkkkkkkkkk..."],
  ["...kddkkddkkddkkddk...", "...kkkkkddkkkkkkddk...", ".......kkkk....kkkk..."],
];

export type CrabFace = "neutral" | "happy" | "love" | "sad" | "sleep";

/** (row, column, cell) drawn over the body. */
export const crabFaces: Record<CrabFace, [number, number, string][]> = {
  neutral: [
    [5, 7, "e"], [6, 7, "e"], [7, 7, "e"],
    [5, 14, "e"], [6, 14, "e"], [7, 14, "e"],
  ],
  happy: [
    [6, 6, "e"], [5, 7, "e"], [6, 8, "e"],
    [6, 13, "e"], [5, 14, "e"], [6, 15, "e"],
    [7, 10, "e"], [7, 11, "e"], [8, 10, "m"], [8, 11, "m"],
  ],
  love: [
    [5, 6, "e"], [6, 7, "e"], [7, 6, "e"],
    [5, 15, "e"], [6, 14, "e"], [7, 15, "e"],
    [7, 4, "p"], [7, 5, "p"], [7, 16, "p"], [7, 17, "p"],
  ],
  sad: [
    [6, 6, "e"], [7, 7, "e"], [6, 8, "e"],
    [6, 13, "e"], [7, 14, "e"], [6, 15, "e"],
    [8, 15, "b"], [9, 15, "b"],
  ],
  sleep: [
    [6, 6, "e"], [6, 7, "e"], [6, 8, "e"],
    [6, 13, "e"], [6, 14, "e"], [6, 15, "e"],
    [8, 10, "m"], [8, 11, "m"],
  ],
};

export type CrabEmote = "heart" | "drop" | "sleep";

export const crabEmotes: Record<CrabEmote, string[]> = {
  heart: [".kk.kk.", "krRkrrk", "krrrrrk", ".krrrk.", "..krk..", "...k..."],
  drop: ["..k..", ".kbk.", "kbBbk", "kbbbk", ".kkk."],
  // Outlined when drawn, so it reads on any background (as in the app).
  sleep: ["....zzzz", "......z.", "zzz..z..", "..z.zzzz", ".z......", "zzz....."],
};

/** Why the crab comes out. */
export type CrabAct = "fresh" | "nervous" | "exhausted" | "charging" | "sleepy";

/**
 * One act, as `Script::new` in the app: times in seconds, positions as a
 * share of the stage the crab walks on (0 = left edge, 1 = right edge),
 * hop heights in cells.
 */
export interface CrabScript {
  seconds: number;
  path: [number, number][];
  hops: [number, number][];
  shake?: [number, number];
  faces: [number, CrabFace][];
  walking: [number, number][];
  emote?: [CrabEmote, number, number];
}

export const crabScripts: Record<CrabAct, CrabScript> = {
  fresh: {
    seconds: 3.0,
    path: [[0, 0.5]],
    hops: [[0.35, 5], [0.85, 4], [1.55, 6]],
    faces: [[0, "happy"], [1.5, "love"]],
    walking: [],
    emote: ["heart", 1.5, 2.65],
  },
  nervous: {
    seconds: 2.8,
    path: [[0, 0.5]],
    hops: [],
    shake: [0.35, 2.2],
    faces: [[0, "neutral"], [0.35, "sad"]],
    walking: [],
    emote: ["drop", 0.4, 2.4],
  },
  exhausted: {
    seconds: 3.6,
    path: [[0, 0.5]],
    hops: [[0.3, 2]],
    faces: [[0, "sad"], [1.0, "sleep"]],
    walking: [],
    emote: ["sleep", 1.1, 3.25],
  },
  // Scurries in from the left, a happy hop halfway, and runs off to the right.
  charging: {
    seconds: 4.6,
    path: [[0, 0], [1.9, 0.55], [2.9, 0.55], [4.6, 1]],
    hops: [[2.0, 6], [3.0, 3]],
    faces: [[0, "happy"], [2.0, "love"], [2.9, "happy"]],
    walking: [[0, 1.9], [2.9, 4.6]],
    emote: ["heart", 2.05, 3.0],
  },
  // Shuffles in slowly from the right, stops and falls asleep.
  sleepy: {
    seconds: 5.4,
    path: [[0, 1], [2.6, 0.62], [5.4, 0.62]],
    hops: [],
    faces: [[0, "neutral"], [2.7, "sleep"]],
    walking: [[0, 2.6]],
    emote: ["sleep", 2.9, 5.1],
  },
};

/** The moments the crab comes out for, in the order the section shows them. */
export interface CrabMoment {
  act: CrabAct;
  title: string;
  body: string;
  /** Under the menu bar's AI island, or walking on the dock. */
  place: "island" | "dock";
  /** What the island shows when it happens. */
  usage?: number;
  battery?: "charging" | "saver";
}

export const crabMoments: CrabMoment[] = [
  {
    act: "fresh",
    title: "Your AI limit resets",
    body: "Back to 0 %. It hops out under the provider with a heart.",
    place: "island",
    usage: 0,
  },
  {
    act: "nervous",
    title: "Past 90 %",
    body: "It trembles, with a drop of sweat. Time to pace yourself.",
    place: "island",
    usage: 92,
  },
  {
    act: "exhausted",
    title: "Limit reached",
    body: "At 100 % it runs out of steam and dozes off.",
    place: "island",
    usage: 100,
  },
  {
    act: "charging",
    title: "Charger plugged in",
    body: "It scurries along the dock, happy to be powered up.",
    place: "dock",
    battery: "charging",
  },
  {
    act: "sleepy",
    title: "Energy saver on",
    body: "It shuffles onto the dock and takes a nap.",
    place: "dock",
    battery: "saver",
  },
];

export const crabIntro = {
  eyebrow: "Meet the crab",
  title: "A little friend for the moments that matter.",
  lead: "A pixel crab comes out only when something worth it happens, plays a short scene and leaves. The rest of the time it costs nothing. Pick a moment, or hover over it.",
} as const;

export const crabFacts = [
  "Never all the time: only on moments like these, the battery and the network too.",
  "Hover over it and it reacts.",
  "Runs in the system compositor; zero cost while away.",
  "Settings → Dock → Mascot on special moments.",
] as const;

/** The mascots to pick from, as Settings names them. */
export const mascotChoices = ["crab", "cat", "fox", "capybara", "mochi", "girl"] as const;
