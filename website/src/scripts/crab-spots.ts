import { Crab, reactOnClick, watchVisible, type Perch } from "@/scripts/crab";

/**
 * The AI usage section: when the figure scrolls in, the crab hops out under
 * the open island with a heart, as it does when a limit resets, and stays.
 */
export function initUsageCrab(root: HTMLElement) {
  const figure = root.querySelector<HTMLElement>(".usage-desk");
  const island = root.querySelector<HTMLElement>(".usage-island");
  const el = root.querySelector<HTMLElement>("[data-usage-crab]");
  if (!figure || !island || !el) return;

  const crab = new Crab(el);
  crab.show(false);
  let played = false;

  const perch = (): Perch => {
    const d = figure.getBoundingClientRect();
    const r = island.getBoundingClientRect();
    const centre = r.left - d.left + r.width / 2;
    return {
      left: centre - crab.width,
      right: centre + crab.width,
      y: Math.min(r.bottom - d.top + crab.height + 9 * crab.cell, d.height - 12),
    };
  };

  watchVisible(
    figure,
    (visible) => {
      if (!visible || played) return;
      played = true;
      void crab.play("fresh", perch(), { exit: false }).then(() => crab.startBlinking());
    },
    0.5,
  );
  reactOnClick(crab);
  window.addEventListener(
    "resize",
    () => {
      if (played && !crab.busy) crab.place(crab.xAt(0.5, perch()), perch());
    },
    { passive: true },
  );
}

/**
 * The download card: the crab sits on the app icon and cheers whenever the
 * pointer reaches the download button.
 */
export function initDownloadCrab(root: HTMLElement) {
  const el = root.querySelector<HTMLElement>("[data-download-crab]");
  const cta = root.querySelector<HTMLElement>(".get-actions a");
  if (!el) return;

  const crab = new Crab(el);
  crab.show(false);
  let shown = false;

  watchVisible(
    el,
    (visible) => {
      if (!visible || shown) return;
      shown = true;
      void crab.play("fresh", undefined, { exit: false }).then(() => crab.startBlinking());
    },
    0.6,
  );
  cta?.addEventListener("pointerenter", () => {
    if (!shown || crab.busy) return;
    crab.face("love");
    crab.poke();
    crab.bounce(5);
    window.setTimeout(() => crab.face(crab.rest), 800);
  });
  reactOnClick(crab);
}

/**
 * The Settings replica: "Crab on special moments" shows it on the preview
 * dock; turning the switch on sends it scurrying along, as the charger does.
 */
export function initPreviewCrab(root: HTMLElement) {
  const preview = root.querySelector<HTMLElement>("[data-preview]");
  const dock = root.querySelector<HTMLElement>("[data-preview-dock]");
  const el = root.querySelector<HTMLElement>("[data-preview-crab]");
  const toggle = root.querySelector<HTMLButtonElement>('[data-setting="mascot"]');
  if (!preview || !dock || !el || !toggle) return;

  const crab = new Crab(el);
  crab.show(false);
  let seen = false;

  const perch = (): Perch => {
    const d = preview.getBoundingClientRect();
    const r = dock.getBoundingClientRect();
    return { left: r.left - d.left + 4, right: r.right - d.left - 4, y: r.top - d.top + 1 };
  };
  const on = () => toggle.getAttribute("aria-checked") === "true";
  const parade = () => {
    if (on() && !dock.classList.contains("is-hidden")) void crab.play("charging", perch());
  };

  // The Settings script flips the switch first (it registered earlier).
  toggle.addEventListener("click", () => {
    if (on()) parade();
    else {
      crab.stop();
      void crab.appear(false);
    }
  });
  watchVisible(
    preview,
    (visible) => {
      if (visible && !seen) {
        seen = true;
        window.setTimeout(parade, 600);
      }
    },
    0.6,
  );
}
