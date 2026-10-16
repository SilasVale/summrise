//! THE CATALOGUE SLICE — a third party's facts about a model, COMMITTED rather than fetched at runtime.
//!
//! `scripts/update-model-catalogue.sh` fetches `models.dev/api.json`, extracts the six facts this repo uses for the
//! two providers we resell, and writes `catalogue/opencode.json` and `catalogue/opencode-go.json`. **Those two files
//! are the only thing the worker reads**: no network, no KV, no timer — a pure lookup that can be tested in-process,
//! which is what makes it usable from the store filter, the adoption path and the console's copy alike.
//!
//! **THE SIXTH FACT THE PLAN NAMED HAS NO SOURCE, AND THIS MODULE DOES NOT INVENT ONE.** Task 3's interface says
//! "`Shape` from `provider.shape`"; measured against the live document (2026-10-16) a provider carries `api` (a URL),
//! `doc`, `env`, `id`, `models`, `name` and `npm` (an SDK package) — **there is no shape field**, and nothing on the
//! model says whether it answers `completions` or `responses`. Adoption therefore cannot take a dialect from here
//! (Task 7 needs another source), and a guessed dialect is exactly how a row ends up on the wrong arm.
//!
//! **ABSENCE IS NOT EVIDENCE**: `facts_for` returns `None` for a row the slice does not carry, while a row it does
//! carry answers `Some(Facts { .. })` whose individual facets may still be `None`. The two are different questions —
//! "we know nothing about this id" and "the source does not state its context window" — and the console shows them
//! differently.
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const OPENCODE: &str = include_str!("catalogue/opencode.json");
const OPENCODE_GO: &str = include_str!("catalogue/opencode-go.json");

/// One row's facts, as the slice carries them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
    pub context: Option<i64>,
    pub output: Option<i64>,
    /// The provider's own word — `deprecated` is the one this document uses today.
    pub status: Option<String>,
    /// Does it accept an image? From `modalities.input`.
    pub vision: bool,
}

/// The two providers' rows, keyed by id, parsed ONCE.
pub struct Slice {
    rows: BTreeMap<String, Facts>,
}

impl Slice {
    /// The facts for an id the slice carries; `None` means the slice does not know this id at all.
    pub fn facts_for(&self, id: &str) -> Option<&Facts> {
        self.rows.get(id)
    }

    /// How many rows the slice carries — for a floor in tests rather than for behaviour.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// `opencode-go`'s ids carry the `og/` prefix in this repo and the bare slug in the source, so both spellings are
/// indexed: **a lookup that missed the prefix would report "no facts" for every model we actually sell.**
fn prefixes() -> [(&'static str, &'static str); 2] {
    [("og/", OPENCODE_GO), ("", OPENCODE)]
}

fn parse_into(rows: &mut BTreeMap<String, Facts>, text: &str, prefix: &str) {
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(array) = doc.as_array() else { return };
    for row in array {
        let Some(id) = row["id"].as_str() else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        rows.insert(
            format!("{prefix}{id}"),
            Facts {
                input_cost: row["input_cost"].as_f64(),
                output_cost: row["output_cost"].as_f64(),
                context: row["context"].as_i64(),
                output: row["output"].as_i64(),
                status: row["status"].as_str().map(str::to_string),
                vision: row["vision"].as_bool().unwrap_or(false),
            },
        );
    }
}

/// **THE SLICE, PARSED ONCE** — a process-lifetime constant like the registry, and pure in between.
pub fn slice() -> &'static Slice {
    static SLICE: OnceLock<Slice> = OnceLock::new();
    SLICE.get_or_init(|| {
        let mut rows = BTreeMap::new();
        for (prefix, text) in prefixes() {
            parse_into(&mut rows, text, prefix);
        }
        Slice { rows }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **THE FLOOR IS THE POINT**: an `include_str!` that silently pointed at an empty file would make every lookup
    /// answer `None`, and every caller would believe the catalogue simply does not know these models.
    #[test]
    fn the_slice_carries_both_providers() {
        assert!(
            slice().len() >= 100,
            "only {} rows — the slice is not what it should be",
            slice().len()
        );
    }

    /// Facts come through, prefixed and bare.
    #[test]
    fn facts_come_through_for_both_spellings() {
        let go = slice()
            .facts_for("og/mimo-v2.6-pro")
            .expect("the go provider's row is indexed with its prefix");
        assert!(go.context.unwrap_or(0) > 0, "a context window came through");
        // **THE TWO PROVIDERS ARE INDEXED UNDER THE SPELLINGS THIS REPO USES FOR THEM**: `opencode-go` is the
        // `og/` channel (its source ids are bare slugs — `mimo-v2.6-pro` — and the registry advertises
        // `og/mimo-v2.6-flash`, which is the same family), while the `opencode` provider's rows are indexed bare.
        // The test's first version asked for a GO id without its prefix and failed — **which is the mapping telling
        // the truth about itself**.
        assert!(
            slice().facts_for("big-pickle").is_some(),
            "the bare provider's rows are indexed bare"
        );
        assert!(
            slice().facts_for("og/mimo-v2.6-pro").is_some(),
            "the go provider's rows are indexed under the og/ prefix"
        );
    }

    /// **ABSENCE IS NOT EVIDENCE**: an id the slice does not carry is `None`, which is a different answer from a row
    /// whose individual facets are missing.
    #[test]
    fn a_row_the_slice_does_not_carry_is_none() {
        assert!(slice().facts_for("og/nope").is_none());
        assert!(slice().facts_for("../../etc/passwd").is_none());
        assert!(slice().facts_for("").is_none());
    }

    /// A vision model is read from `modalities.input` — the one facet that is a membership test rather than a value.
    #[test]
    fn vision_is_read_from_the_input_modalities() {
        let with_image = slice().rows.values().filter(|f| f.vision).count();
        assert!(
            with_image > 0,
            "no row accepts an image — the field name moved"
        );
        assert!(
            with_image < slice().len(),
            "every row accepts an image — the test is not reading a facet"
        );
    }
}
