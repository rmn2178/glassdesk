import { Crab, reactOnClick, watchVisible, type Perch } from "@/scripts/crab";

/** Where it settles on the window's title bar: right of centre, clear of the traffic lights. */
const HOME = 0.82;
/** An icon this much larger than at rest is magnified: the crab moves out of its way. */
const MAGNIFIED = 1.22;
const REST_ICON = 52;
const WATCH_MS = 140;
/** It does not run off again right after running off. */
const DODGE_GAP_MS = 1400;
/** Quiet time between two idle moves. */
const IDLE_MIN_MS = 6500;
const IDLE_SPREAD_MS = 4500;
/** The mockup rises in for about this long before the crab comes out. */
const ENTRANCE_DELAY_MS = 1700;

type Spot = "window" | "dock";

/**
 * The hero's crab. It lives on the Notes window's title bar, hops down onto
 * the dock when the window is minimized with the Genie (and back up when it
 * returns), scurries away from icons that grow under it, reacts to the
 * pointer, and plays an emotion when clicked.
 */
export function initHeroCrab(desk: HTMLElement) {
  const el = desk.querySelector<HTMLElement>("[data-hero-crab]");
  const note = desk.querySelector<HTMLElement>("[data-note]");
  const dock = desk.querySelector<HTMLElement>(".dock");
  if (!el || !note || !dock) return;

  const crab = new Crab(el);
  let spot: Spot = "window";
  let share = HOME;
  let visible = false;
  let started = false;
  let idleTimer = 0;
  let watchTimer = 0;
  let lastInteraction = 0;
  let lastDodge = 0;

  const perch = (which: Spot): Perch => {
    const d = desk.getBoundingClientRect();
    if (which === "window") {
      const r = note.getBoundingClientRect();
      return { left: r.left - d.left + 18, right: r.right - d.left - 18, y: r.top - d.top + 1 };
    }
    const r = dock.getBoundingClientRect();
    // On phones the dock scrolls wider than the desk: stay on the visible part.
    return {
      left: Math.max(r.left - d.left + 22, 10),
      right: Math.min(r.right - d.left - 22, d.width - 10),
      y: r.top - d.top + 1,
    };
  };

  const settle = () => {
    if (crab.busy) return;
    crab.place(crab.xAt(share, perch(spot)), perch(spot));
  };

  const remember = () => {
    share = crab.shareOn(perch(spot));
  };

  /* ---- Idle life ------------------------------------------------------ */

  const idle = async () => {
    idleTimer = 0;
    if (!visible || crab.busy || performance.now() - lastInteraction < 3000) return scheduleIdle();
    const roll = Math.random();
    const here = perch(spot);
    if (roll < 0.45) {
      // A stroll to another spot of the same edge.
      const target = spot === "window" ? 0.5 + Math.random() * 0.45 : 0.08 + Math.random() * 0.84;
      await crab.walkTo(crab.xAt(target, here));
      remember();
    } else if (roll < 0.75) {
      crab.face("love");
      crab.bounce(5);
      window.setTimeout(() => crab.face(crab.rest), 900);
    } else {
      await crab.play("fresh", undefined, { enter: false, exit: false, here: true });
    }
    scheduleIdle();
  };

  const scheduleIdle = () => {
    window.clearTimeout(idleTimer);
    if (!visible || crab.reduced) return;
    idleTimer = window.setTimeout(() => void idle(), IDLE_MIN_MS + Math.random() * IDLE_SPREAD_MS);
  };

  /* ---- On the dock: out of the way of magnified icons ------------------ */

  /** The centre (page x) of the most magnified icon under the crab, if any is. */
  const growingUnder = () => {
    const r = el.getBoundingClientRect();
    let peak: { centre: number; size: number } | null = null;
    dock.querySelectorAll<HTMLElement>(".dock-item").forEach((item) => {
      const box = item.getBoundingClientRect();
      if (box.right < r.left || box.left > r.right || item.offsetWidth <= REST_ICON * MAGNIFIED) return;
      if (!peak || item.offsetWidth > peak.size) peak = { centre: box.left + box.width / 2, size: item.offsetWidth };
    });
    return peak as { centre: number; size: number } | null;
  };

  const watchDock = () => {
    watchTimer = 0;
    if (spot !== "dock" || !visible) return;
    const growing = growingUnder();
    if (!crab.busy && growing && performance.now() - lastDodge > DODGE_GAP_MS) {
      const here = perch("dock");
      // Away from where the icons grow: the side the magnification is not on.
      const centre = el.getBoundingClientRect().left + crab.width / 2;
      const target = growing.centre > centre ? 0.04 : 0.96;
      lastDodge = performance.now();
      lastInteraction = lastDodge;
      void crab.walkTo(crab.xAt(target, here), "sad").then(remember);
    }
    watchTimer = window.setTimeout(watchDock, WATCH_MS);
  };

  const startWatching = () => {
    if (!watchTimer && spot === "dock" && !crab.reduced) watchTimer = window.setTimeout(watchDock, WATCH_MS);
  };

  /* ---- The Genie: down to the dock, back up to the window -------------- */

  const moveTo = async (next: Spot) => {
    if (spot === next) return;
    const from = perch(spot);
    const centre = crab.x + crab.width / 2 - from.left;
    spot = next;
    const to = perch(next);
    // Lands below (or above) where it was, inside the new edge.
    const x = Math.min(Math.max(to.left + centre - crab.width / 2, to.left), to.right - crab.width);
    await crab.jumpTo(x, to);
    remember();
    crab.bounce(3);
    startWatching();
  };

  window.addEventListener("nosih:genie", (event) => {
    if (!started) return;
    const { flying, minimized } = (event as CustomEvent<{ flying: boolean; minimized: boolean }>).detail;
    if (flying && spot === "window") void moveTo("dock");
    else if (!flying && !minimized && spot === "dock") void moveTo("window");
  });

  /* ---- Pointer -------------------------------------------------------- */

  reactOnClick(crab, () => {
    lastInteraction = performance.now();
    el.classList.remove("is-hinting");
  });
  el.addEventListener("pointerenter", () => {
    lastInteraction = performance.now();
  });

  /* ---- Entrance -------------------------------------------------------- */

  const enter = async () => {
    started = true;
    // Scrolled down far enough, the Genie has already put the window away.
    spot = note.style.visibility === "hidden" ? "dock" : "window";
    const home = perch(spot);
    crab.show(false);
    crab.place(crab.xAt(0.12, home), home);
    await crab.appear(true);
    await crab.walkTo(crab.xAt(spot === "window" ? HOME : 0.5, perch(spot)));
    remember();
    await crab.play("fresh", undefined, { enter: false, exit: false, here: true });
    // A short nudge that it can be clicked; it fades on its own.
    el.classList.add("is-hinting");
    window.setTimeout(() => el.classList.remove("is-hinting"), 2600);
    crab.startBlinking();
    scheduleIdle();
    startWatching();
  };

  crab.show(false);
  watchVisible(desk, (now) => {
    visible = now;
    if (now && !started) {
      window.setTimeout(() => void enter(), crab.reduced ? 0 : ENTRANCE_DELAY_MS);
    }
    if (now) {
      scheduleIdle();
      startWatching();
    } else {
      window.clearTimeout(idleTimer);
      window.clearTimeout(watchTimer);
      idleTimer = 0;
      watchTimer = 0;
    }
  });

  window.addEventListener("resize", settle, { passive: true });
}
