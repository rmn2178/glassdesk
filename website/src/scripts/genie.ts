import { gsap } from "@/lib/motion";

const MINIMIZE_SECONDS = 0.6;
const RESTORE_SECONDS = 0.5;
/** Strip height in CSS pixels: thin enough that the curved sides look smooth. */
const STRIP = 1;
const MAX_DPR = 2;
const THUMB_SIZE = 52;
/** Resting width of a dock item, needed to measure the slot without the magnification. */
const DOCK_REST = 52;
/** Resize and font events arrive in bursts; the scene is rebuilt once they settle. */
const REBUILD_DELAY_MS = 120;

/** Same curve parameters as the app. */
const BEND_END = 0.5;
const SLIDE_START = 0.15;

const clamp01 = (value: number) => Math.min(1, Math.max(0, value));
const smoothstep = (value: number) => {
  const x = clamp01(value);
  return x * x * (3 - 2 * x);
};

interface Edges {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

interface Scene {
  window: Edges;
  slot: Edges;
  source: HTMLCanvasElement;
}

export function initGenie(root: HTMLElement, reduceMotion: boolean) {
  const canvas = root.querySelector<HTMLCanvasElement>("[data-genie]");
  const note = root.querySelector<HTMLElement>("[data-note]");
  const slot = root.querySelector<HTMLElement>("[data-dock-thumb]");
  const thumb = slot?.querySelector<HTMLCanvasElement>("canvas");
  const restore = slot?.querySelector<HTMLButtonElement>("[data-dock-restore]");
  const dock = root.querySelector<HTMLElement>(".dock");
  const context = canvas?.getContext("2d");
  if (!canvas || !note || !slot || !thumb || !dock || !context) return;

  let dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
  const state = { progress: 0 };
  let scene: Scene | null = null;
  let slotValue = -1;
  let flyingBefore = false;
  let minimizedBefore = false;
  let version = 0;

  const paint = (progress: number) => {
    const hidden = progress > 0;
    const minimized = progress >= 1;
    const flying = hidden && !minimized;
    canvas.classList.toggle("hidden", !flying);
    note.style.visibility = hidden ? "hidden" : "";

    if (flying !== flyingBefore || minimized !== minimizedBefore) {
      flyingBefore = flying;
      minimizedBefore = minimized;
      window.dispatchEvent(new CustomEvent("nosih:genie", { detail: { flying, minimized } }));
    }

    // The slot opens as the window reaches it, so the dock shows no thumbnail while
    // nothing is minimized.
    const open = clamp01((progress - 0.6) / 0.4);
    if (open !== slotValue) {
      slotValue = open;
      slot.style.setProperty("--slot", String(open));
      slot.toggleAttribute("inert", open < 0.5);
      if (open === 0 || open === 1) window.dispatchEvent(new CustomEvent("nosih:slot"));
    }

    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, canvas.width, canvas.height);
    if (flying && scene) drawFrame(context, scene, progress, dpr);
  };

  const build = async () => {
    // Builds are async and can overlap; only the newest one may write the scene.
    const mine = ++version;
    dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    const rootRect = root.getBoundingClientRect();
    const noteRect = note.getBoundingClientRect();
    const slotRect = measureSlot(root, slot);

    // A canvas is a replaced element: `inset-0` does not stretch it, so without an
    // explicit CSS size it draws at its backing-store size and the drawing is scaled by dpr.
    canvas.style.width = `${rootRect.width}px`;
    canvas.style.height = `${rootRect.height}px`;
    canvas.width = Math.round(rootRect.width * dpr);
    canvas.height = Math.round(rootRect.height * dpr);

    // The copy reads the window's layout synchronously, so it is shown for that
    // moment only (no paint happens in between) in case it is minimized.
    const visibility = note.style.visibility;
    note.style.visibility = "";
    const copying = copyElement(note, dpr);
    note.style.visibility = visibility;
    const source = await copying;
    if (mine !== version) return;

    scene = {
      window: {
        left: noteRect.left - rootRect.left,
        top: noteRect.top - rootRect.top,
        right: noteRect.right - rootRect.left,
        bottom: noteRect.bottom - rootRect.top,
      },
      slot: {
        left: slotRect.left - rootRect.left,
        top: slotRect.top - rootRect.top,
        right: slotRect.right - rootRect.left,
        bottom: slotRect.bottom - rootRect.top,
      },
      source,
    };
    renderThumbnail(thumb, source, dpr);
    paint(state.progress);
  };

  build();

  // Below 900 px the dock scrolls sideways and its slot sits off screen, so the
  // window simply stays where it is.
  const wideEnough = window.matchMedia("(min-width: 900px)");

