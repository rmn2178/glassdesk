import { gsap, prefersReducedMotion } from "@/lib/motion";

/** How far the hero copy drifts up while the mockup leaves the viewport. */
const DRIFT = -40;
const PALE = 0.2;

/**
 * Hero motion: the copy fades in with the page, then drifts up and pales as the mockup
 * scrolls away. The mockup itself rises in after the copy, as in the original landing.
 */
export function initHeroMotion() {
  if (prefersReducedMotion()) return;

  const copy = gsap.utils.toArray<HTMLElement>("[data-h]");
  const mock = document.querySelector<HTMLElement>("[data-hero-mock]");
  if (!copy.length || !mock) return;

  const intro = gsap.from(copy, { y: 28, opacity: 0, duration: 1, stagger: 0.1, ease: "power3.out" });

  gsap.from(mock, {
    y: 70,
    opacity: 0,
    duration: 1.2,
    delay: 0.4,
    ease: "power3.out",
    // The dock and the genie measured the mockup before this transform existed, so they
    // rebuild once it lands and the geometry is true again.
    onComplete: () => window.dispatchEvent(new Event("resize")),
  });

  // The drift is built only once the intro has written its final values. A scroll tween on
  // the same properties, created while the copy was still at opacity 0, recorded that zero
  // as its start and left the hero invisible on the way back up.
  intro.eventCallback("onComplete", () => {
    gsap.fromTo(
      copy,
      { y: 0, opacity: 1 },
      {
        y: DRIFT,
        opacity: PALE,
        ease: "none",
        immediateRender: false,
        scrollTrigger: { trigger: mock, start: "top 80%", end: "top 20%", scrub: true },
      },
    );
  });
}
