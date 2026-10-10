//! THE ROWS — WHAT THE DRIVER OBSERVED, AND THE ONLY THING THE JUDGE READS.
//!
//! The driver performs the platform calls and writes one of these; the judge is a pure function over it. That
//! is the whole split, and it is the one the design sweeps already made: their payload returns rows and
//! `summrise-sweep-judge` reads a report. It is also what makes the seven sections that no CI job can run
//! judgeable at all — a recording is a file, and a file can be carried to a box that has a toolchain.
//!
//! **AN OBSERVATION CARRIES WHAT WAS ASKED AND WHAT CAME BACK, AND NOTHING DERIVED.** `args` is the tool body
//! or the request body the driver sent; `result` is the answer the JS suite used to judge
//! (`j.result !== undefined ? j.result : j`); `error` is the transport failure that used to throw. No
//! condition is evaluated and no detail string is composed in the driver, so the two things a reader acts on
//! — whether the device passed, and the sentence saying why — are both computed here.
//!
//! **THE SERIES MATTER: A POLL IS NOT A SAMPLE.** Several checks are runs of the same question asked repeatedly
//! (the shell's first output, the embedded view after a transport switch, the SPA's address bar after AI
//! navigation), and the verdict is taken from the series — the first answer that satisfied the driver's stop
//! rule, or the last one if none did. So an observation is addressed by id and a poll by the id PREFIX it was
//! recorded under, in the order the document lists them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan::Plan;

/// The rows document. `schema` is checked on the way in: a judge that silently accepted a document it does not
/// understand would report a verdict about evidence it could not read.
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rows {
    pub schema: u32,
    /// The agent the driver talked to, as it was given it.
    pub base: String,
    /// **THE PLAN THE RUN ACTUALLY USED, ECHOED BACK.** The judge reads its fixtures from here rather than
    /// from its own defaults, so a recording is judged against the plan that produced it — and a corpus stays
    /// judgeable after the defaults move. `--expect` is what keeps this from being a way to pass by editing
    /// the recording: the caller declares the sections it asked for, and the judge refuses a mismatch.
    pub plan: Plan,
    pub observations: Vec<Observation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    /// Dotted and unique within a run: `terminal.open`, `mcp.stdio.view.3`. Polls share a prefix and end in
    /// the attempt number.
    pub id: String,
    /// What the driver ASKED, when the call takes arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    /// What came back. Absent when `error` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// The transport failure that used to `throw` out of the suite: a non-2xx, or an envelope whose `ok` was
    /// false. The section stopped there, and the judge names the failure instead of going silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Milliseconds since the driver started, so a failing series reads as a timeline rather than a list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

/// One observation as a check sees it. `Missing` is not an error: it means the driver never got that far,
/// which is what the suite's early returns used to express by not registering the check at all.
#[derive(Clone, Copy, Debug)]
pub enum Got<'a> {
    Missing,
    Failed(&'a str),
    Ok(&'a Value),
}

impl<'a> Got<'a> {
    pub fn ok(self) -> Option<&'a Value> {
        match self {
            Got::Ok(v) => Some(v),
            _ => None,
        }
    }

    pub fn failure(self) -> Option<&'a str> {
        match self {
            Got::Failed(e) => Some(e),
            _ => None,
        }
    }
}

/// THE ROWS, WITH THE LOOKUPS A CHECK NEEDS. Borrowed by every section's judge function — `View` rather
/// than `Rows` because the DOCUMENT is `Rows`: one is the file, the other is the reading of it.
pub struct View<'a> {
    pub doc: &'a Rows,
}

impl<'a> View<'a> {
    pub fn new(doc: &'a Rows) -> Self {
        Self { doc }
    }

