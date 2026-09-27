import { Component, type ReactNode } from "react";
import { BrandMark } from "../ui/Icon";

// App-level error boundary (round-161): a crash in any page used to unmount
// the whole React tree — a silent white panel with no hint. Show an error
// card with a reload button instead. Reloading is always safe: terminal
// sessions live in the agent service, not in this window.
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    try {
      console.error("[summrise] render crash:", error, info?.componentStack || "");
    } catch { /* console unavailable */ }
  }

  render() {
    if (this.state.error) {
      return (
        <div className="app-crash">
          <div className="app-crash-card">
            {/* **THE MARK, NOT THE LETTER** — the same change the empty state took in this round, and for the same reason:
                `V` is not in the product's name while `brand/logo.svg` (THE ONE MARK) is what the tray, the rail and the console
                already draw. This tile carried the brand gradient AND a white `V`, so a reader saw a letter on a warm square;
                `BrandMark` brings its own gradients, so the tile is neutral now. */}
            <div className="app-crash-mark"><BrandMark size={28} /></div>
            <h1>Something broke</h1>
            <p className="muted">{this.state.error.message || "Render error"}</p>
            <button
              className="btn"
              onClick={() => {
                this.setState({ error: null });
                location.reload();
              }}
            >
              Reload panel
            </button>
            <p className="hint">The agent service keeps terminal sessions alive — reloading is safe.</p>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
