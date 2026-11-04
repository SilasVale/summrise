import { useEffect, useRef, useState } from "react";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";
import { BrowserPane } from "./BrowserPane";

/** What `screen.capture` answers: one PNG and the size the helper reported for it. */
type Capture = { data: string; width: number; height: number };

/** One capture kept with the moment it was taken, so the line and the file name agree. */
type Shot = { dataUrl: string; width: number; height: number; capturedAt: Date };

/**
 * The device's screen, as a page.
 *
 * The picture is not the console's to take: `screen.capture` asks the per-user helper, which captures
 * the logged-in user's session rather than session 0. A helper that is absent, has not registered this
 * boot, or is looking at a locked or disconnected session answers with the device's own refusal — a
 * sentence, not a black frame. There is no timer: a capture is a deliberate read, like every other
 * device call here.
 */
export function ScreenPage({
  grant,
  view,
  onViewChange,
  deviceOnline,
  helperRunning
}: {
  grant: Grant;
  view: "desktop" | "browser";
  onViewChange: (view: "desktop" | "browser") => void;
  /** The fleet's reachability, as the other device pages take it; `false` is the only fact. */
  deviceOnline?: boolean;
  /**
   * Whether the device reported a registered per-user helper on its last channel read.
   *
   * `undefined` is "it did not say" (an old build, or no facts yet) and draws nothing: this page must
   * not invent a state. It is a hint, not a gate — the helper can register between two polls, and the
   * device's own refusal stays the authority on whether a capture is possible.
   */
  helperRunning?: boolean;
}) {
  const offline = deviceOnline === false;
  const [shot, setShot] = useState<Shot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The disabled button is the visible guard, but a second click can arrive before the re-render
  // disables it; the handler closes over the previous `busy`, so the ref is the real gate.
  const capturing = useRef(false);
  const gate = useRequestGate();

  // A different grant is a different device's screen. The old capture and its refusal belong to the
  // device this page has left, and an answer still on the wire must not paint over the new one.
  useEffect(() => {
    gate.begin();
    capturing.current = false;
    setShot(null);
    setError(null);
    setBusy(false);
  }, [gate, grant]);

  const capture = async () => {
    if (capturing.current) return;
    capturing.current = true;
    const ticket = gate.begin();
    setBusy(true);
    setError(null);
    try {
      const answer = JSON.parse(await callTool(grant, "screen.capture", {})) as Capture;
      if (gate.stale(ticket)) return;
      setShot({
        dataUrl: `data:image/png;base64,${answer.data}`,
        width: answer.width,
        height: answer.height,
        capturedAt: new Date()
      });
    } catch (cause) {
      if (gate.stale(ticket)) return;
      // A failed capture clears the previous picture: an image with the refusal underneath it would
      // read as the screen the device just failed to take.
      setShot(null);
      setError(toolFailure(cause));
    } finally {
      if (!gate.stale(ticket)) {
        capturing.current = false;
        setBusy(false);
      }
    }
  };

  return (
    <div>
      <div className="screen-view-head">
        <div className="seg" role="group" aria-label="Screen view">
          <button
            type="button"
            aria-pressed={view === "desktop"}
            onClick={() => onViewChange("desktop")}
          >
            Machine screen
          </button>
          <button
            type="button"
            aria-pressed={view === "browser"}
            onClick={() => onViewChange("browser")}
          >
            Browser
          </button>
        </div>
        <p className="hint screen-view-note">
          {view === "browser"
            ? "The device's browser, drawn here as screenshots. This is a remote view, not a local window."
            : "A capture of the signed-in user's desktop; locked and disconnected sessions refuse."}
        </p>
        {view === "desktop" && (
          <button
            type="button"
            className="screen-capture"
            disabled={busy || offline}
            title={offline ? "The machine is offline; its screen can be captured when it reconnects" : undefined}
            onClick={() => void capture()}
          >
            {busy ? "Capturing…" : "Capture screen"}
          </button>
        )}
      </div>

      {view === "browser" ? (
        <BrowserPane grant={grant} deviceOnline={deviceOnline} />
      ) : (
        <>
          <p className="hint">
            The picture comes from the per-user helper's session, so a locked or disconnected session
            is the device's refusal, not a black frame.
          </p>

          {/* AN INVITATION THE CONSOLE CAN ALREADY SEE WILL FAIL IS NOT AN INVITATION. The fleet
              carries the helper's registration, so say it before the click rather than after the
              refusal; the button stays enabled because the fact is a poll old and the device's
              answer is what decides. */}
          {helperRunning === false && (
            <p className="hint">
              The fleet says this device's per-user helper has not registered, so a capture will be
              refused. The helper runs inside the signed-in user's desktop session; the Machine tab
              shows its state.
            </p>
          )}

          {error && <p className="error">{error}</p>}
          {error && /per-user helper/i.test(error) && (
            <p className="hint">
              A screen needs the per-user helper running in the signed-in user's desktop session.
              The Machine page says whether this device's helper has registered; a service or
              disconnected session cannot be captured.
            </p>
          )}

          {!shot && !error && !busy && (
            <p className="hint">
              {offline
                ? "The machine is offline; a capture is not possible until it reconnects."
                : "No capture yet. Capture screen takes one when you ask; nothing is captured on a timer."}
            </p>
          )}

          {shot && (
            <>
              <p className="hint">
                <span className="mono">
                  {shot.width}×{shot.height}
                </span>{" "}
                · captured {shot.capturedAt.toLocaleString()} ·{" "}
                <a href={shot.dataUrl} download={downloadName(shot.capturedAt)}>
                  Download
                </a>
              </p>
              <img className="screen-shot" src={shot.dataUrl} alt="the device's screen" />
            </>
          )}
        </>
      )}
    </div>
  );
}

/** `summrise-screen-YYYY-MM-DD-HHMMSS.png`, in the operator's own clock. */
function downloadName(at: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `summrise-screen-${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}-${pad(at.getHours())}${pad(at.getMinutes())}${pad(at.getSeconds())}.png`;
}