    pub fn plan(&self) -> &'a Plan {
        &self.doc.plan
    }

    pub fn fixtures(&self) -> &'a crate::plan::Fixtures {
        &self.doc.plan.fixtures
    }

    pub fn budgets(&self) -> &'a crate::plan::Budgets {
        &self.doc.plan.budgets
    }

    pub fn find(&self, id: &str) -> Option<&'a Observation> {
        self.doc.observations.iter().find(|o| o.id == id)
    }

    pub fn got(&self, id: &str) -> Got<'a> {
        match self.find(id) {
            None => Got::Missing,
            Some(o) => match (&o.error, &o.result) {
                (Some(e), _) => Got::Failed(e),
                (None, Some(v)) => Got::Ok(v),
                // Neither: the driver recorded an observation with no answer at all, which is a driver
                // defect rather than a device one. Treated as a failure so it cannot read as a skip.
                (None, None) => Got::Failed("the driver recorded no answer and no error"),
            },
        }
    }

    /// The observations recorded under a prefix, in the document's order — the poll series. `id` itself is
    /// included, so a one-shot read and a series are addressed the same way.
    pub fn series(&self, prefix: &str) -> Vec<&'a Observation> {
        let dot = format!("{prefix}.");
        self.doc
            .observations
            .iter()
            .filter(|o| o.id == prefix || o.id.starts_with(&dot))
            .collect()
    }

    /// The values a series produced, dropping the ones that failed. Used where the check is about what the
    /// series eventually saw.
    pub fn series_values(&self, prefix: &str) -> Vec<&'a Value> {
        self.series(prefix)
            .into_iter()
            .filter_map(|o| o.result.as_ref())
            .collect()
    }

    /// Every id the run recorded, for the "the driver did not get here" message.
    pub fn ids(&self) -> Vec<&'a str> {
        self.doc
            .observations
            .iter()
            .map(|o| o.id.as_str())
            .collect()
    }
}

impl Rows {
    pub fn parse(text: &str) -> Result<Rows, String> {
        let doc: Rows = serde_json::from_str(text)
            .map_err(|e| format!("the rows document did not parse: {e}"))?;
        if doc.schema != SCHEMA {
            return Err(format!(
                "the rows document says schema {}, and this judge reads schema {SCHEMA} — \
                 a judge that read a document it does not understand would report a verdict about \
                 evidence it could not parse",
                doc.schema
            ));
        }
        Ok(doc)
    }
}

// ── the accessors a check uses, each one named after the JavaScript it replaces ─────────────────────────
//
// THE JS HAD A DIFFERENT SHAPE FOR EVERY ANSWER, and the checks read each one inline: `typeof x === 'string'
// ? x : (x.text || '')`, `Array.isArray(x) ? x : (x && x.sessions) || []`, `x && x.size`. Ported as inline
// Rust they would be nine spellings of one rule per file. They are here, once, with the JS expression they
// replace written on each — which is also how the next reader checks the port rather than trusting it.

/// `typeof v === 'string' ? v : (v.text || '')` — the shape `terminal_read` answers in.
pub fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

/// `v && v.screen` — the shape `terminal_screen`'s tail view answers in.
pub fn screen_of(v: &Value) -> String {
    v.get("screen")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// `Array.isArray(v) ? v : (v && v.sessions) || []` — the two shapes `terminal_list` answers in.
pub fn list_of(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        other => other
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    }
}

/// `.find(s => s && s.id === id)` over a list, in whichever shape it arrived.
pub fn row_of(v: &Value, id: &str) -> Option<Value> {
    list_of(v)
        .into_iter()
        .find(|s| s.get("id").and_then(Value::as_str) == Some(id))
}

/// `(v && v.entries) || []` — the shape `system_file_list` answers in.
pub fn rows_list_entries(v: &Value) -> Vec<Value> {
    v.get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `(v && v.results) || []` — the shape the memory tools answer in.
pub fn results_of(v: &Value) -> Vec<Value> {
    v.get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// `typeof x === 'number'`.
pub fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// A field of an object, or `Value::Null`.
pub fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}

/// `String(v.f)` where a MISSING property is the string `undefined` and a present `null` is `"null"`. The
/// difference shows up in the detail strings (`state=undefined` vs `state=null`) and those are the sentences a
/// reader triages a failure from, so the port keeps it.
pub fn js_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(x) => crate::text::js_string(x),
        None => "undefined".to_string(),
    }
}

/// `String(v && v.f)` — the object itself may be absent, which JavaScript also renders `undefined`.
pub fn js_get(v: Option<&Value>, key: &str) -> String {
    match v {
        Some(v) => js_field(v, key),
        None => "undefined".to_string(),
    }
}

