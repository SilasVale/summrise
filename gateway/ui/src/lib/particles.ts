/**
 * The particle field: slow-drifting iridescent motes behind every surface.
 *
 * DECORATIVE ONLY, and that is a hard rule here rather than a style preference — the
 * project's token layer already states it ("no text colour is ever taken from it, so no
 * contrast measurement moves"). So this layer is:
 *   * a FIXED canvas at z-index 0, below `#root` at z-index 1, with `pointer-events: none`,
 *     so it can never intercept a click or enter the layout;
 *   * drawn at low alpha from colours the page resolves for it — the three names below are
 *     RETIRED (round 235): `--aura-*` was removed in the rebrand, globals.css records that it
 *     "used to sit in this slot", and these readers were left behind. `pick` falls back when a
 *     token reads empty, so the particles quietly draw the PRE-REBRAND hues (190 cyan, 280
 *     violet, 330 pink) instead of the brand's. Choosing the replacement is a design decision,
 *     recorded in docs/agents/ideas.md — the fallbacks stay until it is made.
 *     behind existing surfaces rather than a new contrast pair to measure.
 *
 * IT RESPECTS `prefers-reduced-motion` BY NOT RUNNING AT ALL. A decorative animation that
 * ignores that setting is an accessibility defect, and the honest fallback is the static
 * wash the page already has — not a slower animation.
 *
 * COST IS BOUNDED BY AREA, not by a constant: a 4K window gets more motes than a laptop,
 * but the count is capped so a full-screen browser cannot spend the frame budget on
 * decoration. The loop also stops when the tab is hidden.
 */

interface Mote {
  x: number;
  y: number;
  r: number;
  vx: number;
  vy: number;
  /** WHICH palette entry this mote wears — resolved per frame, so a theme switch reaches motes already in flight. */
  tone: number;
  phase: number;
}

/** Motes per 100k px² of viewport, and a ceiling regardless of size. */
const DENSITY = 0.55;
const MAX_MOTES = 90;
/** Comfortably below any text; the field reads as a tint, not as content. */
const MAX_ALPHA = 0.5;

export function startParticleField(): () => void {
  if (typeof window === "undefined") return () => {};
  // The honest fallback: no animation, and the static wash stays visible.
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return () => {};

  const canvas = document.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.dataset.summriseParticles = "1";
  Object.assign(canvas.style, {
    position: "fixed",
    inset: "0",
    zIndex: "0",
    pointerEvents: "none",
    display: "block",
  } satisfies Partial<CSSStyleDeclaration>);
  document.body.appendChild(canvas);

  const ctx = canvas.getContext("2d");
  if (!ctx) {
    canvas.remove();
    return () => {};
  }

  let motes: Mote[] = [];
  let raf = 0;
  let dpr = 1;

  const readPalette = (): Array<[number, number, number]> => {
    // ══ THE FIELD WEARS THE BRAND (round 14 of the standing goal) ═════════════════════════════════════════════
    // It read `--aura-1/3/4` — a palette the rebrand RETIRED — and fell back to 190 cyan, 280 violet and 330
    // pink, so the ambient layer drawn behind EVERY surface was in the colours the brand had abandoned. This is
    // the decision the operator's inbox recorded (docs/agents/ideas.md row 15): use the brand's own palette.
    //
    // AS COLOURS, NOT HUES. The old code turned a token into a hue and then drew it as `hsla(h 90% 62%)` — so a
    // correct token would still not have been the colour on screen. A mote now carries the palette INDEX and
    // resolves it per frame, which also means a theme switch recolours the motes already in flight instead of
    // only the ones born after it.
    const cs = getComputedStyle(document.body);
    const pick = (name: string, fallback: string): [number, number, number] => {
      const raw = cs.getPropertyValue(name).trim() || fallback;
      const hex = /^#([0-9a-f]{6})$/i.exec(raw);
      if (hex) {
        const n = parseInt(hex[1], 16);
        return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
      }
      const rgb = /^rgba?\(([^)]+)\)$/.exec(raw);
      if (rgb) {
        const p = rgb[1].split(/[\s,/]+/).filter(Boolean).map(Number);
        if (p.length >= 3 && p.slice(0, 3).every((v) => !Number.isNaN(v))) return [p[0], p[1], p[2]];
      }
      // The brand's first stop, if a token is unreadable — never a colour the brand does not use.
      return [0xc2, 0x41, 0x0c];
    };
    return [
      pick("--brand-grad-a", "#c2410c"),
      pick("--brand-grad-b", "#9a3412"),
      pick("--accent", "#bf3a0a"),
    ];
  };

  let palette = readPalette();

  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = window.innerWidth;
    const h = window.innerHeight;
    canvas.width = Math.floor(w * dpr);
    canvas.height = Math.floor(h * dpr);
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    palette = readPalette();

    const want = Math.min(MAX_MOTES, Math.round(((w * h) / 100_000) * DENSITY * 10));
    while (motes.length > want) motes.pop();
    while (motes.length < want) {
      motes.push({
        x: Math.random() * w,
        y: Math.random() * h,
        r: 0.6 + Math.random() * 1.9,
        vx: (Math.random() - 0.5) * 0.16,
        vy: -0.05 - Math.random() * 0.18,
        tone: Math.floor(Math.random() * palette.length),
        phase: Math.random() * Math.PI * 2,
      });
    }
  };

  let last = 0;
  const frame = (t: number) => {
    raf = requestAnimationFrame(frame);
    // ~30fps is plenty for a drift this slow and halves the cost.
    if (t - last < 33) return;
    last = t;
    const w = window.innerWidth;
    const h = window.innerHeight;
    ctx.clearRect(0, 0, w, h);
    for (const m of motes) {
      m.x += m.vx;
      m.y += m.vy;
      m.phase += 0.012;
      // Wrap rather than respawn, so the field never visibly pops.
      if (m.y < -8) m.y = h + 8;
      if (m.y > h + 8) m.y = -8;
      if (m.x < -8) m.x = w + 8;
      if (m.x > w + 8) m.x = -8;
      const twinkle = 0.55 + 0.45 * Math.sin(m.phase);
      ctx.beginPath();
      const [r, g, b] = palette[m.tone] ?? palette[0];
      ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${(MAX_ALPHA * twinkle * 0.35).toFixed(3)})`;
      ctx.arc(m.x, m.y, m.r, 0, Math.PI * 2);
      ctx.fill();
    }
  };

  const onVisibility = () => {
    if (document.hidden) {
      cancelAnimationFrame(raf);
      raf = 0;
    } else if (!raf) {
      raf = requestAnimationFrame(frame);
    }
  };

  resize();
  window.addEventListener("resize", resize);
  document.addEventListener("visibilitychange", onVisibility);
  raf = requestAnimationFrame(frame);

  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
    document.removeEventListener("visibilitychange", onVisibility);
    canvas.remove();
  };
}
