import { useEffect, useState } from "react";

/**
 * The dialogs that are open, oldest first. A Sheet can hold a Modal (the Connect sheet's revoke
 * confirmation is the live case), so Escape and `aria-modal` belong to the top of one stack rather
 * than to every component's own listener.
 */
const openDialogs: object[] = [];
const listeners = new Set<() => void>();

export function useTopDialog(id: object): boolean {
  const [top, setTop] = useState(false);
  useEffect(() => {
    openDialogs.push(id);
    const sync = () => setTop(openDialogs[openDialogs.length - 1] === id);
    listeners.add(sync);
    for (const listener of listeners) listener();
    return () => {
      listeners.delete(sync);
      const at = openDialogs.indexOf(id);
      if (at >= 0) openDialogs.splice(at, 1);
      for (const listener of listeners) listener();
    };
  }, [id]);
  return top;
}
