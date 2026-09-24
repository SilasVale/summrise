/** The three states one read of the device can be in. Owned HERE because the
 *  panel's most-repeated invariant — a read that failed must not render as a
 *  device that is empty — is one fact, and it used to be spelled twice. */
export type ReadState = "reading" | "ok" | "unreadable";

// WHY THE TYPE LEFT THE HOOKS. The rule above is drawn on by every surface that
// renders an empty trail or an empty chart, and the three words were declared
// twice: `SessionReadState` in `hooks/useCommandEvents.ts`, and a private
// `ArchiveListState` twin in `hooks/useSessionArchive.ts` whose own comment said
// it mirrored the first. One of the two living in a hook had a third cost —
// `lib/trailRead.ts` imported the type FROM `hooks/useCommandEvents`, so the
// lib/ layer depended on the hooks/ layer to name a fact neither owns. The
// vocabulary lives in lib/ now, beside the readers that share it.
