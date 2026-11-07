import { useEffect, useRef, useState } from "react";
import type { HostFacts } from "./api";
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
 * sentence, not a black frame. The fleet also reports which session the helper is in and that
 * session's state, so the page can say a refusal the device has already determined instead of
 * inviting a click into it. There is no timer: a capture is a deliberate read, like every other
 * device call here.
 */
export function ScreenPage({
  grant,
  view,
  onViewChange,
  deviceOnline,
  helperRunning,
  helperSession,
  sessions
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
}) {
  const offline = deviceOnline === false;
  const helper = helperSessionFact(helperSession, sessions);
  // `helperRunning === false` has its own sentence, and that is the fact the fleet reported; the
  // session row may be stale behind it, so it does not also gate the button.
  const blockedHelper =
    helperRunning !== false && helper !== null && desktopHidden(helper.state) ? helper : null;
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
            disabled={busy || offline || blockedHelper !== null}
            title={
              offline
                ? "The machine is offline; its screen can be captured when it reconnects"
                : blockedHelper
                  ? helperTitle(blockedHelper)
                  : undefined
            }
            aria-describedby={blockedHelper ? "screen-helper-session" : undefined}
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

          {/* THE SESSION ROW IS THE OTHER HALF OF THE SAME INVITATION. A helper registered in a
              disconnected or idle session cannot reach an input desktop, and the device answers
              with Windows' `os error 5`; the Machine tab's `Helper session 2` and `#2 disconnected`
              are already the explanation. Say it with the way out, and disable the press: this is
              not a poll-old hint like registration, it is the session the helper is in. The fleet
              polls every five seconds, so connecting the session re-enables the button. */}
          {blockedHelper && (
            <p className="hint" id="screen-helper-session">
              The fleet's last report puts this device's per-user helper in session{" "}
              {blockedHelper.id}, which is {blockedHelper.state}. A capture will be refused until
              that session is connected and unlocked — sign in at the machine or reconnect its
              remote session, then capture again.
            </p>
          )}

          {error && <p className="error">{error}</p>}
          {nextStep && <p className="hint">{nextStep}</p>}

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

