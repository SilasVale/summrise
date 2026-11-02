import { useEffect, useRef, useState } from "react";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";

type Target = { id: string; url?: string; title?: string };

/** How slow "live" is: one screenshot per interval, because a CDP screencast is a transport of its own. */
const LIVE_MS = 1_500;
/** The interval the operator reads comes from the interval the timer uses. */
const LIVE_LABEL = `Live · ${LIVE_MS / 1_000} s`;

// The device drives a real Chrome over CDP; the page's half of that is an <img>. `browser.screenshot`
// answers a base64 PNG, which needs no plugin, no second port and no native window.
export function BrowserPane({ grant }: { grant: Grant }) {
  const [targets, setTargets] = useState<Target[]>([]);
  const [target, setTarget] = useState("");
  const [shot, setShot] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  const [status, setStatus] = useState("No browser is open on this device.");
  const [live, setLive] = useState(false);
  const [frames, setFrames] = useState(0);

  // A live tick never queues behind itself: if the device is still answering the previous screenshot,
  // the next interval is skipped rather than stacking a second request.
  const inFlight = useRef(false);
  // The view the running watch belongs to. A different target or grant is a different view, so the
  // watch stops there instead of silently moving to it.
  const liveView = useRef<{ grant: Grant; target: string } | null>(null);

  // A list and a picture are separate reads, so each has its own newest-answer rule: a late list must
  // not replace a newer selection, and a late screenshot must not show a page the operator already
  // navigated away from.
  const listGate = useRequestGate();
  const shotGate = useRequestGate();

  // `prefer` is the target the caller wants selected when it survives the re-list. Without it the
  // first target wins, which is what a close leaves behind.
  const list = async (prefer?: string): Promise<Target[] | null> => {
    const ticket = listGate.begin();
    try {
      const answer = JSON.parse(await callTool(grant, "browser.list", {})) as Target[];
      if (listGate.stale(ticket)) return null;
      setTargets(answer);
      if (answer.length === 0) {
        setTarget("");
        setShot(null);
        setStatus("No browser is open on this device.");
        return answer;
      }
      const next = prefer && answer.some((entry) => entry.id === prefer) ? prefer : answer[0].id;
      setTarget(next);
      setStatus(`${answer.length} target(s).`);
      return answer;
    } catch (cause) {
      // A stale error describes a view that is gone, exactly like stale data does.
      if (listGate.stale(ticket)) return null;
      setStatus("List failed: " + toolFailure(cause));
      return null;
    }
  };

  // One screenshot request, shared by the button and the live timer. A null answer means a newer
  // request has superseded this one, so both its picture and its error belong to a view that is gone.
  const captureOnce = async (which: string): Promise<{ data: string; bytes: number } | null> => {
    const ticket = shotGate.begin();
    try {
      const answer = JSON.parse(await callTool(grant, "browser.screenshot", { target: which })) as {
        data: string;
        bytes: number;
      };
      return shotGate.stale(ticket) ? null : answer;
    } catch (cause) {
      if (shotGate.stale(ticket)) return null;
      throw cause;
    }
  };

  const paint = (answer: { data: string; bytes: number }) => {
    setShot(`data:image/png;base64,${answer.data}`);
    setStatus(`Captured ${answer.bytes} bytes.`);
  };

  const capture = async (which = target) => {
    if (!which) return;
    try {
      const answer = await captureOnce(which);
      if (!answer) return;
      paint(answer);
    } catch (cause) {
      // The picture on screen is of a page this call could not read. Leaving it under a failure
      // sentence would present the old frame as the current one.
      setShot(null);
      setStatus("Screenshot failed: " + toolFailure(cause));
    }
  };

  const choose = (which: string) => {
    setTarget(which);
    // The picture, when there is one, is of the target that was selected a moment ago.
    if (shot) void capture(which);
  };

  // Open is a new tab; Navigate sends the address bar to the selected one. They are different
  // actions, so they are different buttons.
  const open = async () => {
    if (!url.trim()) return;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.open", { url: url.trim() })) as {
        target: string;
      };
      setTarget(answer.target);
      const listed = await list(answer.target);
      if (listed) await capture(answer.target);
    } catch (cause) {
      setStatus("Open failed: " + toolFailure(cause));
    }
  };

  const navigate = async () => {
    if (!target || !url.trim()) return;
    const wanted = target;
    try {
      await callTool(grant, "browser.navigate", { target: wanted, url: url.trim() });
      const listed = await list(wanted);
      // The picture has to be the page just asked for; if the tab vanished, the re-list's sentence is
      // the true one and a screenshot would only replace it with a second failure.
      if (listed?.some((entry) => entry.id === wanted)) await capture(wanted);
    } catch (cause) {
      setStatus("Navigate failed: " + toolFailure(cause));
    }
  };

  const close = async () => {
    if (!target) return;
    const closing = target;
    try {
      await callTool(grant, "browser.close", { target: closing });
      const listed = await list();
      if (listed && listed.length > 0) await capture(listed[0].id);
    } catch (cause) {
      setStatus("Close failed: " + toolFailure(cause));
    }
  };

  const stop = async () => {
    try {
      await callTool(grant, "browser.shutdown", {});
      // Invalidate anything still in flight: the pane now says the browser is stopped, and a late
      // list or screenshot must not bring a target or a picture back.
      listGate.begin();
      shotGate.begin();
      setTargets([]);
      setTarget("");
      setShot(null);
      setStatus("The browser is stopped. No browser is open on this device.");
    } catch (cause) {
      setStatus("Stop failed: " + toolFailure(cause));
    }
  };

  // A click on the picture is a DOM click at the page's own coordinates. It drives ordinary pages and
  // does not pretend to be a native event — the label says so rather than letting the picture imply it.
  const click = async (event: React.MouseEvent<HTMLImageElement>) => {
    if (!shot || !target) return;
    const image = event.currentTarget;
    const box = image.getBoundingClientRect();
    const x = Math.round(((event.clientX - box.left) / box.width) * image.naturalWidth);
    const y = Math.round(((event.clientY - box.top) / box.height) * image.naturalHeight);
    const expression = `(() => {
      const el = document.elementFromPoint(${x}, ${y});
      if (!el) return "nothing at ${x},${y}";
      ["mousedown", "mouseup", "click"].forEach((type) =>
        el.dispatchEvent(new MouseEvent(type, { clientX: ${x}, clientY: ${y}, bubbles: true, cancelable: true, view: window })));
      return el.tagName + " at ${x},${y}";
    })()`;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.eval", { target, expression })) as {
        value?: unknown;
      };
      setStatus("Clicked " + (answer.value ?? "the page") + ".");
      await capture();
    } catch (cause) {
      setStatus("Click failed: " + toolFailure(cause));
    }
  };

  const type = async () => {
    if (!text || !target) return;
    const expression = `(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return "no field is focused";
      el.value = (el.value || "") + ${JSON.stringify(text)};
      ["input", "change", "keyup"].forEach((t) => el.dispatchEvent(new Event(t, { bubbles: true })));
      return el.tagName + " now " + el.value;
    })()`;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.eval", { target, expression })) as {
        value?: unknown;
      };
      setStatus(String(answer.value ?? "typed"));
      setText("");
      await capture();
    } catch (cause) {
      setStatus("Type failed: " + toolFailure(cause));
    }
  };

  useEffect(() => {
    void list();
  }, []);

  useEffect(() => {
    if (!live) {
      liveView.current = null;
      return;
    }
    if (!target) {
      // The last tab closed: there is nothing left to watch, so the toggle goes off rather than
      // staying on over an empty pane.
      liveView.current = null;
      setLive(false);
      return;
    }
    const started = liveView.current;
    if (started && started.grant !== grant) {
      // A new grant is a different authority: the watch ends rather than silently continuing under
      // it, and the sentence says why. A different target is the same authority and the same watch,
      // so the timer restarts on that target instead of stopping.
      liveView.current = null;
      setLive(false);
      setStatus("Live stopped: the grant changed.");
      return;
    }
    liveView.current = { grant, target };

    // This effect's own lifetime: an answer that lands after a stop, a change, or an unmount must not
    // paint a frame, even when the request itself had already been sent.
    let current = true;
    let timer: number | undefined;

    const stopTimer = () => {
      if (timer === undefined) return;
      window.clearInterval(timer);
      timer = undefined;
    };

    const tick = async () => {
      if (!current || document.hidden || inFlight.current) return;
      inFlight.current = true;
      try {
        const answer = await captureOnce(target);
        if (!current || !answer) return;
        paint(answer);
        setFrames((count) => count + 1);
      } catch (cause) {
        if (!current) return;
        // A refusal and any other failure both end the watch; the operator restarts it with the
        // button. The sentence is the one the on-demand path already uses.
        setShot(null);
        setStatus("Screenshot failed: " + toolFailure(cause));
        setLive(false);
      } finally {
        inFlight.current = false;
      }
    };

    const startTimer = () => {
      if (timer !== undefined || document.hidden) return;
      timer = window.setInterval(() => {
        if (!document.hidden) void tick();
      }, LIVE_MS);
    };

    // A hidden tab does not keep asking the device for pictures nobody can see. Coming back visible
    // restarts the interval only while the toggle is still on, which is the only way this effect runs.
    const onVisibility = () => {
      if (document.hidden) stopTimer();
      else startTimer();
    };

    document.addEventListener("visibilitychange", onVisibility);
    startTimer();
    return () => {
      current = false;
      stopTimer();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [live, grant, target]);

  const toggleLive = () => {
    if (live) {
      setLive(false);
      return;
    }
    setFrames(0);
    setLive(true);
  };

  return (
    <div className="browser-pane">
      <div className="panel-head">
        <h2>Browser</h2>
        <div className="actions">
          <button type="button" onClick={() => void list()}>List targets</button>
          <button type="button" disabled={!target} onClick={() => void capture()}>Refresh view</button>
          <button
            type="button"
            disabled={!target}
            title={`One screenshot every ${LIVE_MS / 1_000} s`}
            onClick={toggleLive}
          >
            {live ? "Stop live view" : "Start live view"}
          </button>
          <button type="button" disabled={!target} onClick={() => void close()}>Close tab</button>
          <button type="button" onClick={() => void stop()}>Stop browser</button>
        </div>
      </div>
      <p className={status.includes("failed") ? "error" : "hint"}>{status}</p>
      {live && shot && <p className="hint">{LIVE_LABEL} · frame {frames}</p>}

      {targets.length > 0 && (
        // The browser can hold several pages; the screenshot follows the target, so the operator has
        // to be able to say which one. Without this the pane always showed whichever came first.
        <div className="row">
          <select aria-label="Target" value={target} onChange={(event) => choose(event.target.value)}>
            {targets.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.title || entry.url || entry.id}
              </option>
            ))}
          </select>
        </div>
      )}

      <div className="row">
        <input
          aria-label="Address"
          placeholder="https://example.test — opens a new tab, or navigates the selected one"
          value={url}
          onChange={(event) => setUrl(event.target.value)}
        />
        <button type="button" onClick={() => void open()}>Open new tab</button>
        <button type="button" disabled={!target || !url.trim()} onClick={() => void navigate()}>
          Navigate
        </button>
      </div>
      <div className="row">
        <input placeholder="text to type into the focused field" value={text} onChange={(event) => setText(event.target.value)} />
        <button type="button" disabled={!shot} onClick={() => void type()}>Type</button>
      </div>

      {shot && <img className="browser-shot" src={shot} alt="" onClick={(event) => void click(event)} />}
    </div>
  );
}

