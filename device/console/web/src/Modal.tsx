import { useEffect, useRef, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useTopDialog } from "./dialogStack";

const SEQUENTIAL_CANDIDATES = 'a[href], button, input, select, textarea, summary, [tabindex]';

/** Native sequential targets, including disclosures and excluding unavailable branches. */
export function getSequentialFocusable(root: HTMLElement | null): HTMLElement[] {
  return Array.from(root?.querySelectorAll<HTMLElement>(SEQUENTIAL_CANDIDATES) ?? []).filter((element) => {
    if (element.tabIndex < 0 || element.matches(':disabled, input[type="hidden"]') || element.closest("[hidden], [inert]")) {
      return false;
    }
    const view = element.ownerDocument.defaultView;
    const visibility = view?.getComputedStyle(element).visibility;
    if (visibility === "hidden" || visibility === "collapse") return false;
    for (let branch: HTMLElement | null = element; branch; branch = branch.parentElement) {
      if (view?.getComputedStyle(branch).display === "none") return false;
      if (branch.tagName === "DETAILS" && !branch.hasAttribute("open")) {
        const summary = Array.from(branch.children).find((child) => child.tagName === "SUMMARY");
        if (!summary?.contains(element)) return false;
      }
    }
    return true;
  });
}
const bodyLocks = new WeakMap<HTMLElement, { count: number; overflow: string }>();

/** Shared presentation isolation for Modal and Sheet portals. */
export function useDialogIsolation(ref: RefObject<HTMLElement | null>, top: boolean) {
  useEffect(() => {
    const body = ref.current?.ownerDocument.body;
    if (!body) return;
    const lock = bodyLocks.get(body) ?? { count: 0, overflow: body.style.overflow };
    lock.count++;
    bodyLocks.set(body, lock);
    body.style.overflow = "hidden";
    return () => {
      if (--lock.count === 0) {
        body.style.overflow = lock.overflow;
        bodyLocks.delete(body);
      }
    };
  }, [ref]);

  useEffect(() => {
    const dialog = ref.current;
    if (!top || !dialog) return;
    const previous = new Map<HTMLElement, string | null>();
    const restore = (element: HTMLElement, value: string | null) => {
      if (value === null) element.removeAttribute("inert");
      else element.setAttribute("inert", value);
    };
    const sync = () => {
      const background = new Set<HTMLElement>();
      let branch: HTMLElement = dialog;
      while (branch.parentElement) {
        for (const sibling of branch.parentElement.children) {
          if (sibling !== branch && sibling instanceof HTMLElement) background.add(sibling);
        }
        if (branch.parentElement === dialog.ownerDocument.body) break;
        branch = branch.parentElement;
      }
      for (const [element, value] of previous) {
        if (!background.has(element)) {
          restore(element, value);
          previous.delete(element);
        }
      }
      for (const element of background) {
        if (!previous.has(element)) previous.set(element, element.getAttribute("inert"));
        element.setAttribute("inert", "");
      }
    };
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(dialog.ownerDocument.body, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      for (const [element, value] of previous) restore(element, value);
    };
  }, [ref, top]);
}

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
  useDialogIsolation(ref, top);

  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    // StrictMode replays this effect after focus moved inside; retain the original opener.
    lastFocused.current ??= document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const dialog = ref.current;
    const focusable = () => getSequentialFocusable(dialog);
    (focusable()[0] ?? dialog)?.focus();

    const onKey = (event: KeyboardEvent) => {
      if (!topRef.current) return;
      if (event.key === "Escape") {
        event.preventDefault();
        close.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) {
        event.preventDefault();
        dialog?.focus();
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
      if (topRef.current && lastFocused.current?.isConnected) lastFocused.current.focus();
    };
  }, []);

  return createPortal(
    <div
      className="scrim"
      data-testid="scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && topRef.current) close.current();
      }}
    >
      <div
        className="modal"
        role="dialog"
        aria-modal={top ? "true" : undefined}
        aria-label={title}
        tabIndex={-1}
        ref={ref}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>,
    document.body
  );
}
