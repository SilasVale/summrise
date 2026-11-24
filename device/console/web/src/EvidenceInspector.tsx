import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { useMediaQuery } from "./useMediaQuery";

/** A view of one selected record. Its caller supplies the existing evidence and owns selection. */
export function EvidenceInspector({
  id,
  selectedKey,
  summary,
  onClose,
  children
}: {
  id: string;
  selectedKey: string | null;
  summary?: string;
  onClose: () => void;
  children?: ReactNode;
}) {
  const sheet = useMediaQuery("(max-width: 1023px)");
  const close = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLElement>(null);

  useEffect(() => {
    if (selectedKey !== null) close.current?.focus({ preventScroll: true });
  }, [selectedKey, sheet]);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (!sheet || event.key !== "Tab") return;
    const controls = Array.from(
      panel.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      ) ?? []
    ).filter((element) => !element.closest("[hidden], [inert]"));
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  if (selectedKey === null) {
    if (sheet) return null;
    return (
      <aside id={id} className="panel activity-inspector is-empty" aria-label="Selected activity evidence">
        <header className="activity-inspector-head">
          <h2>Evidence</h2>
        </header>
        <div className="activity-inspector-body">
          <p className="hint">Select a record to inspect its outcome, context and retained evidence.</p>
        </div>
      </aside>
    );
  }

  return (
    <>
      {sheet && (
        <div
          className="sheet-scrim activity-inspector-scrim"
          aria-hidden="true"
          onClick={onClose}
        />
      )}
      <aside
        id={id}
        ref={panel}
        className={"panel activity-inspector" + (sheet ? " is-sheet" : "")}
        role={sheet ? "dialog" : "complementary"}
        aria-label="Selected activity evidence"
        aria-modal={sheet ? true : undefined}
        onKeyDown={onKeyDown}
      >
        <header className="activity-inspector-head">
          <h2>Evidence</h2>
          <button ref={close} type="button" className="mini" aria-label="Close evidence" onClick={onClose}>
            Close
          </button>
          {summary && <p className="hint activity-inspector-context">{summary}</p>}
        </header>
        <div className="activity-inspector-body" role="region" aria-label="Selected evidence details" tabIndex={0}>{children}</div>
      </aside>
    </>
  );
}
