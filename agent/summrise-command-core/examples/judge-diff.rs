//! A DIFFERENTIAL HARNESS FOR `judge_report` — the instrument that makes a refactor of the judge provable
//! instead of hopeful.
//!
//! WHY IT EXISTS. `judge_report` decides what a sweep report means: which findings a run files, IN WHICH
//! ORDER, the sentences it prints and the notes it hands back. A refactor of it must not move one byte of
//! that output, and the crate's own suite did not pin every clause (the mutation table recorded beside the
//! change that split the function says exactly which clauses nothing notices), so "the tests pass" is not
//! the safety argument. The argument is: run this harness over a corpus x option sets BEFORE the change,
//! run it AFTER, and compare md5 — and prove the harness REACHES every clause by emptying each clause's
//! body in turn and showing this output changes.
//!
//! WHY IT IS AN EXAMPLE *AND* THERE IS NOW A TEST BESIDE IT. These are two tools with two jobs, and the
//! earlier version of this header called only one of them right:
//!
//!   * THIS EXAMPLE IS THE INSTRUMENT. It PRINTS; the comparison is run by hand; it exists to BUILD cases
//!     and to keep the inventory of UNNOTICED clauses meaningful while clauses move. It must not fail on a
//!     legitimate behaviour change, because the person making one is its reader.
//!   * `crate::judge_corpus`'s `judge_golden` test is THE PIN. It runs the same corpus against a committed
//!     golden file and goes red on ANY movement — which is the opposite of a defect and is exactly what nine
//!     clauses lacked: they were noticed only when one fixture (`design-sweep/axes.json`) moved, so a single
//!     regeneration would have taken all nine dark at once. A pin that notices every mutation is not a
//!     destroyed inventory; it is the floor the inventory was standing on.
//!
//! THE CORPUS IS NOT HERE — it is `crate::judge_corpus`, shared with the pin byte for byte, so the
//! instrument and the test can never compare two different corpora. This file is the printer.
//!
//! USAGE:
//!
//!     cargo run -p summrise-agent-core --example judge-diff > /tmp/judge-before.txt
//!     md5sum /tmp/judge-before.txt
//!
//! Every string is printed JSON-escaped, so the dump is one line per finding/line/note and the difference
//! between two runs is a line diff rather than an eyeball.

fn main() {
    print!("{}", summrise_agent_core::judge_corpus::render());
}
