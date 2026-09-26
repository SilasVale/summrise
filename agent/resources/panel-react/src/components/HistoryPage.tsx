// HistoryPage — WHAT THIS DEVICE RECORDED, in one place.
//
// WHY ONE PAGE. `Archive` (every recorded session, and the trail inside any one of them) and
// `Activity` (what the device has been doing, grouped by run) answer the same question a person
// actually asks — "what happened here?" — and they sat behind two rail icons with no way to tell
// from the outside which one held the thing you wanted. The operator's own complaint was not
// knowing what the icons were for; the getting-started card now names three pages, and this merge
// is the same idea applied to the rail itself.
//
// THE TWO BODIES ARE UNCHANGED. This is a MERGE, not a rewrite: the sections keep their own
// fetching, their own honest empties and their own tests. What is new is the SCOPE SWITCH — the
// panel's existing `.view-switch` idiom, so it looks and behaves like the per-session switch —
// and a scope line saying which question each half answers.
//
// THE SWITCH IS A TABLIST, like the session switch: a segmented control that only LOOKS like
// buttons must still tell a screen reader it is one of a set with a selected member.

import { useState } from "react";
import { ActivityPage } from "./ActivityPage";
import { ArchivePage } from "./ArchivePage";
import type { Session } from "../hooks/useSessions";

/** The two halves of the record.
 *
 *  THE LABELS WERE "Sessions" AND "Runs" UNTIL 2026-09-26, and the operator asked whether the two tabs should become one
 *  page with a filter, "since a run and a session are two readings of the same work" (their inbox, row 19).
 *
 *  **THE ANSWER IS NO, AND THE DATA SAYS SO.** A session is the CONTAINER; a run is declared INSIDE one — `CONTEXT.md`
 *  states it as "a run exists only where a session declared one", and records from a session that never declared one
 *  belong to no run at all. So the two tabs are two GROUPINGS of the same records, and "one page with a filter" would be
 *  exactly those two tabs plus a click. Merging them would not remove a concept; it would hide one.
 *
 *  WHAT MADE THEM READ AS DUPLICATES WAS THE LABELS, WHICH IS THE SAME DEFECT "Trajectory"/"Path" HAD: neither word said
 *  what changes when you switch. **"By session" and "By run" name the axis**, so the switch explains itself and the
 *  section headings below can keep the plain nouns. */
const HISTORY_SCOPES = [
  {
    id: "sessions",
    label: "By session",
    title: "Every session this device recorded, and the trail inside any one of them",
    hint: "grouped by session · newest first",
  },
  {
    id: "runs",
    label: "By run",
    title: "What this device has been doing, grouped by the run it belonged to",
    hint: "grouped by the run each record belonged to",
  },
] as const;

type HistoryScope = (typeof HISTORY_SCOPES)[number]["id"];

export function HistoryPage({ sessions }: { sessions: Session[] }) {
  const [scope, setScope] = useState<HistoryScope>("sessions");
  const active = HISTORY_SCOPES.find((s) => s.id === scope)!;

  return (
    <div className="history-page">
      {/* The page's own name, for the outline only: on screen the rail icon and the scope switch
          already say where you are, and a visible "History" heading would be a third label for the
          same thing. The sections below stay h2 ("Sessions" / "Runs"). */}
      <h1 className="sr-only">History</h1>
      <div className="history-bar">
        <div className="view-switch" role="tablist" aria-label="Recorded history">
          {HISTORY_SCOPES.map((s) => (
            <button
              key={s.id}
              type="button"
              role="tab"
              aria-selected={scope === s.id}
              title={s.title}
              className={`view-switch-btn${scope === s.id ? " active" : ""}`}
              onClick={() => setScope(s.id)}
            >
              {s.label}
            </button>
          ))}
        </div>
        {/* The scope line is the switch's own caption: it answers "which records am I looking at"
            without the reader having to open the other half to find out.
            IT LIVES HERE AND NOWHERE ELSE. Both sections used to carry their own scope line, which
            after the merge said the same thing 40px below this one — a screenshot of the merged
            page showed the duplication. The section keeps its HEADING (structure and the accessible
            name of the region) and its lede (the explanation); the caption is the page's. */}
        <span className="history-hint">{active.hint}</span>
      </div>
      {scope === "sessions" ? <ArchivePage sessions={sessions} /> : <ActivityPage />}
    </div>
  );
}
