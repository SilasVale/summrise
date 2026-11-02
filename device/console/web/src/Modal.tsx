import { useEffect, useRef } from "react";
import { useTopDialog } from "./dialogStack";

// One dialog at a time, Escape closes it, focus goes to the first field, Tab stays inside, and the
// trigger gets focus back on close — the things a hand-rolled dialog forgets. The legacy panel's
// `.modal` had the same shape.
export function Modal({
  title,
  children,
  onClose
}: {
  title: string;
  children: React.ReactNode;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // The caller's `onClose` is usually an inline arrow with a new identity every render, so the
  // window listener cannot depend on it: reading it through a ref keeps the listener mounted once.
  const close = useRef(onClose);
  const lastFocused = useRef<HTMLElement | null>(null);
  const id = useRef({});
  const top = useTopDialog(id.current);
  // The key listener is registered once, so it reads the current top through a ref.
  const topRef = useRef(top);
  topRef.current = top;

  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    lastFocused.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = ref.current;
    const focusable = () =>
      Array.from(
        dialog?.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
        ) ?? []
      );
    dialog?.querySelector<HTMLElement>("input, select, textarea, button, a[href]")?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (!topRef.current) return;
      if (event.key === "Escape") {
        close.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) {
        event.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (!dialog?.contains(document.activeElement)) {
        event.preventDefault();
        first.focus();
        return;
      }
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      lastFocused.current?.focus();
    };
  }, []);

  return (
    <div className="scrim" data-testid="scrim" onMouseDown={onClose}>
      <div
        className="modal"
        role="dialog"
        aria-modal={top ? "true" : undefined}
        aria-label={title}
        ref={ref}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}
