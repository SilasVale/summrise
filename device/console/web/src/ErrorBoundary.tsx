import { Component, type ErrorInfo, type ReactNode } from "react";

type Props = { children: ReactNode };
type State = { error: Error | null };

/**
 * The page's last line of defence.
 *
 * React 18 and later unmount the whole root when a render throws, so one bad page used to leave the
 * operator with a blank tab and no sentence — the console is the only view of the device, and a white
 * page reads as "the device is gone". A boundary turns that into a panel that says what happened and
 * offers the one action that can recover it. There is no hook for this in React: catching a render
 * error is a class-only lifecycle, which is why this file is a class.
 *
 * The state is never cleared on a route change: the page that threw is remounted under a new tree only
 * after the reload, and resetting here would re-throw it in place.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // The panel is for the operator; this line is for the person reading the browser console. Both
    // are needed: the panel hides the stack, and the stack is what a bug report needs.
    console.error("the console page stopped while drawing", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="login-wrap">
          <section className="panel login" role="alert">
            <h2>Something broke</h2>
            <p className="hint">
              This page stopped while drawing. The device is still running; reload the console to
              come back to it.
            </p>
            <p className="mono">{this.state.error.message}</p>
            <button className="primary" type="button" onClick={() => window.location.reload()}>
              Reload
            </button>
          </section>
        </div>
      );
    }
    return this.props.children;
  }
}
