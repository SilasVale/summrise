import { useEffect, useState } from "react";
import { callTool, type Grant } from "./terminal";

type Target = { id: string; url?: string; title?: string };

// The device drives a real Chrome over CDP; the page's half of that is an <img>. `browser.screenshot`
// answers a base64 PNG, which needs no plugin, no second port and no native window.
export function BrowserPane({ grant }: { grant: Grant }) {
  const [targets, setTargets] = useState<Target[]>([]);
  const [target, setTarget] = useState("");
  const [shot, setShot] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  const [status, setStatus] = useState("No browser is open on this device.");

  const list = async () => {
    try {
      const answer = JSON.parse(await callTool(grant, "browser.list", {}));
      setTargets(answer);
      if (answer.length > 0) {
        setTarget(answer[0].id ?? answer[0].target);
        setStatus(`${answer.length} target(s).`);
      } else {
        setStatus("No browser is open on this device.");
      }
    } catch (cause) {
      setStatus("List failed: " + message(cause));
    }
  };

  const capture = async (which = target) => {
    if (!which) return;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.screenshot", { target: which }));
      setShot(`data:image/png;base64,${answer.data}`);
      setStatus(`Captured ${answer.bytes} bytes.`);
    } catch (cause) {
      setStatus("Screenshot failed: " + message(cause));
    }
  };

  const choose = (which: string) => {
    setTarget(which);
    // The picture, when there is one, is of the target that was selected a moment ago.
    if (shot) void capture(which);
  };

  const open = async () => {
    if (!url.trim()) return;
    try {
      const answer = JSON.parse(await callTool(grant, "browser.open", { url: url.trim() }));
      setTarget(answer.target);
      await list();
      await capture(answer.target);
    } catch (cause) {
      setStatus("Open failed: " + message(cause));
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
      const answer = JSON.parse(await callTool(grant, "browser.eval", { target, expression }));
      setStatus("Clicked " + (answer.value ?? "the page") + ".");
      await capture();
    } catch (cause) {
      setStatus("Click failed: " + message(cause));
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
      const answer = JSON.parse(await callTool(grant, "browser.eval", { target, expression }));
      setStatus(String(answer.value ?? "typed"));
      setText("");
      await capture();
    } catch (cause) {
      setStatus("Type failed: " + message(cause));
    }
  };

  useEffect(() => {
    void list();
  }, []);

  return (
    <div className="browser-pane">
      <div className="panel-head">
        <h2>Browser</h2>
        <div className="actions">
          <button type="button" onClick={() => void list()}>List targets</button>
          <button type="button" disabled={!target} onClick={() => void capture()}>Refresh view</button>
        </div>
      </div>
      <p className={status.includes("failed") ? "error" : "hint"}>{status}</p>

      {targets.length > 0 && (
        // The browser can hold several pages; the screenshot follows the target, so the operator has
        // to be able to say which one. Without this the pane always showed whichever came first.
        <div className="row">
          <select value={target} onChange={(event) => choose(event.target.value)}>
            {targets.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.title || entry.url || entry.id}
              </option>
            ))}
          </select>
        </div>
      )}

      <div className="row">
        <input placeholder="https://example.test — opens or navigates" value={url} onChange={(event) => setUrl(event.target.value)} />
        <button type="button" onClick={() => void open()}>Open</button>
      </div>
      <div className="row">
        <input placeholder="text to type into the focused field" value={text} onChange={(event) => setText(event.target.value)} />
        <button type="button" disabled={!shot} onClick={() => void type()}>Type</button>
      </div>

      {shot && <img className="browser-shot" src={shot} alt="" onClick={(event) => void click(event)} />}
    </div>
  );
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
