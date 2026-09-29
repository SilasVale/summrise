// vitest setup — jsdom polyfills that xterm.js needs (matchMedia, animation frames).
import { vi } from "vitest";

// AN ANIMATION FRAME THAT OUTLIVES THE TEST (round 29 of the standing goal). xterm's Viewport schedules a refresh on a
// timer, and on a loaded CI runner that callback can land AFTER the test environment is torn down — where jsdom's
// `requestAnimationFrame` is gone and the late call throws `this._coreBrowserService.window.requestAnimationFrame is
// not a function`. Vitest counts an unhandled error as a FAILED RUN even when every test passed, so one xterm timer
// turned a green panel job red (measured: 804 passed, 1 error, job failed).
//
// It could not be reproduced locally in eight runs — five of the terminal test alone, three of the whole suite — which
// is exactly why the fix is structural rather than a reproduction: with a frame function installed on the window by
// this file, a callback that arrives late finds something to call instead of throwing. `setTimeout(0)` is the same
// shape jsdom uses, and the ids are kept distinct so a cancel cannot collide with a real timer.
// INSTALLED EVEN THOUGH JSDOM HAS ONE, and that is the whole point: jsdom's frame function is what disappears when the
// environment is torn down, so a guard of `!== "function"` skipped this polyfill during the test and left the late
// callback exactly as exposed as before. (Written the guarded way first, and the panel suite still passed — which is
// how a fix that fixes nothing looks.)
if (typeof window !== "undefined") {
  let next = 1;
  const live = new Map<number, ReturnType<typeof setTimeout>>();
  Object.defineProperty(window, "requestAnimationFrame", {
    writable: true,
    value: (cb: FrameRequestCallback) => {
      const id = next++;
      live.set(
        id,
        setTimeout(() => {
          live.delete(id);
          cb(Date.now());
        }, 0),
      );
      return id;
    },
  });
  Object.defineProperty(window, "cancelAnimationFrame", {
    writable: true,
    value: (id: number) => {
      const t = live.get(id);
      if (t !== undefined) {
        clearTimeout(t);
        live.delete(id);
      }
    },
  });
}

if (typeof window !== "undefined" && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

// WHAT IS PINNED AND WHAT IS NOT: `src/lib/animationFrame.test.ts` proves frames are delivered and cancels are safe
// inside a test. It cannot prove the property this polyfill exists for — a frame function surviving TEARDOWN — and the
// flake does not reproduce locally, so the claim "this ends it" is a construction argument, not a measurement. The
// next few CI runs are the measurement.

// ── THE PANEL'S RUST IS LOADED BEFORE ANY TEST BODY RUNS (2026-09-29) ────────────────────────────
//
// `wasm/panelLogic.ts` now exposes `logic()`, the SYNCHRONOUS door a render-path call site uses, and
// it THROWS rather than falling back to a JavaScript copy. The browser guarantees the load in
// `main.tsx`; this is the same guarantee for the suite, in the one place every test file passes
// through — so a component test needs no `beforeAll`, and a migrated function cannot be tested
// against a different program than the one the operator's browser runs.
//
// THE SAME BYTES, READ THE SAME WAY: `panelLogic()` under vitest reads
// `agent/resources/panel/panel_logic_bg.wasm` off disk and `initSync`s it (there is no server to
// fetch from), which is the arrangement the seam has had since the first family moved.
//
// A TOP-LEVEL AWAIT IS THE POINT, not a convenience: vitest finishes loading this file before the
// first test file is imported, so `logic()` cannot be reached before the module is in memory.
import { panelLogic } from "./wasm/panelLogic";
await panelLogic();
