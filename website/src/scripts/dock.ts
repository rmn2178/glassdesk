import { gsap } from "@/lib/motion";

/**
 * Hero dock, mirroring `Nosih_landing.html`: a gaussian magnification around the
 * pointer, plus a self-playing cursor that sweeps the row and presses an icon.
 *
 * One frame loop drives both and stops as soon as nothing is moving, so an idle
 * or off-screen dock costs nothing.
 */

const REST_SIZE = 52;
const PEAK_SCALE = 1.7;
/** Width of the magnified area, in CSS pixels. */
const SIGMA = 70;
/** Share of the remaining distance covered per frame. */
const EASING = 0.3;
const HOVER_MARGIN_TOP = 70;
const HOVER_MARGIN_BOTTOM = 30;
/** Angular speed of the autoplay sweep, in radians per second. */
const SWEEP_SPEED = 0.9;
const SWEEP_TRAVEL = 0.46;
const AUTOPLAY_INTERVAL = 3200;
/** Only an icon this close to the virtual pointer is pressed. */
const CLICK_RADIUS = 30;
/** The dot appears once the bounce is under way. */
const DOT_DELAY = 900;
/** Bounce after a press: 26 px, four 0.28 s halves, easing out, as in the original. */
const BOUNCE_HEIGHT = 26;
const BOUNCE_DURATION = 0.28;
const BOUNCE_REPEATS = 3;
/** Below this distance the width snaps to its target and the frame loop can stop. */
const SETTLE = 0.05;

/** An item that is not an app (the minimized-window slot): never resized or pressed. */
const isSlot = (item: HTMLElement) => item.hasAttribute("data-dock-thumb");

