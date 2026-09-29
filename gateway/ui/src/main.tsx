import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AuthProvider } from "./contexts/AuthContext.tsx";
import { ToastProvider } from "./contexts/ToastContext.tsx";
import App from "./App.tsx";
// Theme first: applies body[data-theme] before the first paint (no flash).
import "./lib/theme.ts";
import "./styles/globals.css";
import { startParticleField } from "./lib/particles.ts";
import { consoleLogic } from "./wasm/consoleLogic.ts";

// Decorative, below every surface, and a no-op under `prefers-reduced-motion` — see the
// module header. Started before render so the field is behind the first paint.
startParticleField();

// ── AND THE RUST IS LOADED BEFORE THE FIRST RENDER (2026-09-29) ─────────────────────────────────
//
// `index.html` PRELOADS AND COMPILES `ui_logic_bg.wasm` while the bundle is still downloading, so
// this await joins a request that has been in flight since before the bundle arrived. That is what
// makes a migrated function callable DURING RENDER — the constraint block ③ inherited from the
// panel, and the reason `lane.ts` could not be wired before now.
//
// A FAILED LOAD MUST NOT BLANK THE CONSOLE. The promise is memoized and a rejection reaches every
// migrated call site, which is the behaviour that existed before this line; what this branch adds is
// that the page still renders.
const root = createRoot(document.getElementById("root")!);
void consoleLogic()
  .catch(() => {})
  .then(() => {
    root.render(
      <StrictMode>
        <AuthProvider>
          <ToastProvider>
            <App />
          </ToastProvider>
        </AuthProvider>
      </StrictMode>,
    );
  });
