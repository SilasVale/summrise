//! THE ROUTE TABLE'S DISPATCH COUNTER — the dead-route report, in the worker that serves the routes.
//!
//! **WHAT THIS REPLACES, AND WHY IT IS NOT THE SAME SHAPE.** In the console worker, `GET /api/plugins/status`
//! answered `routes: [{index, hits}]` from the PLUGIN REGISTRY: every route a plugin had registered, in
//! registration order, with the number of times it had been dispatched. Two things ended that:
//!
//!   1. **THE REGISTRY IS GONE.** The plugins were deleted over seven landings (their routes moved to this
//!      worker), so by the last one the "table" held a single row and the report answered a question about a
//!      table that no longer described the API.
//!   2. **THE TABLE THAT DOES DESCRIBE THE API IS THIS WORKER'S**, and its granularity is a FAMILY: the front
//!      door hands over a family, and this worker dispatches inside it. **SO THE ROWS ARE FAMILIES, COUNTED
//!      WHERE THE DISPATCH ACTUALLY HAPPENS** — not a hand-maintained list of sixty paths that would drift
//!      from the dispatch tables the moment one was added. The report is coarser than the old one and it is
//!      TRUE, which is the trade a drift-prone enumeration loses every time.
//!
//! **ONE COUNTER PER ISOLATE, LIKE THE SOURCE'S**: the numbers reset when the isolate does, and they are
//! therefore a statement about "since this isolate started", not about the deployment's lifetime. The console
//! never rendered them; the operator is who reads them.

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{json, Value};

/// The families this worker dispatches, in a FIXED order — the `index` a row reports is its position here.
pub const FAMILIES: [&str; 8] = [
    "auth",
    "admin",
    "devices",
    "device_proxy",
    "mcp",
    "upload",
    "tooling",
    "status",
];

thread_local! {
    /// `family → hits`. A map rather than a counter array because the rows are addressed by name at the call
    /// sites, and a `HashMap` over eight short strings is cheaper to keep right than an index table.
    static HITS: RefCell<HashMap<&'static str, u64>> = RefCell::new(HashMap::new());
}

/// Note one dispatch. **CALLED WHERE THE DISPATCH HAPPENS, NOT WHERE THE PATH PARSES**: a family is counted
/// once per request, before its handler, so a path that matches a family but throws inside it is still a hit —
/// which is what "was this route ever reached?" asks.
pub fn note(family: &'static str) {
    HITS.with(|h| {
        *h.borrow_mut().entry(family).or_insert(0) += 1;
    });
}

/// `routeStats()` — `[{index, hits}]`, in the family table's order, with a never-dispatched family reporting
/// zero (which is the point of the instrument: a row that is always zero is a route nobody calls).
pub fn stats() -> Value {
    let rows = HITS.with(|h| {
        let hits = h.borrow();
        FAMILIES
            .iter()
            .enumerate()
            .map(|(index, family)| {
                json!({ "index": index, "hits": hits.get(family).copied().unwrap_or(0) })
            })
            .collect::<Vec<Value>>()
    });
    Value::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_the_family_table_in_its_own_order() {
        // **THE TEST IS ABOUT THE SHAPE AND THE ORDER, NOT ABOUT GLOBAL STATE**: another test in this binary may
        // have counted a dispatch, so the hits are read per row rather than asserted as a whole vector.
        let rows = stats();
        let rows = rows.as_array().expect("an array");
        assert_eq!(rows.len(), FAMILIES.len());
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["index"], json!(i), "row {i} must report its own index");
            assert!(row["hits"].is_u64(), "row {i} must carry a count");
        }
        // Every family has a row, in the table's own order — a family that has never been noted reports ZERO
        // rather than being absent, which is the dead-route signal.
        for (i, family) in FAMILIES.iter().enumerate() {
            let row = rows
                .iter()
                .find(|r| r["index"] == json!(i))
                .unwrap_or_else(|| panic!("no row for {family}"));
            assert!(row["hits"].is_u64());
        }
    }

    #[test]
    fn noting_a_dispatch_moves_that_family_only() {
        let before = stats();
        let Some(first) = FAMILIES.first() else {
            return;
        };
        note(first);
        let after = stats();
        let hits = |v: &Value, i: usize| v[i]["hits"].as_u64().unwrap_or(0);
        let i = 0;
        assert_eq!(hits(&after, i), hits(&before, i) + 1, "{first} must move");
        for j in 1..FAMILIES.len() {
            assert_eq!(hits(&after, j), hits(&before, j), "row {j} must not move");
        }
    }
}
