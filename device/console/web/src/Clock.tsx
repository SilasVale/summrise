import { clockTimeSeconds, exactMoment } from "./format";

/**
 * The clock, to the second, in the console's timezone.
 *
 * `clockTimeSeconds` is the one formatter; this alias exists because the record and the Table have
 * called it `clock` since audit-41 and a rename would be a second spelling of the same idea.
 */
export function clock(ts: number): string {
  return clockTimeSeconds(ts);
}

/**
 * THE ONE CLOCK (audit-43 F8).
 *
 * The visible moment is `HH:MM:SS`; the exact instant — local to the second plus the UTC ISO — is
 * the `title`, and `dateTime` is the UTC instant itself. The seconds stay in their own span so a
 * future layout could hide only them, but no layout does: a minute cannot separate two calls eight
 * seconds apart, so the phone keeps the seconds too.
 *
 * The Table, the Timeline and the Runs history import this element, so their vocabularies cannot
 * drift apart again.
 */
export function Clock(props: { ts: number }) {
  const full = clock(props.ts);
  return (
    <time
      className="timeline-time"
      dateTime={new Date(props.ts).toISOString()}
      title={exactMoment(props.ts)}
    >
      <span className="timeline-clock">{full.slice(0, 5)}</span>
      <span className="timeline-clock-seconds">{full.slice(5)}</span>
    </time>
  );
}

/**
 * The clock with its age beside it: `08:46:18 · 8s ago`.
 *
 * The exact instant stays on the `<time>`'s title. The age's phrase is the caller's — the Activity
 * surfaces count whole units, the Runs history keeps the seconds — but the separator and the one
 * space around it are this component's, so no surface can glue `·` to the clock again (audit-43 F8
 * measured `08:46:18· 8s ago`).
 */
export function Moment(props: {
  ts: number;
  /** The age's whole phrase, already carrying its noun: `8s ago`, `never`. */
  age: string;
  className?: string;
  ageClassName?: string;
}) {
  return (
    <span className={"moment" + (props.className === undefined ? "" : ` ${props.className}`)}>
      <Clock ts={props.ts} />
      <span className={props.ageClassName}>{" · "}{props.age}</span>
    </span>
  );
}
