import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTopDialog } from "./dialogStack";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * One sheet for every focused job: Connect, Try it, Add plugin, Logs and Remove.
 *
 * Right drawer at ≥800 px (600 px), bottom sheet between 640 and 799, full screen on a phone. It traps Tab, closes on Escape and a backdrop press (not while `busy`), locks the body,
 * returns focus to the trigger and is labelled by its title. `prefers-reduced-motion` removes the
 * entrance; the product is complete without it.
 */
export function Sheet({
  title,
  children,
  footer,
  onClose,
  busy = false
}: {
  title: string;
  children: ReactNode;
  /// The pinned action row. Try it's Run lives here so a long result never pushes it off screen.
  footer?: ReactNode;
  onClose: () => void;
  /// A running call: Escape, the close button and the backdrop are inert until it answers.
  busy?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const panel = useRef({});
  const top = useTopDialog(panel.current);
  const lastFocused = useRef<HTMLElement | null>(null);
  const close = useRef(onClose);
  const busyRef = useRef(busy);
  const topRef = useRef(top);
  close.current = onClose;
  busyRef.current = busy;
  topRef.current = top;
  const titleId = useId();

  useEffect(() => {
    lastFocused.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = ref.current;
    const items = () =>
      Array.from(dialog?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    // Focus the job, not the way out: the first control in the body, falling back to the close
    // button only for a sheet with no fields (a log view, a confirmation).
    const first =
      dialog?.querySelector(".sheet-body")?.querySelector<HTMLElement>(FOCUSABLE) ??
      dialog?.querySelector<HTMLElement>(FOCUSABLE) ??
      dialog;
    first?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (!topRef.current) return;
      if (event.key === "Escape") {
        if (busyRef.current) return;
        event.preventDefault();
        close.current();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = items();
      if (focusable.length === 0) {
        event.preventDefault();
        dialog?.focus();
        return;
      }
      const firstItem = focusable[0];
      const lastItem = focusable[focusable.length - 1];
      if (!dialog?.contains(document.activeElement)) {
        event.preventDefault();
        firstItem.focus();
        return;
      }
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem.focus();
      }
    };

    document.addEventListener("keydown", onKey);
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = previousOverflow;
      lastFocused.current?.focus();
    };
  }, []);

  return createPortal(
    <div
      className="sheet-scrim"
      data-testid="sheet-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busyRef.current) close.current();
      }}
    >
      <div
        className="sheet"
        role="dialog"
        aria-modal={top ? "true" : undefined}
        aria-labelledby={titleId}
        tabIndex={-1}
        ref={ref}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="sheet-head">
          <h2 id={titleId}>{title}</h2>
          <button
            type="button"
            className="sheet-close"
            aria-label="Close"
            title="Close"
            disabled={busy}
            onClick={() => {
              if (!busy) close.current();
            }}
          >
            ×
          </button>
        </header>
        <div className="sheet-body">{children}</div>
        {footer && <footer className="sheet-foot">{footer}</footer>}
      </div>
    </div>,
    document.body
  );
}
