import { gsap, ScrollTrigger, prefersReducedMotion } from "@/lib/motion";

const DURATION = 0.9;
const STAGGER = 0.08;
/** Four steps is all the eye notices; a full twenty-item stagger feels slow. */
const STAGGER_STEPS = 4;

let wave = 0;
let resetFrame = 0;

/** Items that enter in the same frame form one wave and stagger among themselves. */
const nextDelay = () => {
  const delay = (wave % STAGGER_STEPS) * STAGGER;
  wave += 1;
  if (!resetFrame) {
    resetFrame = requestAnimationFrame(() => {
      wave = 0;
      resetFrame = 0;
    });
  }
  return delay;
};

/**
 * Fades in the elements marked `data-r` once. Each element gets its own trigger:
 * a batch defers elements that are already on screen at load, which leaves them
 * invisible until the visitor scrolls.
 */
export function initReveal(root: HTMLElement) {
  if (root.dataset.revealed === "true") return;
  root.dataset.revealed = "true";

  const items = [...root.querySelectorAll<HTMLElement>("[data-r]")];
  if (!items.length || prefersReducedMotion()) return;

  gsap.set(items, { opacity: 0, y: 32 });

  items.forEach((item) => {
    ScrollTrigger.create({
      trigger: item,
      start: "top 92%",
      once: true,
      onEnter: () =>
        gsap.to(item, {
          opacity: 1,
          y: 0,
          duration: DURATION,
          delay: nextDelay(),
          ease: "power3.out",
        }),
    });
  });
}