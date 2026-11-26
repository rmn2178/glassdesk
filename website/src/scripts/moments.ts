import { gsap } from "@/lib/motion";
import { crabMoments, crabScripts, type CrabMoment } from "@/data/crab";
import { Crab, watchVisible, type Perch } from "@/scripts/crab";

/** The figure moves to the new value before the crab comes out, as the app sees it change. */
const LEAD_IN = 0.7;
const PAUSE_MS = 900;
/** Where the island's figure starts before each AI moment. */
const BEFORE: Record<string, number> = { fresh: 63, nervous: 84, exhausted: 96 };

/**
 * "Meet the crab": each moment plays the app's act at the right place, under
 * the island or along the dock, after the island or the battery changes.
 * While the section is in view it runs through them on its own, until the
 * visitor picks one.
 */
export function initMoments(root: HTMLElement) {
  const desk = root.querySelector<HTMLElement>(".moments-desk");
  const el = root.querySelector<HTMLElement>("[data-moments-crab]");
  const island = root.querySelector<HTMLElement>("[data-island]");
  const usage = root.querySelector<HTMLElement>("[data-usage]");
  const ring = root.querySelector<SVGCircleElement>("[data-ring]");
  const dock = root.querySelector<HTMLElement>("[data-moments-dock]");
  const power = root.querySelector<HTMLElement>("[data-power]");
  const powerLabel = root.querySelector<HTMLElement>("[data-power-label]");
  const caption = root.querySelector<HTMLElement>("[data-moment-caption]");
  const buttons = [...root.querySelectorAll<HTMLButtonElement>("[data-moment]")];
  if (!desk || !el || !island || !usage || !ring || !dock || !power || !buttons.length) return;

  const crab = new Crab(el);
  crab.show(false);
  let current = 0;
  let auto = true;
  let visible = false;
  let run = 0;
  let timer = 0;
  let progress: gsap.core.Tween | null = null;

  const perch = (moment: CrabMoment): Perch => {
    const d = desk.getBoundingClientRect();
    if (moment.place === "island") {
      const r = island.getBoundingClientRect();
      const centre = r.left - d.left + r.width / 2;
      // Under the island, with room above it for the emote.
      return {
        left: centre - crab.width,
        right: centre + crab.width,
        y: r.bottom - d.top + crab.height + 14 * crab.cell,
      };
    }
    const r = dock.getBoundingClientRect();
    return { left: r.left - d.left + 6, right: r.right - d.left - 6, y: r.top - d.top + 1 };
  };

  const setUsage = (value: number) => {
    usage.textContent = String(Math.round(value));
    ring.setAttribute("stroke-dasharray", `${value} 100`);
    island.classList.toggle("is-hot", value >= 90);
  };

  const setPower = (state?: "charging" | "saver") => {
    if (state) power.dataset.state = state;
    else delete power.dataset.state;
    if (powerLabel) powerLabel.textContent = state === "charging" ? "Charging" : state === "saver" ? "Energy saver" : "";
  };

  const select = (index: number) => {
    buttons.forEach((button, i) => {
      button.setAttribute("aria-pressed", String(i === index));
      const bar = button.querySelector<HTMLElement>(".moments-progress");
      if (bar) gsap.set(bar, { scaleX: 0 });
    });
  };

  const playMoment = async (index: number) => {
    const id = ++run;
    window.clearTimeout(timer);
    progress?.kill();
    current = index;
    const moment = crabMoments[index];
    select(index);
    if (caption) caption.textContent = `${moment.title}. ${moment.body}`;

    // The trigger first: the figure moves, or the battery changes.
    crab.stop();
    crab.show(false);
    if (moment.usage !== undefined) {
      setPower();
      const figure = { value: BEFORE[moment.act] ?? 63 };
      setUsage(figure.value);
      gsap.to(figure, {
        value: moment.usage,
        duration: crab.reduced ? 0 : LEAD_IN,
        ease: "power2.out",
        onUpdate: () => setUsage(figure.value),
      });
    } else {
      setUsage(63);
      setPower(moment.battery);
    }

    const bar = buttons[index].querySelector<HTMLElement>(".moments-progress");
    const seconds = crabScripts[moment.act].seconds;
    if (bar) {
      progress = gsap.fromTo(
        bar,
        { scaleX: 0 },
        { scaleX: 1, duration: LEAD_IN + seconds, ease: "none" },
      );
    }

    await new Promise((resolve) => window.setTimeout(resolve, LEAD_IN * 1000));
    if (id !== run) return;
    await crab.play(moment.act, perch(moment));
    if (id !== run) return;
    if (moment.battery) setPower();
    if (auto && visible) {
      timer = window.setTimeout(() => void playMoment((current + 1) % crabMoments.length), PAUSE_MS);
    }
  };

  buttons.forEach((button, index) => {
    button.addEventListener("click", () => {
      auto = false;
      void playMoment(index);
    });
  });

  // Hovering the crab while an act plays is the app's poke; clicking it replays the moment.
  el.addEventListener("click", () => void playMoment(current));

  watchVisible(
    desk,
    (now) => {
      visible = now;
      if (now && !crab.busy && auto) void playMoment(current);
      if (!now) {
        window.clearTimeout(timer);
        run += 1;
        crab.stop();
        crab.show(false);
        progress?.kill();
      }
    },
    0.45,
  );
}
