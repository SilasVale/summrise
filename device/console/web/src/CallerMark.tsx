/**
 * The console's own call, marked where a row names its machine.
 *
 * The console writes its own explicit actions — a person's list refresh, a retry, an export — on the
 * operator session, and those calls are audited like any other. Its successful automatic polls are
 * not: they are the console redrawing the screen it already shows. A failed poll stays a `poll` line
 * so the machine's silence is visible in the table; it carries `caller: "console"` and this quiet
 * word, because the retry was the console's, not the AI's. A client's call renders nothing, and so
 * does a line older than the `caller` field, because an unknown caller is not a client's call.
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
