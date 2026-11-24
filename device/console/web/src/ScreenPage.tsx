import { useEffect, useRef, useState } from "react";
import type { HostFacts } from "./api";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";
import { BrowserPane } from "./BrowserPane";
import { PageHeading } from "./PageHeading";
import "./styles/workspaces.css";

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
 * sentence, not a black frame. The fleet also reports which session the helper is in and that
 * session's state, so the page can say a refusal the device has already determined instead of
 * inviting a click into it. There is no timer: a capture is a deliberate read, like every other
 * device call here.
 */
export function ScreenPage({
  grant,
  deviceId,
  view,
  onViewChange,
  deviceOnline,
  helperRunning,
  helperSession,
  sessions,
  openBrowser,
  onOpenBrowserHandled
}: {
  grant: Grant;
  /** The machine the pane is about, so a launch that failed is remembered for THAT machine (the hundred-and-ninety-eighth list). */
  deviceId?: string | null;
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
  /**
   * The Windows session the fleet last reported the helper in.
   *
   * The helper captures only its own session's desktop; a session whose state is not `active` or
   * `connected` has no displayed desktop, and the device's answer is Windows' refusal (`os error 5`
   * when the capture thread cannot attach to the input desktop) rather than a picture. This is the
   * fact the Machine tab already draws as `Helper session 2` and `#2 disconnected`, said here before
   * the press. `undefined`/null is "the device did not say", and a state this page does not know is
   * left to the device's answer: no state is invented.
   */
  helperSession?: number | null;
  /** The device's session rows from the same fleet read; the helper's state is looked up here. */
  sessions?: HostFacts["sessions"];
  /**
   * A one-shot request from the desktop shell's tray: the Browser address to prefill. Passed
   * through to the pane, which fills the field and opens nothing; the operator's press is the call.
   */
  openBrowser?: { url: string; nonce: number };
  /** Called once the request above is in the address field. */
  onOpenBrowserHandled?: () => void;
}) {
  const offline = deviceOnline === false;
  const helper = helperSessionFact(helperSession, sessions);
  // `helperRunning === false` is a fleet fact the page must not invite a click against. It used to
  // keep the button enabled because the fact is a poll old; the screen then offered a capture it
  // had just said would be refused. The device's refusal stays the authority when the fleet did not
  // say (`undefined`), and the poll re-enables the button when the helper registers.
  const helperMissing = helperRunning === false;
  // The session row may be stale behind an unregistered helper, so it does not also gate the button
  // when the stronger fact is already there.
  const blockedHelper =
    !helperMissing && helper !== null && desktopHidden(helper.state) ? helper : null;
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

  const nextStep = error ? refusalHint(error, helperSession) : null;

  return (
    <div className="screen-workspace">
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
            : /* **THE LEAD SENTENCE SAYS WHAT THIS IS, AND THE REASON BELOW SAYS WHY IT CANNOT**
                (the thirtieth list, item 1). Measured at 1440 with the helper unregistered, the page
                told the same refusal three times in three vocabularies — `…sessions refuse.`,
                `Capture is off: …`, and `The picture comes from the per-user helper's session, so
                …` — for 591 characters and one disabled button. The refusal is the
                `screen-capture-reason` sentence's job; this one names the surface. */
              "A capture of this machine's signed-in desktop."}
        </p>
      </div>
      {view === "browser" ? (
        <BrowserPane
          grant={grant}
          deviceOnline={deviceOnline}
          deviceId={deviceId ?? null}
          openBrowser={openBrowser}
          onOpenBrowserHandled={onOpenBrowserHandled}
        />
      ) : (
        <div className="media-workspace screen-desktop-workspace">
          <PageHeading level={2} title="Screen" kicker="Desktop capture" description="Inspect a single image of the machine's displayed desktop." />
          <section className={"screen-capture-workarea media-workspace-body" + (shot ? " has-capture" : "")} aria-label="Machine screen capture">
            <div className="screen-capture-canvas media-stage">
              <header className="media-stage-head">
                <div>
                  <p className="page-kicker">Machine screen</p>
                  <h3>Desktop view</h3>
                </div>
                <span className="chip">{busy ? "Capturing…" : shot ? "Captured image" : "No image"}</span>
              </header>
            <div className="screen-capture-result" aria-busy={busy}>
              {error && <p className="error" role="alert">{error}</p>}
              {nextStep && <p className="hint">{nextStep}</p>}

              {!shot && !error && !busy && (
                <p className="hint">
                  {offline
                    ? "No capture yet."
                    : "No capture yet — Capture screen takes one when you ask."}
                </p>
              )}
              {busy && <p className="hint" role="status">Waiting for the machine's capture…</p>}
              {shot && <img className="screen-shot" src={shot.dataUrl} alt="the device's screen" />}
            </div>
            </div>
            <aside className="media-controls screen-controls" aria-label="Screen controls">
              <section className="screen-control-section screen-view-head">
                <h3>Capture</h3>
          <div className="screen-capture-group">
            <button
              type="button"
              className="screen-capture"
              disabled={busy || offline || helperMissing || blockedHelper !== null}
              title={
                offline
                  ? "The machine is offline; its screen can be captured when it reconnects"
                  : helperMissing
                    ? "The fleet says this device's per-user helper has not registered; a capture needs it running in the signed-in user's desktop session"
                    : blockedHelper
                      ? helperTitle(blockedHelper)
                      : undefined
              }
              aria-describedby={
                helperMissing
                  ? "screen-helper-missing"
                  : blockedHelper
                    ? "screen-helper-session"
                    : undefined
              }
              onClick={() => void capture()}
            >
              {busy ? "Capturing…" : "Capture screen"}
            </button>

            {/* AN INVITATION THE CONSOLE CAN ALREADY SEE WILL FAIL IS NOT AN INVITATION. The fleet
                carries the helper's registration, so say it before the click and take the button
                off: an enabled press whose own page says it will be refused is the measured defect.
                The fact is a poll old, so the next fleet read puts the button back. */}
            {helperMissing && (
              <p className="hint screen-capture-reason" id="screen-helper-missing">
                Capture is off: the per-user helper has not registered, so a capture will be
                refused. It starts in the signed-in user's desktop session; the Machine tab shows
                its state.
              </p>
            )}

            {/* THE SESSION ROW IS THE OTHER HALF OF THE SAME INVITATION. A helper registered in a
                disconnected or idle session cannot reach an input desktop, and the device answers
                with Windows' `os error 5`; the Machine tab's `Helper session 2` and `#2 disconnected`
                are already the explanation. Say it with the way out, and disable the press: this is
                not a poll-old hint like registration, it is the session the helper is in. The fleet
                polls every five seconds, so connecting the session re-enables the button. */}
            {blockedHelper && (
              <p className="hint screen-capture-reason" id="screen-helper-session">
                Capture is off: the per-user helper is in session {blockedHelper.id}, which is{" "}
                {blockedHelper.state}. Sign in at the machine or reconnect its remote session, then
                capture again.
              </p>
            )}
          </div>
              </section>
          {shot && (
            <section className="screen-control-section screen-capture-context" aria-label="Capture details">
              <h3>Capture details</h3>
              <dl className="facts">
                <div>
                  <dt>Dimensions</dt>
                  <dd className="mono">{shot.width}×{shot.height}</dd>
                </div>
                <div>
                  <dt>Captured</dt>
                  <dd><time dateTime={shot.capturedAt.toISOString()}>{shot.capturedAt.toLocaleString()}</time></dd>
                </div>
              </dl>
              <a className="btn" href={shot.dataUrl} download={downloadName(shot.capturedAt)}>
                Download
              </a>
              <p className="hint screen-shot-caption">
                The picture is the per-user helper's session, so a locked or disconnected session is
                the machine's refusal, not a black frame.
              </p>
            </section>
          )}
            </aside>
          </section>
        </div>
      )}
    </div>
  );
}

