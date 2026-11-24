import { useEffect, useRef, useState } from "react";
import { useRequestGate } from "./requests";
import { callTool, toolFailure, type Grant } from "./terminal";
import { PageHeading } from "./PageHeading";
import "./styles/workspaces.css";

type Target = { id: string; url?: string; title?: string; type?: string };
type BrowserView = { grant: Grant; deviceId: string | null; target: string; offline: boolean };
type BrowserCapture = { data: string; bytes: number; view: BrowserView };

/// Chrome's own targets are not pages the operator can use. A real device listed the new-tab page,
/// `chrome-untrusted://new-tab-page/...`, `chrome://webui-toolbar.top-chrome` and the omnibox popup
/// beside the actual page; selecting one gives a picture that click/eval cannot drive.
const INTERNAL_SCHEMES = ["chrome://", "chrome-untrusted://", "devtools://", "edge://", "about:"];
function isPageTarget(entry: Target): boolean {
  if (entry.type !== undefined && entry.type !== "page") return false;
  const url = (entry.url ?? "").toLowerCase();
  return !INTERNAL_SCHEMES.some((scheme) => url.startsWith(scheme));
}

/** How slow "live" is: one screenshot per interval, because a CDP screencast is a transport of its own. */
const LIVE_MS = 1_500;
/** The interval the operator reads comes from the interval the timer uses. */
const LIVE_LABEL = `Live · ${LIVE_MS / 1_000} s`;
/** No browser, or no usable page: the address form below is what starts one. */
const NO_BROWSER =
  "No browser is open on this device. Type an address below and choose Open new tab to start one.";
const NO_PAGE =
  "No browser page is open on this device. Type an address below and choose Open new tab to start one.";
/** A down machine cannot list or open a browser; the empty state must not invite the click. */
const OFFLINE = "The machine is offline; its browser cannot be listed or opened until it reconnects.";

/**
 * The two sentences a browser launch failure earns, and the one recovery the pane already offers.
 *
 * The device's launch probe words its failures differently: Chrome exited before the page endpoint
 * answered, or the endpoint did not answer in time. The console has no error code for either, so the
 * device's own sentence is matched. Any other refusal — no Chrome installed, a protocol or HTTP
 * error, the grant — keeps its own answer: "did not come up" would state a cause the answer does not
 * prove, and its fix is not this pane's Stop/Refresh pair.
 */
const BROWSER_DOWN = "The device's browser did not come up.";
const PAGE_ENDPOINT_SILENT = "The device's browser page endpoint did not answer.";
const START_AGAIN = "Stop the browser, then choose Refresh pages to start it again.";

function browserStartSentence(raw: string): string | null {
  if (
    raw.includes("the browser did not start: chrome exited with") &&
    raw.includes("before it answered")
  ) {
    return BROWSER_DOWN;
  }
  if (raw.includes("the browser did not start: chrome did not answer")) {
    return PAGE_ENDPOINT_SILENT;
  }
  return null;
}

// The device drives a real Chrome over CDP; the page's half of that is an <img>. `browser.screenshot`
// answers a base64 PNG, which needs no plugin, no second port and no native window.
/**
 * **A LAUNCH THAT FAILED IS NOT RETRIED ON EVERY VISIT (the hundred-and-ninety-eighth list).** `browser.list` STARTS the browser on the
 * device, so on a machine where chrome cannot come up, every look at this view fired a doomed call — measured: three
 * `browser.list` failures in one audit window, all the console's own. The pane remembers the machine whose launch failed, shows that
 * machine's own answer, and waits for the operator's `Refresh pages` instead of asking again by itself.
 */
const launchFailed = new Set<string>();
/** The machine's own words for its failed launch, kept beside the memory so the sentence has its evidence. */
const launchFailure = new Map<string, string>();

