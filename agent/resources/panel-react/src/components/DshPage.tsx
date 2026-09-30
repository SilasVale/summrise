// DshPage — the ONE harness entry, the sibling of BrowserPage.
//
// The harness (DSH) runs on THIS machine and the desktop shell embeds it as a real
// WebContentsView. A page served to a plain browser has nothing to position, so it explains that
// instead of rendering a slot that can never fill — the same choice BrowserPage makes, for the
// same reason.
import { useState } from "react";
import { DshPane } from "./DshPane";
import { WorkspacesPanel } from "./WorkspacesPanel";
import { Icon } from "../ui/Icon";

export function DshPage() {
  const embedded = !!((window as unknown as { summriseDsh?: unknown }).summriseDsh);
  if (embedded) {
    return <EmbeddedHarness />;
  }
  return (
    <div className="dsh-page">
      <h1 className="sr-only">Harness</h1>
      <div className="browser-mode-b-placeholder">
        <div className="browser-placeholder">
          <span className="browser-placeholder-mark" aria-hidden="true">
            <Icon name="harness" size={26} />
          </span>
          <p><strong>The harness needs the Summrise desktop app</strong></p>
          <p className="browser-mode-b-hint">
            This page embeds the harness in the Summrise Desktop (Electron) shell. A plain web
            browser cannot show it — open this device in the desktop app instead.
          </p>
        </div>
      </div>
    </div>
  );
}

/**
 * The embedded harness, with the machine/workspace panel above it.
 *
 * THE PANEL IS COLLAPSED BY DEFAULT AND THAT IS THE POINT: the harness is the page, and a form that pushed it down
 * on every visit would be the tail wagging the dog. Expanded, it takes height from the slot — and `DshPane`
 * re-reports the slot's bounds through its own ResizeObserver, so the native view follows without either side
 * knowing about the other.
 */
function EmbeddedHarness() {
  const [open, setOpen] = useState(false);
  return (
    <div className="dsh-page">
      {/* The page's name, for the outline only: this view is a pane, not a document with a visible title. */}
      <h1 className="sr-only">Harness</h1>
      <div className="dsh-machines">
        <button
          className="btn btn-mini"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          aria-label="Workspace hosts"
        >
          {open ? "Hide workspace hosts" : "Workspace hosts…"}
        </button>
      </div>
      {open ? <WorkspacesPanel /> : null}
      <DshPane />
    </div>
  );
}
