import { useEffect, useRef, useState } from "react";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";

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
export function ScreenPage({ grant }: { grant: Grant }) {
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
      <div className="panel-head">
        <h2>Screen</h2>
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void capture()}>
            {busy ? "Capturing…" : "Capture screen"}
          </button>
        </div>
      </div>
      <p className="hint">
        The picture comes from the per-user helper's session, so a locked or disconnected session is
        the device's refusal, not a black frame.
      </p>

      {error && <p className="error">{error}</p>}

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
    </div>
  );
}

/** `summrise-screen-YYYY-MM-DD-HHMMSS.png`, in the operator's own clock. */
function downloadName(at: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `summrise-screen-${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}-${pad(at.getHours())}${pad(at.getMinutes())}${pad(at.getSeconds())}.png`;
}

