// lane.ts — A CHANNEL PREFIX'S LANE COLOUR, as a TABLE rather than a ladder (round 140 of the standing goal).
//
// The mapping lived in `Models.tsx` as eight `if (p === "…") return "lane-…";` lines and a fallback, under a docstring saying
// it was "the same mapping the Routes page uses" — and there is NO second copy in this console, so the sentence described a
// consumer that does not exist. Both are the spine's subject one layer out: a mapping is DATA, and written as control flow
// it hides two things — which prefixes have a lane at all, and that anything else falls silently to `lane-def`.
//
// WHAT IT DOES NOT DO: decide which prefixes exist. That is the gateway's payload (`or/` and the rest arrive from
// `/api/admin/public`), so an unknown prefix lands on `lane-def` on purpose — the table just makes that visible, and the
// sheet's rules are the other half of the same list (see the follow-up recorded in the ledger: a clause that compares them).
const LANE_CLASSES: Record<string, string> = {
  og: "lane-og",
  ds: "lane-ds",
  or: "lane-or",
  qw: "lane-qw",
  nv: "lane-nv",
  gmi: "lane-gmi",
  cm: "lane-cm",
  amd: "lane-amd",
};

/** The lane class for a channel prefix, with its trailing slash ignored (`or/` and `or` are one channel). */
export function laneClass(prefix: string): string {
  return LANE_CLASSES[prefix.replace(/\/$/, "")] ?? "lane-def";
}
