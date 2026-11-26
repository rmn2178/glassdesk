import { gsap } from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";

// One registration for the whole page: scripts import these instead of registering again.
gsap.registerPlugin(ScrollTrigger);

// Fonts and late images change the layout ScrollTrigger measured on boot.
document.fonts.ready.then(() => ScrollTrigger.refresh());
window.addEventListener("load", () => ScrollTrigger.refresh());

export { gsap, ScrollTrigger };

/** Shared motion helpers. Scripts run in the browser only. */
export const prefersReducedMotion = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;
