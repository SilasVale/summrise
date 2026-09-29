// react-jsx: no React import needed
import "./lib/theme"; // theme applies before any render (light default)
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { startParticleField } from "./lib/particles";
import { panelLogic } from "./wasm/panelLogic";

// Decorative, below every surface, and a no-op under `prefers-reduced-motion` — see the
// module header. Started before render so the field is behind the first paint.
startParticleField();

// The panel mounts into #root (the agent's index.html hosts it). React 18
// createRoot replaces the old DOM-manipulation bootstrap. The boundary
// catches render crashes in any page — a broken page must never take the
// whole panel down to a silent white screen (round-161).
//
// ── AND THE RUST IS LOADED BEFORE THE FIRST RENDER (2026-09-29) ─────────────────────────────────
//
// `index.html` PRELOADS `panel_logic_bg.wasm` at parse time — in parallel with panel.js's own
// download, which is ~11× larger — so this await joins a request that has been in flight since
// before the bundle arrived. That is what makes a migrated function callable DURING RENDER, which
// is the constraint P2 was stuck on (`wasm/panelLogic.ts`'s header carries the measurement).
//
// A FAILED LOAD MUST NOT BLANK THE PANEL. The promise is memoized and a rejection reaches every
// migrated call site, which is the behaviour that existed before this line; what this branch adds is
// that the page still renders. A blank screen would be a worse failure than a panel whose migrated
// features are broken.
const rootEl = document.getElementById("root");
if (rootEl) {
  const root = createRoot(rootEl);
  void panelLogic()
    .catch(() => {})
    .then(() => {
      root.render(
        <ErrorBoundary>
          <App />
        </ErrorBoundary>,
      );
    });
}