export function BrowserPane({
  grant,
  deviceId = null,
  deviceOnline,
  openBrowser,
  onOpenBrowserHandled
}: {
  grant: Grant;
  /** The machine this view draws, so a failed launch is remembered per machine. */
  deviceId?: string | null;
  deviceOnline?: boolean;
  /**
   * A one-shot request from the desktop shell's tray: an address to put in the field, with a nonce
   * so the same URL arriving again is a new request. The pane performs no device call for it.
   */
  openBrowser?: { url: string; nonce: number };
  /** Called once the address is in the field. */
  onOpenBrowserHandled?: () => void;
}) {
  const offline = deviceOnline === false;
  const [targets, setTargets] = useState<Target[]>([]);
  const [target, setTarget] = useState("");
  const [image, setImage] = useState<{ dataUrl: string; view: BrowserView } | null>(null);
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  // The first paint must not invite a control the same props have already disabled; the effect
  // below keeps the sentence right when the fleet's word changes later.
  const [status, setStatus] = useState(offline ? OFFLINE : NO_BROWSER);
  // The raw `browser.list` answer when the sentence above it is the mapped launch failure. A new
  // sentence replaces both, so the evidence can never outlive the failure it belongs to.
  const [rawAnswer, setRawAnswer] = useState<string | null>(null);
  const say = (sentence: string, raw: string | null = null) => {
    setStatus(sentence);
    setRawAnswer(raw);
  };
  const [live, setLive] = useState(false);
  const [frames, setFrames] = useState(0);
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const [zoom, setZoom] = useState<"fit" | "actual">("fit");

  const openBrowserHandledRef = useRef(onOpenBrowserHandled);

  useEffect(() => {
    openBrowserHandledRef.current = onOpenBrowserHandled;
  }, [onOpenBrowserHandled]);

  // **THE TRAY'S BROWSER LINK IS AN ADDRESS, NOT A PRESS.** The shell names a URL; the operator
  // still chooses Open new tab (or Navigate). Nothing here calls `browser.open`: a page that acted
  // on the fragment would launch a remote browser without the person in front of it asking.
  useEffect(() => {
    if (!openBrowser) return;
    setUrl(openBrowser.url);
    openBrowserHandledRef.current?.();
  }, [openBrowser]);

  // A live tick never queues behind itself: if the device is still answering the previous screenshot,
  // the next interval is skipped rather than stacking a second request.
  const inFlight = useRef(false);
  // The view the running watch belongs to. A different target or grant is a different view, so the
  // watch stops there instead of silently moving to it.
  const liveView = useRef<{ grant: Grant; target: string } | null>(null);

  // A context object is also its epoch: leaving and returning to the same grant/device still makes
  // a new lifetime. A late action may not acquire a fresh request ticket after its lifetime ended.
  const contextRef = useRef({ grant, deviceId, offline });
  if (contextRef.current.grant !== grant || contextRef.current.deviceId !== deviceId ||
    contextRef.current.offline !== offline) {
    contextRef.current = { grant, deviceId, offline };
  }
  const context = contextRef.current;
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const isCurrentContext = () => mounted.current && contextRef.current === context;

  // A list and a picture are separate reads, so each has its own newest-answer rule: a late list must
  // not replace a newer selection, and a late screenshot must not show a page the operator already
  // navigated away from.
  const listGate = useRequestGate();
  const shotGate = useRequestGate();

  // Keep a view's identity across ordinary renders, but replace it on every context or target change.
  // Returning to the same target cannot revive an older image or input action.
  const viewRef = useRef<BrowserView>({ grant, deviceId, target, offline });
  const previousView = viewRef.current;
  if (previousView.grant !== grant || previousView.deviceId !== deviceId ||
    previousView.target !== target || previousView.offline !== offline) {
    viewRef.current = { grant, deviceId, target, offline };
  }
  const isCurrentView = (view: BrowserView) =>
    isCurrentContext() && !view.offline && viewRef.current === view;
  const shot = image && isCurrentView(image.view) ? image.dataUrl : null;

  const selectTarget = (which: string) => {
    if (!isCurrentContext()) return;
    const current = viewRef.current;
    if (current.target !== which) {
      // Selection must invalidate even the very first capture, when there is no image yet.
      shotGate.begin();
      setImage(null);
      setSize(null);
      viewRef.current = { ...current, target: which };
    }
    setTarget(which);
  };

  useEffect(() => {
    listGate.begin();
    shotGate.begin();
    setImage(null);
    setSize(null);
  }, [grant, deviceId, offline, listGate, shotGate]);

  // `prefer` is the target the caller wants selected when it survives the re-list. Without it the
  // first target wins, which is what a close leaves behind.
  const list = async (prefer?: string): Promise<Target[] | null> => {
    if (offline || !isCurrentContext()) return null;
    const ticket = listGate.begin();
    try {
      const answer = JSON.parse(await callTool(grant, "browser.list", {})) as Target[];
      if (listGate.stale(ticket) || !isCurrentContext()) return null;
      const pages = answer.filter(isPageTarget);
      setTargets(pages);
      if (pages.length === 0) {
        selectTarget("");
        setImage(null);
        say(answer.length === 0 ? NO_BROWSER : NO_PAGE);
        return pages;
      }
      const next = prefer && pages.some((entry) => entry.id === prefer) ? prefer : pages[0].id;
      selectTarget(next);
      // A launch that worked retires the memory: the next visit asks again.
      if (deviceId !== null) {
        launchFailed.delete(deviceId);
        launchFailure.delete(deviceId);
      }
      say(`${pages.length} ${pages.length === 1 ? "page" : "pages"}.`);
      return pages;
    } catch (cause) {
      // A stale error describes a view that is gone, exactly like stale data does.
      if (listGate.stale(ticket) || !isCurrentContext()) return null;
      const raw = toolFailure(cause);
      const mapped = browserStartSentence(raw);
      if (mapped !== null && deviceId !== null) {
        launchFailed.add(deviceId);
        launchFailure.set(deviceId, raw);
      }
      // The device's loopback port and exit status are evidence, not the line: the mapped sentence
      // says only what the answer proves and names the two controls that can try again.
      say(mapped ?? "List failed: " + raw, mapped ? raw : null);
      return null;
    }
  };

  // One screenshot request, shared by the button and the live timer. A null answer means a newer
  // request has superseded this one, so both its picture and its error belong to a view that is gone.
  const captureOnce = async (which: string): Promise<BrowserCapture | null> => {
    const view = viewRef.current;
    if (view.target !== which || !isCurrentView(view)) return null;
    const ticket = shotGate.begin();
    try {
      const answer = JSON.parse(await callTool(grant, "browser.screenshot", { target: which })) as {
        data: string;
        bytes: number;
      };
      return shotGate.stale(ticket) || !isCurrentView(view) ? null : { ...answer, view };
    } catch (cause) {
      if (shotGate.stale(ticket) || !isCurrentView(view)) return null;
      throw cause;
    }
  };

  const paint = (answer: BrowserCapture): boolean => {
    if (!isCurrentView(answer.view)) return false;
    setImage({ dataUrl: `data:image/png;base64,${answer.data}`, view: answer.view });
    say(`Captured ${answer.bytes} bytes.`);
    return true;
  };

  const capture = async (which = target) => {
    const view = viewRef.current;
    if (!which || view.target !== which || !isCurrentView(view)) return;
    try {
      const answer = await captureOnce(which);
      if (!answer) return;
      paint(answer);
    } catch (cause) {
      if (!isCurrentView(view)) return;
      // The picture on screen is of a page this call could not read. Leaving it under a failure
      // sentence would present the old frame as the current one.
      setImage(null);
      say("Screenshot failed: " + toolFailure(cause));
    }
  };

  const choose = (which: string) => {
    selectTarget(which);
    // The picture, when there is one, is of the target that was selected a moment ago.
    if (shot) void capture(which);
  };

  // Open is a new tab; Navigate sends the address bar to the selected one. They are different
  // actions, so they are different buttons.
  const open = async () => {
    if (!url.trim() || offline || !isCurrentContext()) return;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.open", { url: url.trim() })) as {
        target: string;
      };
      if (!isCurrentContext()) return;
      selectTarget(answer.target);
      const listed = await list(answer.target);
      if (isCurrentContext() && listed) await capture(answer.target);
    } catch (cause) {
      if (!isCurrentContext()) return;
      say("Open failed: " + toolFailure(cause));
    }
  };

  const navigate = async () => {
    if (!target || !url.trim() || offline || !isCurrentContext()) return;
    const wanted = target;
    try {
      await callTool(grant, "browser.navigate", { target: wanted, url: url.trim() });
      if (!isCurrentContext()) return;
      const listed = await list(wanted);
      // The picture has to be the page just asked for; if the tab vanished, the re-list's sentence is
      // the true one and a screenshot would only replace it with a second failure.
      if (isCurrentContext() && listed?.some((entry) => entry.id === wanted)) await capture(wanted);
    } catch (cause) {
      if (!isCurrentContext()) return;
      say("Navigate failed: " + toolFailure(cause));
    }
  };

  const close = async () => {
    if (!target || offline || !isCurrentContext()) return;
    const closing = target;
    try {
      await callTool(grant, "browser.close", { target: closing });
      if (!isCurrentContext()) return;
      const listed = await list();
      if (isCurrentContext() && listed && listed.length > 0) await capture(listed[0].id);
    } catch (cause) {
      if (!isCurrentContext()) return;
      say("Close failed: " + toolFailure(cause));
    }
  };

  const stop = async () => {
    if (offline || !isCurrentContext()) return;
    try {
      await callTool(grant, "browser.shutdown", {});
      if (!isCurrentContext()) return;
      // Invalidate anything still in flight: the pane now says the browser is stopped, and a late
      // list or screenshot must not bring a target or a picture back.
      listGate.begin();
      shotGate.begin();
      setTargets([]);
      selectTarget("");
      setImage(null);
      say("The browser is stopped. " + NO_BROWSER);
    } catch (cause) {
      if (!isCurrentContext()) return;
      say("Stop failed: " + toolFailure(cause));
    }
  };

  // A click on the picture is a DOM click at the page's own coordinates. It drives ordinary pages and
  // does not pretend to be a native event — the label says so rather than letting the picture imply it.
  const click = async (event: React.MouseEvent<HTMLButtonElement>) => {
    if (!shot || !target || !image || !isCurrentView(image.view)) return;
    const view = image.view;
    const picture = event.currentTarget.querySelector("img");
    if (!picture) return;
    const box = picture.getBoundingClientRect();
    // A keyboard activation has no pointer coordinates; it lands at the centre, which is the one
    // point a keyboard user can name without seeing a cursor.
    const keyboard = event.detail === 0;
    const clientX = keyboard ? box.left + box.width / 2 : event.clientX;
    const clientY = keyboard ? box.top + box.height / 2 : event.clientY;
    const x = Math.round(((clientX - box.left) / box.width) * picture.naturalWidth);
    const y = Math.round(((clientY - box.top) / box.height) * picture.naturalHeight);
    const expression = `(() => {
      const el = document.elementFromPoint(${x}, ${y});
      if (!el) return "nothing at ${x},${y}";
      ["mousedown", "mouseup", "click"].forEach((type) =>
        el.dispatchEvent(new MouseEvent(type, { clientX: ${x}, clientY: ${y}, bubbles: true, cancelable: true, view: window })));
      // Synthetic events do not run the browser's default focus action; without this a click on an
      // input left the Type box saying no field was focused. Only focus what a click could focus.
      if (el instanceof HTMLElement) {
        if (el.matches("input, textarea, select, button") || el.isContentEditable) el.focus();
      }
      return el.tagName + " at ${x},${y}";
    })()`;
    try {
      const answer = JSON.parse(await callTool(view.grant, "browser.eval", { target: view.target, expression })) as {
        value?: unknown;
      };
      if (!isCurrentView(view)) return;
      say("Clicked " + (answer.value ?? "the page") + ".");
      await capture();
    } catch (cause) {
      if (!isCurrentView(view)) return;
      say("Click failed: " + toolFailure(cause));
    }
  };

  const type = async () => {
    if (!text || !target || !shot || !image || !isCurrentView(image.view)) return;
    const view = image.view;
    const expression = `(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return "no field is focused";
      el.value = (el.value || "") + ${JSON.stringify(text)};
      ["input", "change", "keyup"].forEach((t) => el.dispatchEvent(new Event(t, { bubbles: true })));
      return el.tagName + " now " + el.value;
    })()`;
    try {
      const answer = JSON.parse(await callTool(view.grant, "browser.eval", { target: view.target, expression })) as {
        value?: unknown;
      };
      if (!isCurrentView(view)) return;
      say(String(answer.value ?? "typed"));
      setText("");
      await capture();
    } catch (cause) {
      if (!isCurrentView(view)) return;
      say("Type failed: " + toolFailure(cause));
    }
  };

  useEffect(() => {
    // A MACHINE THE FLEET SAYS IS DOWN IS NOT ASKED. `browser.list` cannot reach it, and the pane
    // would sit on its opening sentence until the console's dial timed out.
    if (offline) {
      setTargets([]);
      selectTarget("");
      setImage(null);
      say(OFFLINE);
      return;
    }
    // **A MACHINE WHOSE LAUNCH ALREADY FAILED IS NOT ASKED AGAIN BY ITSELF.** The sentence it earned stays on screen, and the operator's
    // `Refresh pages` is the retry — one call per attempt instead of one per visit.
    if (deviceId !== null && launchFailed.has(deviceId)) {
      setTargets([]);
      selectTarget("");
      setImage(null);
      say(BROWSER_DOWN, launchFailure.get(deviceId) ?? null);
      return;
    }
    void list();
  }, [offline, deviceId]);

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
      say("Live stopped: the grant changed.");
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
        if (!current || !answer || !paint(answer)) return;
        setFrames((count) => count + 1);
      } catch (cause) {
        if (!current || !isCurrentContext()) return;
        // A refusal and any other failure both end the watch; the operator restarts it with the
        // button. The sentence is the one the on-demand path already uses.
        setImage(null);
        say("Screenshot failed: " + toolFailure(cause));
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
  }, [live, grant, deviceId, offline, target]);

  const toggleLive = () => {
    if (live) {
      setLive(false);
      return;
    }
    setFrames(0);
    setLive(true);
  };

  return (
    <div className="browser-pane media-workspace">
      <PageHeading
        level={2}
        title="Browser"
        kicker="Remote workspace"
        description="A browser running on this machine, with page controls beside the remote view."
      />
      <div className="browser-workarea media-workspace-body">
        <section className="browser-canvas media-stage" aria-label="Remote page view">
          <header className="media-stage-head">
            <div>
              <p className="page-kicker">Selected page</p>
              <h3>{targets.find((entry) => entry.id === target)?.title || "Page view"}</h3>
            </div>
            <span className="chip">{live ? "Live view" : "On demand"}</span>
          </header>
          {live && shot && <p className="hint">{LIVE_LABEL} · frame {frames}</p>}
          {shot ? (
            <>
              <div className="browser-meta">
                <span className="hint">
                  Remote view
                  {size ? (
                    <>
                      {" · "}
                      <span className="mono">
                        {size.width}×{size.height}
                      </span>
                    </>
                  ) : null}
                  {" · PNG"}
                </span>
                <button
                  type="button"
                  className="mini"
                  onClick={() => setZoom(zoom === "fit" ? "actual" : "fit")}
                >
                  {zoom === "fit" ? "Actual size" : "Fit width"}
                </button>
              </div>
              <div className={`browser-view table-scroll ${zoom}`} role="region" aria-label="Remote browser screenshot" tabIndex={0}>
                {/* The pointer still maps to the image's own coordinates; Enter clicks its centre. */}
                <button
                  type="button"
                  className="browser-shot-button"
                  aria-label="Click the remote page; a keyboard click lands at its centre"
                  onClick={(event) => void click(event)}
                >
                  <img
                    className="browser-shot"
                    src={shot}
                    alt="the device's browser page"
                    onLoad={(event) =>
                      setSize({
                        width: event.currentTarget.naturalWidth,
                        height: event.currentTarget.naturalHeight
                      })
                    }
                  />
                </button>
              </div>
            </>
          ) : (
            <div className="browser-view-empty">
              <p className="hint">
                {target
                  ? "Refresh view takes a screenshot of the selected page."
                  : "No page view yet."}
              </p>
            </div>
          )}
        </section>
        <aside className="media-controls browser-controls" aria-label="Browser controls">
          <section className="browser-control-section browser-navigation" aria-label="Browser navigation">
            <h3>Pages</h3>
        {targets.length > 0 && (
          <div className="browser-page-picker">
            <div className="field">
              <label className="field-label" htmlFor="browser-page">Page</label>
              <select id="browser-page" aria-label="Page" value={target} onChange={(event) => choose(event.target.value)}>
                {targets.map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.title || entry.url || entry.id}
                  </option>
                ))}
              </select>
            </div>
            {targets.find((entry) => entry.id === target)?.url && (
              <p className="hint browser-url">{targets.find((entry) => entry.id === target)?.url}</p>
            )}
          </div>
        )}
        <div className="actions" role="group" aria-label="Browser pages">
          <button type="button" disabled={offline} onClick={() => void list()}>Refresh pages</button>
          <button type="button" disabled={!target || offline} onClick={() => void close()}>Close tab</button>
          <button type="button" disabled={offline} onClick={() => void stop()}>Stop browser</button>
        </div>
          </section>
          <section className="browser-control-section" aria-label="Address controls">
            <h3>Navigate</h3>
        <div className="field">
          <label className="field-label" htmlFor="browser-address">Address</label>
          <div className="row browser-address">
            <input
              id="browser-address"
              aria-label="Address"
              placeholder="https://example.test — opens a new tab, or navigates the selected one"
              value={url}
              onChange={(event) => setUrl(event.target.value)}
            />
            <div className="browser-address-actions" role="group" aria-label="Address actions">
              <button type="button" disabled={!url.trim() || offline} onClick={() => void open()}>
                Open new tab
              </button>
              <button type="button" disabled={!target || !url.trim() || offline} onClick={() => void navigate()}>
                Navigate
              </button>
            </div>
          </div>
        </div>
          </section>
          <section className="browser-control-section" aria-label="Capture controls">
            <h3>View controls</h3>
            <div className="actions" role="group" aria-label="View controls">
              <button type="button" disabled={!target || offline} onClick={() => void capture()}>Refresh view</button>
              <button
                type="button"
                disabled={!target || offline}
                title={`One screenshot every ${LIVE_MS / 1_000} s`}
                aria-pressed={live}
                onClick={toggleLive}
              >
                {live ? "Stop live view" : "Start live view"}
              </button>
            </div>
          </section>
          <section className="browser-control-section browser-input" aria-label="Remote page input">
          <header><h3>Page input</h3></header>
          <p className="hint">Click a field in the page view, then add text to it.</p>
          <div className="field">
            <label className="field-label" htmlFor="browser-input-text">Text for the focused field</label>
            <div className="row browser-type">
              <input
                id="browser-input-text"
                aria-label="Text to type into the focused field"
                placeholder="text to type into the focused field"
                value={text}
                onChange={(event) => setText(event.target.value)}
              />
              <button type="button" disabled={!shot || offline} onClick={() => void type()}>Type</button>
            </div>
          </div>
          <p className="hint">Input is DOM-level, not a local browser window.</p>
          </section>
          <section className="browser-control-section browser-state" aria-label="Browser status">
            <h3>Browser status</h3>
      <div className="browser-status">
        <p className={rawAnswer !== null || status.includes("failed") ? "error" : "hint"}>{status}</p>
        {rawAnswer !== null && (
          <>
            <p className="hint">{START_AGAIN}</p>
            <details className="browser-answer">
              <summary>The device's answer</summary>
              <p className="mono">{rawAnswer}</p>
            </details>
          </>
        )}
      </div>
          </section>
        </aside>
      </div>
    </div>
  );
}