/** `summrise-screen-YYYY-MM-DD-HHMMSS.png`, in the operator's own clock. */
function downloadName(at: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `summrise-screen-${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}-${pad(at.getHours())}${pad(at.getMinutes())}${pad(at.getSeconds())}.png`;
}

/** The device's own row for the helper's session, when the fleet reported both facts. */
function helperSessionFact(
  helperSession: number | null | undefined,
  sessions: HostFacts["sessions"]
): { id: number; state: string } | null {
  if (helperSession == null) return null;
  const row = sessions?.find((session) => session.id === helperSession);
  return row ? { id: helperSession, state: row.state } : null;
}

/**
 * Whether a reported session state is one this page knows has no displayed desktop.
 *
 * The device reports Windows' WTS names (`active`, `connected`, `disconnected`, `idle`, …), and
 * `state-N` for a value it does not recognise. Only a state the page knows may hide the button: an
 * unknown `state-12` is not read as disconnected, and an absent value is not a state at all.
 */
function desktopHidden(state: string): boolean {
  if (!state || state === "active" || state === "connected") return false;
  return !/^state-\d+$/.test(state);
}

/** The disabled button's hover: the same fact, with the way out. */
function helperTitle(helper: { id: number; state: string }): string {
  return `The helper is in session ${helper.id}, which is ${helper.state}; a capture needs that session connected and unlocked. Sign in at the machine or reconnect its remote session, then capture again.`;
}

/**
 * The device's refusal, followed by what the operator can do about it.
 *
 * The device's sentences are specific about the cause — `no per-user helper is running`, `the
 * session is locked`, or Windows' own `the capture thread could not attach to the input desktop:
 * … (os error 5)` — but none of them names the way out. This adds only steps that exist: start the
 * helper by signing in to a desktop session, or connect and unlock the session the helper is in. A
 * refusal the page cannot explain draws no sentence of its own; the device's own text stays the
 * error.
 */
function refusalHint(error: string, helperSession?: number | null): string | null {
  if (/per-user helper/i.test(error)) {
    return "A screen needs the per-user helper running in the signed-in user's desktop session. Sign in at the machine (or reconnect its remote session) so the helper starts there, then capture again.";
  }
  if (
    /input desktop|could not attach|session is locked|not being displayed|os error 5|access is denied|拒绝访问/i.test(
      error
    )
  ) {
    const where = helperSession != null ? ` in session ${helperSession}` : "";
    return `The device refused the picture: the helper${where} could not see a displayed desktop. Unlock the machine or connect that session (sign in at the machine or reconnect its remote session), then capture again.`;
  }
  return null;
}