/// JavaScript truthiness, which the suite leaned on in the places it wrote `x && x.field` as a condition: `0`,
/// `""`, `null` and a missing field are all false, and an empty array or object is TRUE.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// The last answer a poll series produced — which is what the JS held in its accumulator after the loop,
/// whether or not it broke early.
pub fn last_of<'a>(series: &[&'a Value]) -> Option<&'a Value> {
    series.last().copied()
}

/// `JSON.stringify(x)` where `x` may be ABSENT, which in JavaScript renders as no detail line at all rather
/// than as the string `"undefined"`. A present `null` is the string `"null"`.
pub fn js_json_or_empty(v: Option<&Value>) -> String {
    match v {
        Some(v) => crate::text::js_json(v),
        None => String::new(),
    }
}

/// **THE RULE THE OUTCOME CHECKS ARE ABOUT, PORTED VERBATIM** (round 96): do the caller's answer and the
/// session row AGREE about the last command, in BOTH directions?
///
/// The caller's `exit_code` and the row's `last_exit_code` are the same fact told to two audiences. What must
/// never happen is one reporting a code the other never saw. When the caller got NO code — a bash PTY in CI
/// fires no shell marker, so a `done` read is `partial` with a null code — the row must carry none either,
/// because absent means "no code was observed" rather than zero.
pub fn same_outcome(row: &Value, caller: &Value) -> bool {
    let code = field(caller, "exit_code");
    let on_row = field(row, "last_exit_code");
    match code.as_f64() {
        Some(c) => on_row.as_f64() == Some(c),
        // `typeof onRow !== 'number'` — null, absent and a string all count as "no number".
        None => on_row.as_f64().is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(observations: Vec<Observation>) -> Rows {
        Rows {
            schema: SCHEMA,
            base: "http://127.0.0.1:18080".to_string(),
            plan: Plan::default(),
            observations,
        }
    }

    fn obs(id: &str, result: Value) -> Observation {
        Observation {
            id: id.to_string(),
            args: None,
            result: Some(result),
            error: None,
            at_ms: None,
        }
    }

    #[test]
    fn a_series_is_the_document_order_and_excludes_a_neighbouring_prefix() {
        let d = doc(vec![
            obs("panel.xterm.0", json!("a")),
            obs("panel.xterm.1", json!("b")),
            obs("panel.xtermish", json!("no")),
            obs("panel.tab.click", json!("clicked")),
        ]);
        let r = View::new(&d);
        let got: Vec<&Value> = r.series_values("panel.xterm");
        assert_eq!(got, vec![&json!("a"), &json!("b")]);
    }

    #[test]
    fn a_recorded_failure_is_not_a_missing_observation() {
        let d = doc(vec![Observation {
            id: "terminal.open".to_string(),
            args: None,
            result: None,
            error: Some("tool terminal_open HTTP 500".to_string()),
            at_ms: None,
        }]);
        let r = View::new(&d);
        assert_eq!(
            r.got("terminal.open").failure(),
            Some("tool terminal_open HTTP 500")
        );
        assert!(matches!(r.got("terminal.never"), Got::Missing));
    }

    #[test]
    fn a_rows_document_from_another_schema_is_refused() {
        let mut text = serde_json::to_string(&doc(vec![])).unwrap();
        text = text.replace("\"schema\":1", "\"schema\":2");
        let e = Rows::parse(&text).unwrap_err();
        assert!(e.contains("schema 2"), "{e}");
    }

    #[test]
    fn same_outcome_agrees_in_both_directions() {
        // both have the same number
        assert!(same_outcome(
            &json!({"last_exit_code": 1}),
            &json!({"exit_code": 1})
        ));
        // one has a number the other does not
        assert!(!same_outcome(
            &json!({"last_exit_code": 0}),
            &json!({"exit_code": 1})
        ));
        // the caller got no number and the row carries none either — an AGREEMENT
        assert!(same_outcome(
            &json!({"last_exit_code": null}),
            &json!({"exit_code": null})
        ));
        assert!(same_outcome(&json!({}), &json!({"exit_code": null})));
        // the caller got no number but the row claims one — a DISAGREEMENT
        assert!(!same_outcome(
            &json!({"last_exit_code": 0}),
            &json!({"exit_code": null})
        ));
    }
}
