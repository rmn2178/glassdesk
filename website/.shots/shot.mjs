// Dev-only screenshot helper: drives the installed Chrome over CDP so a section
// below the fold can be captured after scrolling (headless --screenshot cannot scroll).
// Usage: node .shots/shot.mjs <url> <out.png> <width> <height> <selector|y>
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const CHROME = "C:/Program Files/Google/Chrome/Application/chrome.exe";
const PORT = 9333;
const [, , url = "http://localhost:4321/", out = ".shots/shot.png", w = "1440", h = "1000", target = "0"] =
  process.argv;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const chrome = spawn(
  CHROME,
  [
    "--headless=new",
    `--remote-debugging-port=${PORT}`,
    "--user-data-dir=C:/Users/USUARIO/AppData/Local/Temp/nosih-cdp",
    "--hide-scrollbars",
    "--no-first-run",
    "--disable-gpu",
    "about:blank",
  ],
  { stdio: "ignore" },
);

let ws;
try {
  for (let i = 0; i < 60; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/json/new?about:blank`, { method: "PUT" });
      if (res.ok) {
        ws = new WebSocket((await res.json()).webSocketDebuggerUrl);
        break;
      }
    } catch {
      await sleep(250);
    }
  }
  if (!ws) throw new Error("no CDP target");

  await new Promise((r) => ws.addEventListener("open", r, { once: true }));

  let seq = 0;
  const pending = new Map();
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    const done = pending.get(msg.id);
    if (done) {
      pending.delete(msg.id);
      done(msg.result ?? {});
      return;
    }
    if (msg.method === "Runtime.exceptionThrown") {
      const d = msg.params.exceptionDetails;
      console.log("EXCEPTION:", d.exception?.description ?? d.text);
    }
    if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") {
      console.log("CONSOLE:", msg.params.args.map((a) => a.value ?? a.description).join(" "));
    }
  });
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      const id = ++seq;
      pending.set(id, resolve);
      ws.send(JSON.stringify({ id, method, params }));
    });

  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", {
    width: Number(w),
    height: Number(h),
    deviceScaleFactor: 1,
    mobile: false,
  });
  await send("Page.navigate", { url });

  for (let i = 0; i < 80; i++) {
    const state = await send("Runtime.evaluate", { expression: "document.readyState" });
    if (state.result?.value === "complete") break;
    await sleep(150);
  }
  await sleep(1500);

  const scroll = /^\d+$/.test(target)
    ? Number(target)
    : ((
        await send("Runtime.evaluate", {
          expression: `Math.max(0, document.querySelector(${JSON.stringify(target)}).getBoundingClientRect().top + scrollY - 40)`,
        })
      ).result?.value ?? 0);
  await send("Runtime.evaluate", { expression: `scrollTo(0, ${scroll})` });
  await sleep(1800);

  if (process.env.PROBE) {
    const expression = readFileSync(process.env.PROBE, "utf8");
    const out = await send("Runtime.evaluate", {
      expression: `(() => { ${expression} })()`,
      returnByValue: true,
      awaitPromise: true,
    });
    console.log("probe:", out.result?.value ?? JSON.stringify(out));
  }

  const hover = process.argv[7];
  if (hover) {
    const [hx, hy] = hover.split(",").map(Number);
    await send("Page.bringToFront");
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: hx, y: hy, button: "none", buttons: 0 });
    await sleep(700);
    const probe = await send("Runtime.evaluate", {
      expression: `(() => {
        const card = document.querySelectorAll("[data-spotlight]")[1];
        return JSON.stringify({
          mx: card.style.getPropertyValue("--mx"),
          tiltX: card.style.getPropertyValue("--tx"),
          slab: getComputedStyle(card.querySelector(".slab")).transform,
        });
      })()`,
    });
    console.log("hover:", probe.result?.value);

    // Headless CDP mouse input is not always delivered; a synthetic event still proves
    // the listeners are attached and writing the variables the CSS reads.
    await send("Runtime.evaluate", {
      expression: `(() => {
        try {
          const card = document.querySelectorAll("[data-spotlight]")[1];
          card.dispatchEvent(new PointerEvent("pointermove", { clientX: ${hx}, clientY: ${hy}, bubbles: true }));
          return JSON.stringify({
            mx: card.style.getPropertyValue("--mx"),
            my: card.style.getPropertyValue("--my"),
            tiltX: card.style.getPropertyValue("--tx"),
            slab: getComputedStyle(card.querySelector(".slab")).transform,
          });
        } catch (error) {
          return "ERR " + error.message;
        }
      })()`,
    }).then((r) => console.log("synthetic:", r.result?.value));
  }

  const state = await send("Runtime.evaluate", {
    expression: `JSON.stringify({
      progress: (() => {
        const note = document.querySelector("[data-note]");
        if (!note) return null;
        const top = note.getBoundingClientRect().top + scrollY;
        const start = Math.max(0, top - innerHeight * 0.55);
        return Math.min(1, Math.max(0, (scrollY - start) / innerHeight)).toFixed(2);
      })(),
      slot: document.querySelector("[data-dock-thumb]")?.style.getPropertyValue("--slot") ?? "",
      thumbWidth: Math.round(document.querySelector("[data-dock-thumb]")?.getBoundingClientRect().width ?? 0),
      genieTarget: document.querySelector(".desk")?.dataset.genieTarget ?? "",
      genieDesk: (() => {
        const desk = document.querySelector(".desk").getBoundingClientRect();
        return [Math.round(desk.left), Math.round(desk.top), Math.round(desk.width), Math.round(desk.height)];
      })(),
      target: (() => {
        const desk = document.querySelector(".desk");
        const slot = document.querySelector("[data-dock-thumb]");
        if (!desk || !slot) return null;
        const items = [...desk.querySelectorAll(".dock-item")];
        const saved = items.map((item) => [item.style.width, item.style.height]);
        items.forEach((item) => { item.style.width = "52px"; item.style.height = "52px"; });
        const previous = slot.style.getPropertyValue("--slot");
        slot.style.setProperty("--slot", "1");
        const rect = slot.getBoundingClientRect();
        if (previous) slot.style.setProperty("--slot", previous); else slot.style.removeProperty("--slot");
        items.forEach((item, index) => { item.style.width = saved[index][0]; item.style.height = saved[index][1]; });
        const base = desk.getBoundingClientRect();
        return [Math.round(rect.left + rect.width / 2 - base.left), Math.round(rect.top + rect.height / 2 - base.top)];
      })(),
      slotNow: (() => {
        const desk = document.querySelector(".desk").getBoundingClientRect();
        const rect = document.querySelector("[data-dock-thumb]").getBoundingClientRect();
        return [Math.round(rect.left + rect.width / 2 - desk.left), Math.round(rect.top + rect.height / 2 - desk.top)];
      })(),
    })`,
  });
  console.log("page:", state.result?.value);

  if (process.env.INTERACT === "1") {
    const clickAt = async ({ x, y }) => {
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none", buttons: 0 });
      await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
      await sleep(300);
    };

    const centreOf = async (selector) => {
      const found = await send("Runtime.evaluate", {
        returnByValue: true,
        expression: `(() => { const el = ${selector}; if (!el) return null; const b = el.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`,
      });
      return found.result?.value;
    };

    // A real click is what grants the transient activation the clipboard needs.
    await send("Browser.grantPermissions", {
      permissions: ["clipboardReadWrite", "clipboardSanitizedWrite"],
      origin: "http://localhost:4321",
    });

    const tabCentre = await centreOf(`document.querySelectorAll("[data-tab]")[1]`);
    if (tabCentre) await clickAt(tabCentre);

    const key = async (name, code, vk) => {
      await send("Input.dispatchKeyEvent", {
        type: "keyDown",
        key: name,
        code,
        windowsVirtualKeyCode: vk,
        nativeVirtualKeyCode: vk,
      });
      await send("Input.dispatchKeyEvent", { type: "keyUp", key: name, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk });
      await sleep(200);
    };

    await key("ArrowRight", "ArrowRight", 39);
    const keyboard = await send("Runtime.evaluate", {
      returnByValue: true,
      expression: `JSON.stringify({ focus: document.activeElement?.id, selected: [...document.querySelectorAll("[data-tab]")].map((t) => t.getAttribute("aria-selected")) })`,
    });
    console.log("keyboard:", keyboard.result?.value);

    if (tabCentre) await clickAt(tabCentre);

    const copyCentre = await centreOf(`document.querySelector("[data-panel]:not([hidden]) [data-copy]")`);
    if (copyCentre) await clickAt(copyCentre);
    await sleep(400);

    const result = await send("Runtime.evaluate", {
      awaitPromise: true,
      returnByValue: true,
      expression: `(async () => {
        const tabs = [...document.querySelectorAll("[data-tab]")];
        const copy = document.querySelector("[data-panel]:not([hidden]) [data-copy]");
        const clip = await navigator.clipboard.readText().catch((e) => "ERR " + e.name);
        return JSON.stringify({
          selected: tabs.map((t) => t.getAttribute("aria-selected")),
          hidden: [...document.querySelectorAll("[data-panel]")].map((p) => p.hidden),
          copied: copy.dataset.copied,
          clip,
          label: copy.getAttribute("aria-label"),
        });
      })()`,
    });
    console.log("interact:", result.result?.value);
  }

  if (process.env.ACTION === "restore") {
    const tap = async ({ x, y }) => {
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none", buttons: 0 });
      await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
      await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
      await sleep(300);
    };
    const centre = async (selector) => {
      const found = await send("Runtime.evaluate", {
        returnByValue: true,
        expression: `(() => { const el = ${selector}; if (!el) return null; const b = el.getBoundingClientRect(); return { x: b.left + b.width / 2, y: b.top + b.height / 2 }; })()`,
      });
      return found.result?.value;
    };

    const slotCentre = await centre(`document.querySelector("[data-dock-restore]")`);
    if (slotCentre) await tap(slotCentre);
    await sleep(1500);

    const after = await send("Runtime.evaluate", {
      returnByValue: true,
      expression: `JSON.stringify({
        progress: (() => { const n = document.querySelector("[data-note]"); const top = n.getBoundingClientRect().top + scrollY; const start = Math.max(0, top - innerHeight * 0.55); return Math.min(1, Math.max(0, (scrollY - start) / innerHeight)).toFixed(2); })(),
        slot: document.querySelector("[data-dock-thumb]").style.getPropertyValue("--slot"),
        thumbWidth: Math.round(document.querySelector("[data-dock-thumb]").getBoundingClientRect().width),
        noteVisibility: getComputedStyle(document.querySelector("[data-note]")).visibility,
      })`,
    });
    console.log("restore:", after.result?.value);
  }

  const clip = process.env.CLIP?.split(",").map(Number);
  const options = clip ? { clip: { x: clip[0], y: clip[1], width: clip[2], height: clip[3], scale: 2 } } : {};
  const shot = out.endsWith(".jpg")
    ? await send("Page.captureScreenshot", { format: "jpeg", quality: 80, captureBeyondViewport: false, ...options })
    : await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false, ...options });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log(`saved ${out} at y=${scroll}`);
  await send("Browser.close");
} finally {
  try {
    ws?.close();
  } catch {}
  setTimeout(() => chrome.kill(), 300);
}
