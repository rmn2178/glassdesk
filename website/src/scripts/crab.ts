import { gsap, prefersReducedMotion } from "@/lib/motion";
import { crabScripts, type CrabAct, type CrabEmote, type CrabFace } from "@/data/crab";

/** Popping in and out, a hop, and the shortest gap between two pokes: the app's values. */
const POP = 0.28;
const HOP = 0.45;
const POKE_GAP_MS = 900;
/** Walking speed, in cells per second (the app's charging walk). */
const WALK_CELLS = 70;

/**
 * Where the crab may stand, in pixels of its offset parent: `y` is the edge
 * under its legs, `left`..`right` the span it walks along.
 */
export interface Perch {
  left: number;
  right: number;
  y: number;
}

interface PlayOptions {
  /** Pop in at the start (otherwise it is already there). */
  enter?: boolean;
  /** Pop out at the end. */
  exit?: boolean;
  /** Stay where it is instead of following the act's path. */
  here?: boolean;
}

/**
 * One crab on the page. It plays the app's acts (`crabScripts`) with GSAP:
 * the body slides smoothly, the legs step at 8 fps in CSS, faces and emotes
 * switch on cue. With reduced motion it only changes faces and emotes.
 */
export class Crab {
  readonly el: HTMLElement;
  readonly reduced = prefersReducedMotion();
  /** The face it goes back to after an act. */
  rest: CrabFace;
  private readonly pop: HTMLElement;
  private readonly hop: HTMLElement;
  private readonly heart: SVGElement | null;
  private act: gsap.core.Timeline | null = null;
  private lastPoke = 0;
  private blinkTimer = 0;

  constructor(el: HTMLElement) {
    this.el = el;
    this.pop = el.querySelector<HTMLElement>(".crab-pop") ?? el;
    this.hop = el.querySelector<HTMLElement>(".crab-hop") ?? el;
    this.heart = el.querySelector<SVGElement>(".crab-poke");
    this.rest = (el.dataset.face as CrabFace) ?? "happy";
    el.addEventListener("pointerenter", () => this.poke());
  }

  get cell() {
    return parseFloat(getComputedStyle(this.el).getPropertyValue("--cell")) || 3;
  }

  get width() {
    return this.el.offsetWidth;
  }

  get height() {
    return this.el.offsetHeight;
  }

  /** Its left edge, as placed with `place`. */
  get x() {
    return Number(gsap.getProperty(this.el, "x")) || 0;
  }

  get busy() {
    return Boolean(this.act?.isActive());
  }

  face(face: CrabFace) {
    this.el.dataset.face = face;
  }

  emote(emote: CrabEmote | null) {
    if (emote) this.el.dataset.emote = emote;
    else delete this.el.dataset.emote;
  }

  walking(on: boolean) {
    this.el.classList.toggle("is-walking", on && !this.reduced);
  }

  /** Puts it at `x` (left edge), standing on `perch`. */
  place(x: number, perch: Perch) {
    gsap.set(this.el, { x, y: perch.y - this.height });
  }

  /** The left edge for a share (0..1) of the span of `perch`. */
  xAt(share: number, perch: Perch) {
    return perch.left + share * Math.max(perch.right - perch.left - this.width, 0);
  }

  /** The share of `perch` its current position is at. */
  shareOn(perch: Perch) {
    const span = Math.max(perch.right - perch.left - this.width, 1);
    return Math.min(Math.max((this.x - perch.left) / span, 0), 1);
  }

  show(visible: boolean) {
    gsap.set(this.pop, { scale: visible ? 1 : 0, opacity: visible ? 1 : 0 });
  }

  stop() {
    this.act?.kill();
    this.act = null;
    this.walking(false);
    this.emote(null);
    gsap.set(this.hop, { x: 0, y: 0, scaleX: 1, scaleY: 1 });
  }

