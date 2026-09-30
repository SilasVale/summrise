// DshPage — the ONE harness entry, the sibling of BrowserPage.
//
// The harness (DSH) runs on THIS machine and the desktop shell embeds it as a real
// WebContentsView. A page served to a plain browser has nothing to position, so it explains that
// instead of rendering a slot that can never fill — the same choice BrowserPage makes, for the
// same reason.
import { DshPane } from "./DshPane";
import { Icon } from "../ui/Icon";

export function DshPage() {
  const embedded = !!((window as unknown as { summriseDsh?: unknown }).summriseDsh);
  if (embedded) {
    return (
      <div className="dsh-page">
        {/* The page's name, for the outline only: this view is a pane, not a document with a
            visible title. */}
        <h1 className="sr-only">Harness</h1>
        <DshPane />
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
