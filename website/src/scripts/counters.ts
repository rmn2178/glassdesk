import { gsap, ScrollTrigger, prefersReducedMotion } from "@/lib/motion";

const DURATION = 1.6;

/**
 * Counts each figure up once, the first time it scrolls into view. The markup ships the
 * real number, so a visitor without JavaScript or with reduced motion reads the figure
 * instead of a zero.
 */
export function initCounters(root: ParentNode) {
  const nodes = [...root.querySelectorAll<HTMLElement>("[data-counter]")];
  if (!nodes.length || prefersReducedMotion()) return;

  nodes.forEach((node) => {
    const to = Number(node.dataset.to);
    const decimals = Number(node.dataset.decimals ?? 0);
    if (!Number.isFinite(to)) return;

    const counter = { value: 0 };

    ScrollTrigger.create({
      trigger: node,
      start: "top 92%",
      once: true,
      // Zeroing inside the trigger, not on load: if the trigger never fires the figure
      // stays correct instead of stuck at zero.
      onEnter: () => {
        node.textContent = (0).toFixed(decimals);
        gsap.to(counter, {
          value: to,
          duration: DURATION,
          ease: "power2.out",
          onUpdate: () => {
            node.textContent = counter.value.toFixed(decimals);
          },
        });
      },
    });
  });
}
