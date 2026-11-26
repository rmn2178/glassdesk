import type { ImageMetadata } from "astro";

import desktop01 from "@/assets/wallpapers/desktop-01.jpg";
import desktop02 from "@/assets/wallpapers/desktop-02.jpg";
import desktop03 from "@/assets/wallpapers/desktop-03.jpg";
import desktop04 from "@/assets/wallpapers/desktop-04.jpg";
import desktop05 from "@/assets/wallpapers/desktop-05.jpg";
import desktop06 from "@/assets/wallpapers/desktop-06.jpg";
import desktop07 from "@/assets/wallpapers/desktop-07.jpg";
import desktop08 from "@/assets/wallpapers/desktop-08.jpg";
import desktop09 from "@/assets/wallpapers/desktop-09.jpg";
import desktop10 from "@/assets/wallpapers/desktop-10.jpg";

/**
 * The openvid.dev desktop set (desktop-01 … desktop-10), committed under `src/assets`
 * so the page keeps its promise of zero third-party requests at runtime.
 */
export const wallpapers: ImageMetadata[] = [
  desktop01,
  desktop02,
  desktop03,
  desktop04,
  desktop05,
  desktop06,
  desktop07,
  desktop08,
  desktop09,
  desktop10,
];
