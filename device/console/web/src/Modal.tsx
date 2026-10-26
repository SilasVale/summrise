import { useEffect, useRef } from "react";

// One dialog at a time, Escape closes it, and focus goes to the first field — the three things a
// hand-rolled dialog forgets. The legacy panel's `.modal` had the same shape.
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

  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Once on mount: re-focusing on a parent render would pull the caret out of the field being typed
  // in as soon as a controlled input changed.
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("input, select, textarea, button")?.focus();
  }, []);

  return (
    <div className="scrim" data-testid="scrim" onMouseDown={onClose}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={title} ref={ref} onMouseDown={(event) => event.stopPropagation()}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}
