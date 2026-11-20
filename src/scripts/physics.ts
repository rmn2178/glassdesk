/* ============================================================================
   GLASSDESK — Zero-Idle-CPU Liquid Glass Spring Physics & Specular Refraction
   - Real second-order damped harmonic oscillator (semi-implicit Euler)
   - Frame-rate independent across 60Hz / 120Hz / 144Hz / 240Hz monitors
   - Self-sleeping RAF loop: 0% CPU when settled or when reduced-motion is on
   ============================================================================ */

interface SpringAxis {
  value: number;
  velocity: number;
  target: number;
}

class SpringDriver {
  private axes: Record<string, SpringAxis> = {};
  private stiffness: number;
  private damping: number;
  private rafId: number | null = null;
  private lastTs = 0;
  private onFrame: (values: Record<string, number>) => void;

  constructor(
    initial: Record<string, number>,
    stiffness: number,
    damping: number,
    onFrame: (values: Record<string, number>) => void
  ) {
    this.stiffness = stiffness;
    this.damping = damping;
    this.onFrame = onFrame;
    for (const [k, v] of Object.entries(initial)) {
      this.axes[k] = { value: v, velocity: 0, target: v };
    }
  }

  public setTargets(targets: Partial<Record<string, number>>): void {
    for (const [k, v] of Object.entries(targets)) {
      if (v !== undefined && this.axes[k]) {
        this.axes[k].target = v;
      }
    }
    this.wake();
  }

  public impulse(velocities: Partial<Record<string, number>>): void {
    for (const [k, dv] of Object.entries(velocities)) {
      if (dv !== undefined && this.axes[k]) {
        this.axes[k].velocity += dv;
      }
    }
    this.wake();
  }

  private wake(): void {
    if (this.rafId !== null) return;
    this.lastTs = performance.now();
    this.rafId = window.requestAnimationFrame(this.step);
  }

  private step = (ts: number): void => {
    const dt = Math.min((ts - this.lastTs) / 1000, 0.032);
    this.lastTs = ts;

    let stillMoving = false;
    const snapshot: Record<string, number> = {};

    for (const [k, axis] of Object.entries(this.axes)) {
      const displacement = axis.value - axis.target;
      const accel = -this.stiffness * displacement - this.damping * axis.velocity;
      axis.velocity += accel * dt;
      axis.value += axis.velocity * dt;

      if (Math.abs(axis.value - axis.target) > 0.0006 || Math.abs(axis.velocity) > 0.0006) {
        stillMoving = true;
      } else {
        axis.value = axis.target;
        axis.velocity = 0;
      }
      snapshot[k] = axis.value;
    }

    this.onFrame(snapshot);

    if (stillMoving) {
      this.rafId = window.requestAnimationFrame(this.step);
    } else {
      this.rafId = null;
    }
  };
}

export function initLiquidGlassPhysics(): void {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    return;
  }

  const island = document.getElementById("dynamic-island");
  if (island) {
    const spring = new SpringDriver(
      { tx: 0, ty: 0, sx: 1, sy: 1, gx: 50, gy: 50, op: 0 },
      340,
      24,
      (v) => {
        const atRest =
          v.tx === 0 && v.ty === 0 && v.sx === 1 && v.sy === 1 && v.op === 0;
        island.style.transform = atRest
          ? ""
          : `translate3d(${v.tx.toFixed(2)}px, ${v.ty.toFixed(2)}px, 0) scale3d(${v.sx.toFixed(4)}, ${v.sy.toFixed(4)}, 1)`;
        island.style.setProperty("--gx", `${v.gx.toFixed(1)}%`);
        island.style.setProperty("--gy", `${v.gy.toFixed(1)}%`);
        island.style.setProperty("--g-op", v.op.toFixed(3));
      }
    );

    let isPointerDown = false;

    island.addEventListener("pointerenter", () => {
      spring.setTargets({ sx: 1.032, sy: 1.022, op: 1 });
    });

    island.addEventListener("pointermove", (e) => {
      const rect = island.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const nx = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
      const ny = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
      const tx = (nx - 0.5) * 6.5;
      const ty = (ny - 0.5) * 2.2;
      const scaleX = isPointerDown ? 0.975 : 1.034;
      const scaleY = isPointerDown ? 0.962 : 1.024;
      spring.setTargets({
        tx,
        ty,
        sx: scaleX,
        sy: scaleY,
        gx: nx * 100,
        gy: ny * 100,
        op: 1,
      });
    });

    island.addEventListener("pointerdown", () => {
      isPointerDown = true;
      spring.setTargets({ sx: 0.972, sy: 0.958 });
    });

    const release = () => {
      if (!isPointerDown) return;
      isPointerDown = false;
      spring.impulse({ sx: 0.32, sy: 0.24 });
      spring.setTargets({ sx: 1.03, sy: 1.02 });
    };

    island.addEventListener("pointerup", release);

    island.addEventListener("pointerleave", () => {
      isPointerDown = false;
      const expanded = island.classList.contains("is-expanded");
      spring.setTargets({
        tx: 0,
        ty: 0,
        sx: expanded ? 1.035 : 1,
        sy: expanded ? 1.015 : 1,
        gx: 50,
        gy: 50,
        op: 0,
      });
    });
  }

  // Coalesced pointer-tracked specular refraction across open Liquid Glass panels
  const panelHost = document.getElementById("panel-host");
  if (panelHost) {
    let pendingEvent: PointerEvent | null = null;
    let hostRaf: number | null = null;

    const flushPanelSpecular = () => {
      hostRaf = null;
      const e = pendingEvent;
      pendingEvent = null;
      if (!e) return;

      const panel = (e.target as HTMLElement | null)?.closest<HTMLElement>(".glass-panel");
      if (!panel) return;
      const rect = panel.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;

      const x = ((e.clientX - rect.left) / rect.width) * 100;
      const y = ((e.clientY - rect.top) / rect.height) * 100;
      panel.style.setProperty("--px", `${x.toFixed(1)}%`);
      panel.style.setProperty("--py", `${y.toFixed(1)}%`);
    };

    panelHost.addEventListener(
      "pointermove",
      (e) => {
        pendingEvent = e;
        if (hostRaf === null) {
          hostRaf = window.requestAnimationFrame(flushPanelSpecular);
        }
      },
      { passive: true }
    );
  }
}
