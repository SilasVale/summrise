// ConsoleKeys — the bar itself. The vocabulary and the wire mapping live in
// `lib/consoleKeys.ts` (pure, tested); this component renders them and reports failures the
// same way the pane's own keystroke path does (`vale-write-failed`), so a key that did not
// reach the device never looks like one that did.
//
// IT IS DELIBERATELY NOT A TOGGLE-ABLE MODE. These are one-shot signals: pressing ^C sends one
// interrupt, and pressing BRK asserts one break. A "sticky Ctrl" would be a different feature
// with a different failure mode (a modifier left on is a console that eats the next command),
// and nothing here needs it.
import { callTool } from "../lib/api";
import { keysForKind, keyParams, type ConsoleKey } from "../lib/consoleKeys";

export function ConsoleKeys({ sessionId, kind }: { sessionId: string; kind?: string | null }) {
  const keys = keysForKind(kind);
  const send = (key: ConsoleKey) => {
    callTool("terminal_write", keyParams(key, sessionId)).catch(() => {
      // The pane's own convention: a failed write is announced, never swallowed. A break that
      // silently did nothing would have an operator waiting for a bootloader that never stops.
      window.dispatchEvent(
        new CustomEvent("vale-write-failed", { detail: { sid: sessionId, key: key.id } }),
      );
    });
  };
  return (
    <div className="term-keybar" role="toolbar" aria-label="Console keys">
      {keys.map((k) => (
        <button key={k.id} type="button" title={k.title} data-key={k.id} onClick={() => send(k)}>
          {k.label}
        </button>
      ))}
    </div>
  );
}