  /** Plays one of the app's acts; resolves when it ends. */
  play(name: CrabAct, perch?: Perch, options: PlayOptions = {}): Promise<void> {
    const { enter = true, exit = true, here = !perch } = options;
    const script = crabScripts[name];
    const cell = this.cell;
    this.stop();

    return new Promise((resolve) => {
      const tl = gsap.timeline({
        onComplete: () => {
          this.walking(false);
          this.emote(null);
          if (!exit) this.face(this.rest);
          this.act = null;
          resolve();
        },
      });
      this.act = tl;

      if (perch) gsap.set(this.el, { y: perch.y - this.height });
      if (perch && !here) {
        const path = script.path;
        const last = path[path.length - 1];
        tl.set(this.el, { x: this.xAt((this.reduced ? last : path[0])[1], perch) }, 0);
        if (!this.reduced) {
          path.slice(1).forEach(([time, share], index) => {
            const [start] = path[index];
            tl.to(this.el, { x: this.xAt(share, perch), duration: time - start, ease: "none" }, start);
          });
        }
      }

      if (enter) {
        tl.fromTo(
          this.pop,
          { scale: this.reduced ? 1 : 0, opacity: 0 },
          { scale: 1, opacity: 1, duration: POP, ease: "back.out(2.2)" },
          0,
        );
      }
      if (exit) {
        tl.to(
          this.pop,
          { scale: this.reduced ? 1 : 0, opacity: 0, duration: POP, ease: "back.in(2)" },
          script.seconds - POP,
        );
      }

      if (!this.reduced) {
        script.hops.forEach(([time, cells]) => {
          tl.to(this.hop, { y: -cells * cell, duration: HOP / 2, ease: "sine.out" }, time);
          tl.to(this.hop, { y: 0, duration: HOP / 2, ease: "sine.in" }, time + HOP / 2);
        });
        if (script.shake) {
          const [from, to] = script.shake;
          const swings = Math.max(Math.round((to - from) / 0.05), 2);
          tl.fromTo(
            this.hop,
            { x: -cell * 0.5 },
            { x: cell * 0.5, duration: 0.05, repeat: swings - 1, yoyo: true, ease: "none" },
            from,
          );
          tl.set(this.hop, { x: 0 }, to);
        }
      }

      script.faces.forEach(([time, face]) => tl.call(() => this.face(face), undefined, time));
      script.walking.forEach(([from, to]) => {
        tl.call(() => this.walking(true), undefined, from);
        tl.call(() => this.walking(false), undefined, to);
      });
      if (script.emote) {
        const [emote, from, to] = script.emote;
        tl.call(() => this.emote(emote), undefined, from);
        tl.call(() => this.emote(null), undefined, to);
      }
      // The act lasts its full length even when nothing moves at the end.
      tl.set({}, {}, script.seconds);
    });
  }

  /** Pops in (or out) where it stands. */
  appear(visible = true): Promise<void> {
    return new Promise((resolve) => {
      gsap.fromTo(
        this.pop,
        { scale: visible ? (this.reduced ? 1 : 0) : 1, opacity: visible ? 0 : 1 },
        {
          scale: visible ? 1 : this.reduced ? 1 : 0,
          opacity: visible ? 1 : 0,
          duration: POP,
          ease: visible ? "back.out(2.2)" : "back.in(2)",
          onComplete: () => resolve(),
        },
      );
    });
  }

