// BrowserPage — the ONE browser entry (round-162/163; round-261 mode-B
// removal). The JPEG screenshot stream (BrowserPane/useBrowser, the
// plain-web fallback) is GONE: the browser is a REAL embedded WebContentsView
// in the Electron desktop shell only. In a plain browser there is no
// screenshot path anymore — the page explains that the browser needs the
// desktop app.
import { EmbeddedBrowserPane } from "./EmbeddedBrowserPane";
import { Icon } from "../ui/Icon";

interface Props {
  token: string;
}

export function BrowserPage({ token }: Props) {
  // round-246: in the Electron shell the main process embeds a REAL
  // WebContentsView (window.valeEmbedded) — render the controller for it.
  const embedded = !!((window as any).valeEmbedded);
  if (embedded) {
    return (
      <div className="browser-page">
        {/* The page's name, for the outline only: this view is a pane, not a document with a
            visible title. */}
        <h1 className="sr-only">Browser</h1>
        <EmbeddedBrowserPane token={token} />
      </div>
    );
  }
  // round-261 (user: "模式 B 可以删除"): no screenshot-stream fallback.
  // The remote browser is only meaningful as the real embedded view in the
  // Vale desktop app; a plain web page cannot show it.
  return (
    <div className="browser-page">
      <h1 className="sr-only">Browser</h1>
      <div className="browser-mode-b-placeholder">
        <div className="browser-placeholder">
          {/* AN ICON, NOT AN EMOJI. This was `<span style={{fontSize:40}}>🖥</span>`:
              an emoji in the platform font, which renders as a TOFU BOX wherever that
              font has no colour glyph for it — exactly what the first screenshot of
              this page showed. It also broke two house rules at once (the icon system
              in ui/Icon.tsx owns every glyph in this app, and inline styles are not
              how this sheet sizes anything). */}
          <span className="browser-placeholder-mark" aria-hidden="true">
            <Icon name="browser" size={26} />
          </span>
          <p><strong>The browser needs the Vale desktop app</strong></p>
          <p className="browser-mode-b-hint">
            This page is served by the agent. Open it inside the Vale Desktop
            (Electron) shell to get the real embedded browser — plain web
            browsers cannot render it.
          </p>
        </div>
      </div>
    </div>
  );
}
