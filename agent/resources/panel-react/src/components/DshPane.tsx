// DshPane — the harness's own UI, embedded in the Summrise desktop shell.
//
// The shell owns a SECOND WebContentsView (window.summriseDsh) beside the browser one, and this
// pane is its SPA-side controller, exactly as EmbeddedBrowserPane is for the browser:
//   - render an empty slot the main process overlays the native view onto
//   - report the slot's bounds on mount / resize / page switch, so the view tracks the layout
//   - ask the MAIN process to open the harness — this component never learns an address, because
//     the harness runs on this machine's loopback and its port belongs to the main process (see
//     lib/embeddedBridge: `open()` takes no URL, and url-policy.isDshUrl is the view's one door)
//   - a crashed renderer arrives as `onGone` and gets a recovery banner instead of "starting…"
//     forever, which is the failure the browser pane's round-256 fixed and this one would
//     otherwise repeat
//
// PLAIN BROWSERS NEVER MOUNT THIS: with no bridge there is nothing to position, and DshPage says
// so rather than drawing a dead slot.
import { useCallback, useEffect, useRef, useState } from "react";
import { dshBridge } from "../lib/embeddedBridge";
import { Icon } from "../ui/Icon";

const slotId = "summrise-dsh-slot";

export function DshPane() {
  const slotRef = useRef<HTMLDivElement | null>(null);
  const [gone, setGone] = useState<string | null>(null);
  const [opened, setOpened] = useState(false);

  // Bounds are relative to the window content — the slot's getBoundingClientRect IS that space
  // (the SPA fills the window), which is the contract the browser pane already established.
  const reportBounds = useCallback(() => {
    const el = slotRef.current;
    const b = dshBridge();
    if (!el || !b) return;
    const r = el.getBoundingClientRect();
    if (r.width < 50 || r.height < 50) {
      void b.place(null);
      return;
    }
    void b.place({
      x: Math.round(r.left),
      y: Math.round(r.top),
      width: Math.round(r.width),
      height: Math.round(r.height),
    });
  }, []);

  useEffect(() => {
    const b = dshBridge();
    if (!b) return;
    void b.open().then(() => setOpened(true));
    void b.state().then((s) => {
      if (s?.ok && s.url) setOpened(true);
    });
    const offGone = b.onGone((d) => {
      setGone(d.reason || `exit ${d.exitCode}`);
      setOpened(false);
    });
    reportBounds();
    const el = slotRef.current;
    let ro: ResizeObserver | null = null;
    if (el && typeof ResizeObserver !== "undefined") {
      ro = new ResizeObserver(() => reportBounds());
      ro.observe(el);
    }
    window.addEventListener("resize", reportBounds);
    return () => {
      if (ro) ro.disconnect();
      window.removeEventListener("resize", reportBounds);
      offGone();
      // Leaving the page: hide the native view so it does not linger over other SPA pages.
      void b.place(null);
    };
  }, [reportBounds]);

  const recover = useCallback(() => {
    const b = dshBridge();
    if (!b) return;
    setGone(null);
    void b.recover().then(() => {
      setOpened(true);
      reportBounds();
    });
  }, [reportBounds]);

  return (
    <div className="dsh-pane">
      <div className="dsh-toolbar">
        <span className="dsh-toolbar-title">
          <Icon name="harness" size={14} /> Harness
        </span>
        <button
          className="btn btn-mini"
          onClick={() => { const b = dshBridge(); if (b) void b.reload(); }}
          title="Reload the harness view"
        >
          Reload
        </button>
      </div>
      {gone ? (
        <div className="dsh-gone" role="status">
          <p><strong>The harness view stopped</strong> ({gone}).</p>
          <button className="btn btn-mini" onClick={recover}>Recover</button>
        </div>
      ) : null}
      {/* The slot is EMPTY ON PURPOSE: the native view is composited over this rectangle by the
          main process. `data-opened` exists so a sweep can tell "asked for it" from "never did". */}
      <div className="dsh-slot" id={slotId} ref={slotRef} data-opened={opened ? "yes" : "no"} />
    </div>
  );
}