  /** Walks to `x` on its current edge, legs stepping. */
  walkTo(x: number, face: CrabFace = "happy"): Promise<void> {
    this.stop();
    const distance = Math.abs(x - this.x);
    if (this.reduced || distance < 1) {
      gsap.set(this.el, { x });
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.face(face);
      this.walking(true);
      this.act = gsap.timeline({
        onComplete: () => {
          this.walking(false);
          this.face(this.rest);
          this.act = null;
          resolve();
        },
      });
      this.act.to(this.el, { x, duration: distance / (WALK_CELLS * this.cell) + 0.15, ease: "power1.inOut" });
    });
  }

  /** Jumps in an arc to (x, perch): from a window's edge down to the dock and back. */
  jumpTo(x: number, perch: Perch): Promise<void> {
    this.stop();
    const y = perch.y - this.height;
    if (this.reduced) {
      gsap.set(this.el, { x, y });
      return Promise.resolve();
    }
    const x0 = this.x;
    const y0 = Number(gsap.getProperty(this.el, "y")) || 0;
    const lift = Math.max(this.cell * 14, (y0 - y) * 0.35 + this.cell * 8);
    const state = { t: 0 };
    return new Promise((resolve) => {
      this.face("happy");
      this.act = gsap.timeline({
        onComplete: () => {
          this.face(this.rest);
          this.act = null;
          resolve();
        },
      });
      this.act
        .to(this.hop, { scaleX: 1.12, scaleY: 0.86, duration: 0.12, ease: "power1.out" })
        .to(state, {
          t: 1,
          duration: 0.75,
          ease: "none",
          onUpdate: () => {
            const t = state.t;
            gsap.set(this.el, { x: x0 + (x - x0) * t, y: y0 + (y - y0) * t - lift * 4 * t * (1 - t) });
          },
        })
        .to(this.hop, { scaleX: 0.92, scaleY: 1.12, duration: 0.12, ease: "power1.out" }, 0.12)
        .to(this.hop, { scaleX: 1.14, scaleY: 0.84, duration: 0.1, ease: "power1.out" }, 0.87)
        .to(this.hop, { scaleX: 1, scaleY: 1, duration: 0.3, ease: "elastic.out(1, 0.45)" });
    });
  }

  /** A little hop, in place. */
  bounce(cells = 4) {
    if (this.reduced || this.busy) return;
    gsap
      .timeline()
      .to(this.hop, { y: -cells * this.cell, duration: HOP / 2, ease: "sine.out" })
      .to(this.hop, { y: 0, duration: HOP / 2, ease: "sine.in" });
  }

  /**
   * Hovered: a squash and a heart that rises and fades, as the app's `react`.
   * At most once every 0.9 s.
   */
  poke() {
    const now = performance.now();
    if (now - this.lastPoke < POKE_GAP_MS) return;
    this.lastPoke = now;
    const cell = this.cell;
    if (!this.reduced && !this.busy) {
      gsap
        .timeline()
        .to(this.hop, { scaleX: 1.12, scaleY: 0.86, duration: 0.09, ease: "power1.out" })
        .to(this.hop, { scaleX: 0.92, scaleY: 1.12, duration: 0.12, ease: "power1.out" })
        .to(this.hop, { scaleX: 1, scaleY: 1, duration: 0.14, ease: "power1.out" });
    }
    if (this.heart) {
      gsap.fromTo(
        this.heart,
        { y: 0, opacity: 1 },
        { y: this.reduced ? 0 : -6 * cell, opacity: 0, duration: 0.65, ease: "power1.out" },
      );
    }
  }

  /** Blinks now and then while it rests: it looks alive, and costs one timer. */
  startBlinking() {
    if (this.reduced || this.blinkTimer) return;
    const tick = () => {
      this.blinkTimer = window.setTimeout(tick, 2600 + Math.random() * 3200);
      if (this.busy || !["happy", "neutral"].includes(this.el.dataset.face ?? "")) return;
      const face = this.el.dataset.face as CrabFace;
      this.face("sleep");
      window.setTimeout(() => {
        if (this.el.dataset.face === "sleep" && !this.busy) this.face(face);
      }, 140);
    };
    this.blinkTimer = window.setTimeout(tick, 2000);
  }

  stopBlinking() {
    window.clearTimeout(this.blinkTimer);
    this.blinkTimer = 0;
  }
}

/** Calls `onChange(true)` when `el` scrolls into view and `false` when it leaves. */
export function watchVisible(el: Element, onChange: (visible: boolean) => void, threshold = 0.25) {
  const observer = new IntersectionObserver(
    (entries) => entries.forEach((entry) => onChange(entry.isIntersecting)),
    { threshold },
  );
  observer.observe(el);
  return observer;
}

const REACTIONS: CrabAct[] = ["fresh", "nervous", "exhausted"];

/** A clicked crab plays a random emotion in place, never the same twice in a row. */
export function reactOnClick(crab: Crab, onReact?: () => void) {
  let last: CrabAct | null = null;
  crab.el.addEventListener("click", () => {
    if (crab.busy) return;
    const choices = REACTIONS.filter((act) => act !== last);
    const act = choices[Math.floor(Math.random() * choices.length)];
    last = act;
    onReact?.();
    void crab.play(act, undefined, { enter: false, exit: false, here: true });
  });
}