  const play = (target: 0 | 1) => {
    if (!wideEnough.matches || state.progress === target) return;
    gsap.killTweensOf(state);
    if (reduceMotion) {
      state.progress = target;
      paint(target);
      return;
    }
    gsap.to(state, {
      progress: target,
      duration: (target ? MINIMIZE_SECONDS : RESTORE_SECONDS) * Math.abs(target - state.progress),
      ease: "none",
      onUpdate: () => paint(state.progress),
    });
  };

  root.querySelector<HTMLButtonElement>("[data-minimize]")?.addEventListener("click", () => play(1));
  restore?.addEventListener("click", () => play(0));

  // Scrolling plays the effect once per crossing, never scrubs it: the window minimizes
  // when the whole dock is on screen and comes back when the dock leaves at the bottom.
  // In between, the buttons are free to do what the user asks.
  let dockInView: boolean | null = null;
  let frame = 0;
  const onScroll = () => {
    frame = 0;
    if (reduceMotion || !wideEnough.matches) return;
    const rect = dock.getBoundingClientRect();
    const inView = rect.bottom <= window.innerHeight && rect.top >= 0;
    const below = rect.top > window.innerHeight;
    if (dockInView === null) {
      // First reading: jump straight to the matching state, without animating.
      dockInView = inView;
      if (inView) {
        state.progress = 1;
        paint(1);
      }
      return;
    }
    if (inView && !dockInView) play(1);
    if (below && dockInView) play(0);
    if (inView || below) dockInView = inView;
  };
  window.addEventListener(
    "scroll",
    () => {
      if (!frame) frame = requestAnimationFrame(onScroll);
    },
    { passive: true },
  );

  wideEnough.addEventListener("change", (event) => {
    if (event.matches) {
      dockInView = null;
      onScroll();
    } else {
      gsap.killTweensOf(state);
      state.progress = 0;
      paint(0);
    }
  });

  let timer = 0;
  const refresh = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      build();
      onScroll();
    }, REBUILD_DELAY_MS);
  };
  window.addEventListener("resize", refresh, { passive: true });
  document.fonts.ready.then(refresh);
  window.addEventListener("load", refresh);
}

/** Where the slot will be once open, with every dock icon at rest. */
function measureSlot(root: HTMLElement, slot: HTMLElement): DOMRect {
  const items = [...root.querySelectorAll<HTMLElement>(".dock-item")];
  const saved = items.map((item) => [item.style.width, item.style.height]);
  items.forEach((item) => {
    if (item === slot) return;
    item.style.width = `${DOCK_REST}px`;
    item.style.height = `${DOCK_REST}px`;
  });
  const previous = slot.style.getPropertyValue("--slot");
  slot.style.setProperty("--slot", "1");
  const rect = slot.getBoundingClientRect();
  slot.style.setProperty("--slot", previous || "0");
  items.forEach((item, index) => {
    item.style.width = saved[index][0];
    item.style.height = saved[index][1];
  });
  return rect;
}

/** One frame of the app's genie: equal strips placed by `bend` and `slide`. */
function drawFrame(context: CanvasRenderingContext2D, scene: Scene, t: number, dpr: number) {
  const { window: win, slot, source } = scene;
  const height = win.bottom - win.top;
  const count = Math.max(1, Math.round(height / STRIP));
  const bend = smoothstep(t / BEND_END);
  const slideT = clamp01((t - SLIDE_START) / (1 - SLIDE_START));
  const shift = slideT * slideT * Math.max(0, slot.bottom - win.top);
  const span = slot.top - win.top;
  const sliceHeight = source.height / count;

  const sides = (y: number) => {
    const along = span > 0 ? clamp01((y - win.top) / span) : 1;
    const curve = ((1 - Math.cos(Math.PI * along)) / 2) * bend;
    return [win.left + (slot.left - win.left) * curve, win.right + (slot.right - win.right) * curve];
  };

  context.setTransform(dpr, 0, 0, dpr, 0, 0);
  for (let index = 0; index < count; index += 1) {
    const top = win.top + (index / count) * height + shift;
    if (top >= slot.bottom) break;
    const bottom = Math.min(win.top + ((index + 1) / count) * height + shift, slot.bottom);
    const [left, right] = sides((top + bottom) / 2);
    // Each strip overlaps the next slightly, so no seams show between them.
    context.drawImage(
      source,
      0,
      index * sliceHeight,
      source.width,
      sliceHeight,
      left,
      top,
      right - left,
      bottom - top + 0.6,
    );
  }
}

/**
 * Copies an element into a canvas from its own layout: backgrounds, borders, text and
 * inline SVG icons, with the computed fonts and colours. Enough for the Notes mock, and
 * it can never drift from the markup.
 */
