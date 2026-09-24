// useSessionArchive — the panel's read of `GET /api/sessions`: the sessions
// THIS DEVICE has recorded, durable on disk for 30 days, surviving agent
// restarts and page reloads.
//
// WHY A HOOK OF ITS OWN (and not `terminal_list`). The panel's session list
// comes from the `terminal_list` MCP tool, which answers for LIVE sessions
// only: once a session closes, its tab is inert and nothing else can reach it,
// and after a reload or a restart the past is unreachable entirely. The agent
// has served the durable list all along; nothing consumed it. This hook is that
// consumer, and it deliberately reads the ROUTE rather than a tool, because the
// route is the durable corpus and the tool is the live registry.
//
// WHY THERE IS NO POLL TIMER. Answering `/api/sessions` folds EVERY session file
// on the device (`session_log::list_sessions` → `terminal_state_of` per file),
// so a timer here would re-read hundreds of JSONL files from disk on a fixed
// cadence for a surface that is, by definition, about the past. Refreshes are
// event-driven instead, mirroring round-163's removal of the terminal list's 3 s
// poll: mount (which also happens on every page switch), the agent's
// `sessions-changed` push, and the tab regaining focus.
//
// HONEST STATE. The list has three distinguishable conditions and the component
// renders a different sentence for each: still reading, read OK (empty is an
// empty ARCHIVE), and the read FAILED. A failed read never reports "no sessions
// recorded" — that is a claim about the device drawn from a failure to reach it.
// The three words and the failure rule are `lib/readState.ts` + `useDeviceRead`'s
// now; this hook used to spell both by hand.
import { useEffect } from "react";
import { useDeviceRead } from "./useDeviceRead";
import { archiveEntries, type ArchiveEntry } from "../lib/archive";
import type { ReadState } from "../lib/readState";

export function useSessionArchive(): { entries: ArchiveEntry[]; state: ReadState } {
  // THE LOOP IS `useDeviceRead`'s (see its header): the mount read, the ordering guard this hook
  // was the first to write down, the unmount guard and keep-last-on-failure. What is left here is
  // the event wiring below and the throw, which is the caller's own rule rather than the module's.
  // Keep-last is right for THIS reader for a reason of its own, worth stating even though the rule
  // moved: the files the list names are durable on disk, so the last good list stays TRUE while a
  // refresh fails — what the read state adds is that the list is no longer passed off as current.
  const { data, read, refresh } = useDeviceRead<ArchiveEntry[]>({
    path: "/api/sessions",
    // archiveEntries THROWS on a response this panel does not understand. That is the point:
    // `[]` from a malformed body would render as "this device has recorded no sessions".
    // `useDeviceRead` catches the throw and reports the read as `"unreadable"`, keeping the last
    // good list — so the module's idiom for "a body this panel cannot use" is exactly what this
    // reader has always done by hand.
    reduce: (_previous, body) => archiveEntries(body),
    initial: [],
  });

  useEffect(() => {
    const onChange = () => { void refresh(); };
    // Focus only, not every visibility change: going HIDDEN is not a reason to
    // re-read every session file on the device.
    const onVisible = () => { if (document.visibilityState === "visible") void refresh(); };
    window.addEventListener("summrise-sessions-changed", onChange);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      // No `alive` flag to clear: an in-flight reply that lands after unmount is dropped inside
      // `useDeviceRead` (it owns the unmount guard, see its header).
      window.removeEventListener("summrise-sessions-changed", onChange);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refresh]);

  return { entries: data, state: read };
}
