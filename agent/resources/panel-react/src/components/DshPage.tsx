// DshPage — the ONE harness entry, the sibling of BrowserPage.
//
// The harness (DSH) runs on THIS machine and the desktop shell embeds it as a real
// WebContentsView. A page served to a plain browser has nothing to position, so it explains that
// instead of rendering a slot that can never fill — the same choice BrowserPage makes, for the
// same reason.
import { useState } from "react";
import { DshPane } from "./DshPane";
import { HarnessHosts } from "./HarnessHosts";
import { Icon } from "../ui/Icon";

/**
 * THE HARNESS PAGE IS TWO PANES: hosts on the left, the SELECTED host's own harness on the right.
 *
 * The right pane is the harness's native WebContentsView, and it is the same ONE view whatever host is
 * selected — the main process navigates it to that host's forward, which is why the column has to ask the
 * main process rather than navigate anything itself. The left pane is the only part the panel draws for a
 * host: the files, the editor, the command line and the terminal are all the far side's, which is the
 * arrangement the operator chose after the panel had been drawing its own copy of them.
 */
export function DshPage() {
  const embedded = !!((window as unknown as { summriseDsh?: unknown }).summriseDsh);
  const [selected, setSelected] = useState<string | null>(null);
  if (embedded) {
    return (
      <div className="dsh-page">
        {/* The page's name, for the outline only: this view is a pane, not a document with a
            visible title. */}
        <h1 className="sr-only">Harness</h1>
        <div className="dsh-two-pane">
          <HarnessHosts selected={selected} onSelect={setSelected} />
          <DshPane />
        </div>
      </div>
    );
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