async function copyElement(element: HTMLElement, dpr: number): Promise<HTMLCanvasElement> {
  const box = element.getBoundingClientRect();
  const out = document.createElement("canvas");
  out.width = Math.round(box.width * dpr);
  out.height = Math.round(box.height * dpr);
  const context = out.getContext("2d");
  if (!context) return out;
  context.scale(dpr, dpr);

  const local = (rect: DOMRect) => ({
    x: rect.left - box.left,
    y: rect.top - box.top,
    width: rect.width,
    height: rect.height,
  });
  const radius = (style: CSSStyleDeclaration, rect: DOMRect) =>
    Math.min(parseFloat(style.borderTopLeftRadius) || 0, rect.width / 2, rect.height / 2);
  const opacityOf = (node: Element) => {
    let value = 1;
    for (let current: Element | null = node; current && current !== element.parentElement; current = current.parentElement) {
      value *= parseFloat(getComputedStyle(current).opacity);
    }
    return value;
  };

  const rootStyle = getComputedStyle(element);
  context.save();
  context.beginPath();
  context.roundRect(0, 0, box.width, box.height, radius(rootStyle, box));
  context.clip();

  const icons: Promise<void>[] = [];
  const nodes = [element, ...element.querySelectorAll<Element>("*")];
  for (const node of nodes) {
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden") continue;
    const alpha = opacityOf(node);
    if (alpha === 0) continue;
    const rect = node.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) continue;
    const at = local(rect);
    context.globalAlpha = alpha;

    if (node instanceof SVGSVGElement) {
      icons.push(drawSvg(context, node, at));
      continue;
    }
    if (node instanceof SVGElement) continue;

    if (style.backgroundColor !== "rgba(0, 0, 0, 0)") {
      context.fillStyle = style.backgroundColor;
      context.beginPath();
      context.roundRect(at.x, at.y, at.width, at.height, radius(style, rect));
      context.fill();
    }
    const borderWidth = parseFloat(style.borderTopWidth);
    if (borderWidth > 0 && style.borderTopStyle !== "none") {
      context.strokeStyle = style.borderTopColor;
      context.lineWidth = borderWidth;
      context.beginPath();
      const half = borderWidth / 2;
      context.roundRect(at.x + half, at.y + half, at.width - borderWidth, at.height - borderWidth, radius(style, rect));
      context.stroke();
    }
    const right = parseFloat(style.borderRightWidth);
    if (right > 0 && borderWidth === 0 && style.borderRightStyle !== "none") {
      context.fillStyle = style.borderRightColor;
      context.fillRect(at.x + at.width - right, at.y, right, at.height);
    }

    for (const child of node.childNodes) {
      if (child.nodeType !== Node.TEXT_NODE || !child.textContent?.trim()) continue;
      const range = document.createRange();
      range.selectNodeContents(child);
      const textRect = range.getBoundingClientRect();
      if (textRect.width === 0) continue;
      context.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
      context.letterSpacing = style.letterSpacing === "normal" ? "0px" : style.letterSpacing;
      context.fillStyle = style.color;
      context.textBaseline = "alphabetic";
      const raw = child.textContent.trim();
      const text =
        style.textTransform === "uppercase"
          ? raw.toUpperCase()
          : style.textTransform === "lowercase"
            ? raw.toLowerCase()
            : raw;
      const metrics = context.measureText(text);
      context.fillText(text, textRect.left - box.left, textRect.top - box.top + metrics.fontBoundingBoxAscent);
    }
  }

  await Promise.all(icons);
  context.restore();
  return out;
}

function drawSvg(
  context: CanvasRenderingContext2D,
  svg: SVGSVGElement,
  at: { x: number; y: number; width: number; height: number },
): Promise<void> {
  const copy = svg.cloneNode(true) as SVGSVGElement;
  copy.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  copy.setAttribute("width", String(at.width));
  copy.setAttribute("height", String(at.height));
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(copy.outerHTML)}`;
  const alpha = context.globalAlpha;
  return new Promise((resolve) => {
    const image = new Image();
    image.onload = () => {
      const previous = context.globalAlpha;
      context.globalAlpha = alpha;
      context.drawImage(image, at.x, at.y, at.width, at.height);
      context.globalAlpha = previous;
      resolve();
    };
    image.onerror = () => resolve();
    image.src = url;
  });
}

/** The dock thumbnail is the same copy, cropped square from the top. */
function renderThumbnail(canvas: HTMLCanvasElement, source: HTMLCanvasElement, dpr: number) {
  const size = Math.round(THUMB_SIZE * dpr);
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) return;
  context.clearRect(0, 0, size, size);
  context.save();
  context.beginPath();
  context.roundRect(0, 0, size, size, 12 * dpr);
  context.clip();
  const scale = Math.max(size / source.width, size / source.height);
  const width = source.width * scale;
  context.drawImage(source, (size - width) / 2, 0, width, source.height * scale);
  context.restore();
}