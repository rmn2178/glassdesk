/**
 * Rotates the mock desktop wallpaper like a slideshow. A shuffled bag keeps the order
 * random while showing every picture once before any repeats.
 */

const HOLD_MS = 7000;

function shuffle(length: number): number[] {
  const order = Array.from({ length }, (_, index) => index);

  for (let index = order.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(Math.random() * (index + 1));
    [order[index], order[swap]] = [order[swap], order[index]];
  }

  return order;
}

function parseUrls(raw: string | undefined): string[] {
  if (!raw) return [];

  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

export function initWallpaper(root: HTMLElement, reduceMotion: boolean) {
  const layers = [...root.querySelectorAll<HTMLElement>("[data-wallpaper-layer]")];
  const urls = parseUrls(root.dataset.wallpaperUrls);
  if (layers.length < 2 || !urls.length) return;

  let shown = Math.floor(Math.random() * urls.length);
  let visible = 0;

  layers[visible].style.backgroundImage = `url("${urls[shown]}")`;
  layers[visible].classList.add("is-active");

  // A slideshow is decoration: with reduced motion one picture stays and no timer runs.
  if (reduceMotion || urls.length === 1) return;

  let bag = shuffle(urls.length);
  let timer = 0;

  const pick = () => {
    if (!bag.length) {
      bag = shuffle(urls.length);
      // A fresh bag must not open with the picture already on screen.
      if (urls.length > 1 && bag[bag.length - 1] === shown) {
        [bag[bag.length - 1], bag[bag.length - 2]] = [bag[bag.length - 2], bag[bag.length - 1]];
      }
    }

    return bag.pop() ?? 0;
  };

  const preload = (url: string) =>
    new Promise<void>((resolve) => {
      const image = new Image();
      image.onload = () => resolve();
      image.onerror = () => resolve();
      image.src = url;
    });

  const swap = async () => {
    const nextShown = pick();
    const incoming = layers[visible === 0 ? 1 : 0];

    // Without the decoded picture the fade would reveal an empty layer.
    await preload(urls[nextShown]);
    incoming.style.backgroundImage = `url("${urls[nextShown]}")`;
    void incoming.offsetWidth;
    incoming.classList.add("is-active");
    layers[visible].classList.remove("is-active");

    visible = visible === 0 ? 1 : 0;
    shown = nextShown;
  };

  const schedule = () => {
    timer = window.setTimeout(async () => {
      // Hidden tabs throttle timers; skipping keeps the sequence where it was.
      if (!document.hidden) await swap();
      schedule();
    }, HOLD_MS);
  };

  schedule();

  window.addEventListener(
    "pagehide",
    () => {
      window.clearTimeout(timer);
    },
    { once: true },
  );
}
