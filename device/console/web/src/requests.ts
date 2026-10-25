import { useEffect, useRef } from "react";

/** The two answers a page needs to know whether a response still belongs to the view it shows. */
type RequestGate = { begin(): number; stale(ticket: number): boolean };

/**
 * The newest request wins.
 *
 * `begin()` claims a ticket and invalidates every ticket before it. When the answer arrives,
 * `stale(ticket)` says whether that request is still the newest: a superseded answer drops both its
 * data and its error, because either would describe a view the operator has already left.
 *
 * The counter is a ref, not state: claiming a ticket must not render, and the check compares against
 * the value at the moment the answer lands, not the one the call site closed over. Unmounting the
 * page invalidates every ticket it still holds, so a late answer cannot set state on a page that is
 * gone (the same epoch also ends when a page starts over for a different grant).
 */
export function useRequestGate(): RequestGate {
  const newest = useRef(0);
  const gate = useRef<RequestGate>({
    begin: () => {
      newest.current += 1;
      return newest.current;
    },
    stale: (ticket: number) => ticket !== newest.current
  });

  useEffect(
    () => () => {
      // The page is gone: nothing it still holds can be the newest request.
      newest.current += 1;
    },
    []
  );

  return gate.current;
}
