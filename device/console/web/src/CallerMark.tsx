/**
 * The console's own read, marked where a row names its machine.
 *
 * The console calls `/mcp` on the operator session for its own page — the terminal's session poll and
 * the terminal stream's parked reads — and those calls are audited like any other. They belong in the
 * record: hiding them would be a different lie, and the audit is the record. One quiet word is what
 * tells them from the AI's calls; a client's call renders nothing, and so does a line older than the
 * `caller` field, because an unknown caller is not a client's call.
 */

/** What the muted word explains when a pointer rests on it. */
const TITLE = "The console made this call on its own behalf; it is not an AI client's call";

/** The word beside the machine, in the timeline (`timeline-caller`) and the table (`audit-caller`). */
export function ConsoleCaller(props: { className: string }) {
  return (
    <span className={props.className} title={TITLE}>
      console
    </span>
  );
}
