//! THE ROWS DOCUMENT — what a run ASKED and what came back, and nothing about what it means.
//!
//! **THIS IS THE CONTRACT THE DRIVER AND THE JUDGE MEET ON**, and it is deliberately the shape the e2e
//! landing settled on (`agent/scripts/e2e/README.md`): `{schema, base, plan, observations}`, where an
//! observation is `{id, args, result|error, at_ms}`. The driver's exit code says whether it COLLECTED —
//! never whether the page passed — and the verdict is the judge's, from the same bytes.
//!
//! **A MISSING FIELD IS AN ERROR NAMING THE FIELD, NOT A DEFAULT.** That is Review Focus 5 of the plan, and
//! it is the difference between two failures a reader cannot tell apart: a rows document from an older
//! schema, and a run that measured nothing. `#[serde(default)]` on `observations` would turn the first into
//! the second — the judge would read an empty list, find nothing wrong, and say so — which is exactly the
//! "a scan that read nothing is not a clean scan" rule this suite applies everywhere else. So the fields are
//! checked by name, in order, before anything is deserialized.
//!
//! **AND AN OBSERVATION THAT NEITHER SUCCEEDED NOR FAILED IS NOT AN OBSERVATION.** `result` and `error` are
//! both optional so that a partly-collected run can be written down — the e2e driver writes its rows even
//! when it aborts mid-way — but the pair being absent is refused, because a step with no outcome is a step
//! the runtime silently skipped.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The schema this reader understands. Bumped when the shape changes, so a reader refuses an older document
/// by name rather than by accident.
pub const SCHEMA: &str = "summrise.design-sweep.rows/1";

/// One thing the driver asked for, and what came back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// `<surface>.<section>`, from `passes::Step`.
    pub id: String,
    /// What was asked, verbatim — the plan's own arguments for that rung.
    #[serde(default)]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When it came back, on the run's own clock. Absent for a step that never ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

impl Observation {
    /// Did this observation produce anything at all? A step that neither succeeded nor failed was skipped.
    pub fn collected(&self) -> bool {
        self.result.is_some() || self.error.is_some()
    }

    /// Did it fail? An `error` is a failure whether or not a partial `result` came with it.
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
}

/// The whole document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rows {
    pub schema: String,
    /// The revision the run measured, so a verdict can say what it is about.
    pub base: String,
    /// The plan the run was given, as data — the same bytes `--emit` wrote.
    pub plan: Value,
    pub observations: Vec<Observation>,
}

/// Parse a rows document, refusing a document whose fields are not all there — **by name**.
///
/// The order of the checks is the order a reader reads the document, and each message names the field, the
/// schema this reader knows, and what the absence would have meant.
pub fn parse(text: &str) -> Result<Rows, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("the rows document is not JSON: {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| format!("the rows document is {} rather than an object", kind_of(&v)))?;
    for field in ["schema", "base", "plan", "observations"] {
        if !obj.contains_key(field) {
            return Err(format!(
                "the rows document has no `{field}` field — refusing rather than defaulting, because an \
                 absent `{field}` is indistinguishable from a run that measured nothing and would read as a \
                 clean sweep. This reader understands `{SCHEMA}`; the fields are `schema`, `base`, `plan` \
                 and `observations`."
            ));
        }
    }
    let rows: Rows = serde_json::from_value(v)
        .map_err(|e| format!("the rows document does not fit `{SCHEMA}`: {e}"))?;
    if rows.schema != SCHEMA {
        return Err(format!(
            "the rows document says its schema is `{}` and this reader understands `{SCHEMA}` — refusing an \
             older or newer document rather than reading the fields that happen to be spelled the same",
            rows.schema
        ));
    }
    for (i, o) in rows.observations.iter().enumerate() {
        if o.id.trim().is_empty() {
            return Err(format!(
                "observation {i} has no `id`, so a finding against it would name nothing"
            ));
        }
        if !o.collected() {
            return Err(format!(
                "observation {} (`{}`) carries neither `result` nor `error` — a step with no outcome is a \
                 step the runtime skipped, and reporting it as measured is the failure this field pair \
                 exists to make impossible",
                i, o.id
            ));
        }
    }
    Ok(rows)
}

/// The JSON kind of a value, for a message that says what it got instead of what it wanted.
fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> String {
        json!({
            "schema": SCHEMA,
            "base": "dfa593c9",
            "plan": { "sections": ["timing"] },
            "observations": [
                { "id": "panel-light-Terminal.timing", "args": {}, "result": { "toFirstRowMs": 812 }, "at_ms": 812 }
            ]
        })
        .to_string()
    }

    /// The happy path, so the refusals below are refusals of something that otherwise works.
    #[test]
    fn a_well_formed_document_reads() {
        let r = parse(&good()).expect("a well-formed document");
        assert_eq!(r.schema, SCHEMA);
        assert_eq!(r.observations.len(), 1);
        assert!(r.observations[0].collected());
        assert!(!r.observations[0].failed());
    }

    /// A missing field is named, and the message says what the absence would have meant — a refusal a
    /// reader can act on rather than a deserializer's line number.
    #[test]
    fn a_missing_field_is_refused_by_name() {
        let e = parse("{}").expect_err("an empty object is not a rows document");
        assert!(e.contains("`observations`"), "{e}");
        assert!(
            e.contains(SCHEMA),
            "the reader should say which schema it knows: {e}"
        );
        for field in ["schema", "base", "plan", "observations"] {
            let mut v: Value = serde_json::from_str(&good()).unwrap();
            v.as_object_mut().unwrap().remove(field);
            let e = parse(&v.to_string()).unwrap_err();
            assert!(
                e.contains(&format!("`{field}`")),
                "removing `{field}` gave: {e}"
            );
        }
    }

    /// A step with no outcome is refused, because it is the one shape that reads as "measured, nothing
    /// found".
    #[test]
    fn an_observation_with_no_outcome_is_refused() {
        let v = json!({
            "schema": SCHEMA,
            "base": "dfa593c9",
            "plan": {},
            "observations": [{ "id": "panel-light-Terminal.idle", "args": {} }]
        });
        let e = parse(&v.to_string()).unwrap_err();
        assert!(e.contains("panel-light-Terminal.idle"), "{e}");
        assert!(e.contains("result") && e.contains("error"), "{e}");
    }

    /// A different schema is refused rather than read field-for-field: the spelling matching is not the
    /// promise, the shape is.
    #[test]
    fn a_different_schema_is_refused() {
        let mut v: Value = serde_json::from_str(&good()).unwrap();
        v["schema"] = json!("summrise.design-sweep.rows/0");
        let e = parse(&v.to_string()).unwrap_err();
        assert!(e.contains("rows/0") && e.contains(SCHEMA), "{e}");
    }

    /// A document that is not an object at all says what it got.
    #[test]
    fn a_document_that_is_not_an_object_says_what_it_got() {
        let e = parse("[]").unwrap_err();
        assert!(e.contains("an array"), "{e}");
    }
}