export function initDock(root: HTMLElement, reduceMotion: boolean) {
  const dock = root.querySelector<HTMLElement>(".dock");
  // Without a hovering pointer there is nothing to follow; the dock stays at rest.
  if (!dock || reduceMotion || !window.matchMedia("(hover: hover)").matches) return;
  if (dock.dataset.ready === "true") return;
  dock.dataset.ready = "true";

  const cursor = root.querySelector<HTMLElement>("[data-cursor]");
  const cursorIcon = cursor?.querySelector<SVGElement>("svg") ?? null;
  const ripple = cursor?.querySelector<HTMLElement>("[data-cursor-ripple]") ?? null;
  const items = Array.from(dock.querySelectorAll<HTMLElement>(".dock-item"));

  const widths = items.map(() => REST_SIZE);
  let centers: number[] = [];
  let pointer: number | null = null;
  let frame = 0;
  let visible = true;
  /** The sweep owns the pointer whenever the real one is away from the dock. */
  let auto = true;
  let geniePaused = false;
  let nextClick = 0;

  const rest = () => {
    items.forEach((item, index) => {
      // The minimized-window slot sizes itself from `--slot`; an inline width would pin it open.
      if (isSlot(item)) return;
      widths[index] = REST_SIZE;
      item.style.width = `${REST_SIZE}px`;
      item.style.height = `${REST_SIZE}px`;
    });
  };

  const measure = () => {
    centers = items.map((item) => {
      const rect = item.getBoundingClientRect();
      return rect.left + rect.width / 2;
    });
  };

  const press = (item: HTMLElement) => {
    const icon = item.querySelector<HTMLElement>("img");
    if (icon) {
      gsap.to(icon, {
        y: -BOUNCE_HEIGHT,
        duration: BOUNCE_DURATION,
        ease: "power2.out",
        yoyo: true,
        repeat: BOUNCE_REPEATS,
        overwrite: true,
      });
    }

    if (cursorIcon) {
      cursorIcon.classList.remove("is-pressing");
      // Any layout read restarts the animation when the same icon is pressed twice.
      void cursorIcon.getBoundingClientRect();
      cursorIcon.classList.add("is-pressing");
    }

    if (ripple) {
      ripple.classList.remove("is-rippling");
      void ripple.offsetWidth;
      ripple.classList.add("is-rippling");
    }

    if (!item.querySelector(".dock-dot")) {
      window.setTimeout(() => {
        if (item.querySelector(".dock-dot")) return;
        const dot = document.createElement("i");
        dot.className = "dock-dot is-new";
        dot.setAttribute("aria-hidden", "true");
        item.append(dot);
      }, DOT_DELAY);
    }
  };

  const nearest = (x: number) => {
    let best = CLICK_RADIUS;
    let found: HTMLElement | null = null;

    items.forEach((item, index) => {
      if (isSlot(item)) return;
      const distance = Math.abs(centers[index] - x);
      if (distance < best) {
        best = distance;
        found = item;
      }
    });

    return found;
  };

  const sweepFrame = (time: number) => {
    const dockRect = dock.getBoundingClientRect();
    const rootRect = root.getBoundingClientRect();
    const seconds = time / 1000;
    const x =
      dockRect.left +
      dockRect.width / 2 +
      Math.sin(seconds * SWEEP_SPEED) * dockRect.width * SWEEP_TRAVEL;

    if (cursor) {
      // At 45 % of an icon, over the glass, as in the original.
      const y = dockRect.top - rootRect.top + REST_SIZE * 0.45;
      cursor.style.transform = `translate(${x - rootRect.left}px, ${y}px)`;
      cursor.style.opacity = "1";
    }

    if (nextClick === 0) nextClick = time + AUTOPLAY_INTERVAL;
    if (time >= nextClick) {
      nextClick = time + AUTOPLAY_INTERVAL;
      const target = nearest(x);
      if (target) press(target);
    }

    return x;
  };

  const step = (time: number) => {
    frame = 0;
    let moving = false;

    if (auto && visible) {
      pointer = sweepFrame(time);
      moving = true;
    }

    items.forEach((item, index) => {
      if (isSlot(item)) return;

      const distance = pointer === null ? 0 : pointer - centers[index];
      const target =
        pointer === null
          ? REST_SIZE
          : REST_SIZE *
            (1 + (PEAK_SCALE - 1) * Math.exp(-(distance * distance) / (2 * SIGMA * SIGMA)));
      const delta = target - widths[index];

      if (Math.abs(delta) > SETTLE) {
        widths[index] += delta * EASING;
        moving = true;
      } else {
        widths[index] = target;
      }

      item.style.width = `${widths[index]}px`;
      item.style.height = `${widths[index]}px`;
    });

    if (moving) frame = requestAnimationFrame(step);
  };

  const start = () => {
    if (!frame) frame = requestAnimationFrame(step);
  };

  const follow = (x: number | null) => {
    // Widths move the centres, so a fresh entry measures the resting layout once.
    if (x !== null && pointer === null) {
      rest();
      measure();
    }
    pointer = x;
    start();
  };

  root.addEventListener(
    "pointermove",
    (event: PointerEvent) => {
      if (geniePaused || event.pointerType !== "mouse") return;

      const dockRect = dock.getBoundingClientRect();
      const near =
        event.clientY > dockRect.top - HOVER_MARGIN_TOP &&
        event.clientY < dockRect.bottom + HOVER_MARGIN_BOTTOM;

      auto = !near;
      if (near) nextClick = 0;
      if (cursor) cursor.style.opacity = near ? "0" : "1";
      follow(near ? event.clientX : null);
    },
    { passive: true },
  );

  root.addEventListener("pointerleave", () => {
    if (geniePaused) return;
    auto = true;
    nextClick = 0;
    if (cursor) cursor.style.opacity = "1";
    follow(null);
  });

  // Clicking an icon bounces it, as the original did.
  dock.addEventListener("click", (event) => {
    const item = event.target instanceof Element ? event.target.closest<HTMLElement>(".dock-item") : null;
    if (item && !isSlot(item)) press(item);
  });

  // A narrow window scrolls the row, which moves every centre.
  dock.parentElement?.addEventListener("scroll", measure, { passive: true });

  // The page scroll moves the dock under a parked pointer, and no pointermove fires then.
  window.addEventListener(
    "scroll",
    () => {
      if (pointer !== null) measure();
    },
    { passive: true },
  );

  // The genie opens and closes the minimized-window slot, which reflows the row.
  window.addEventListener("nosih:slot", () => {
    measure();
    start();
  });

  // While a window is flying to the dock the demo stands still: the sweep magnifies
  // icons and would drag the slot away from where the window is heading. Once it has
  // landed the dock is live again, hover included.
  window.addEventListener("nosih:genie", (event) => {
    const { flying } = (event as CustomEvent<{
      flying: boolean;
      minimized: boolean;
    }>).detail;
    geniePaused = flying;
    auto = !geniePaused;
    nextClick = 0;
    if (geniePaused) {
      pointer = null;
      rest();
      measure();
    }
    if (cursor) cursor.style.opacity = geniePaused ? "0" : "1";
    if (!geniePaused) start();
  });

  window.addEventListener(
    "resize",
    () => {
      rest();
      measure();
      start();
    },
    { passive: true },
  );

  new IntersectionObserver(([entry]) => {
    visible = entry?.isIntersecting ?? false;
    if (visible) start();
  }).observe(root);

  // The cursor shows up with the first sweep frame instead of flashing in a corner.
  if (cursor) cursor.style.opacity = "0";
  rest();
  measure();
  start();
}
