//! The plan as DATA (serde), and the two `wants()` semantics the sweeps actually have.
//!
//! The two are not the same and the difference is preserved deliberately — it is what the JavaScript
//! does, and the parity harness compares against the JavaScript rather than against an opinion about
//! it. See `report_notes()` below, and the crate's own `notes` field, which names the divergences so a
//! later round can decide whether they are deliberate.

use serde::Serialize;
use serde_json::{Map, Value};

/// Which sweep a plan is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Panel,
    Console,
    Landing,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Panel => "panel",
            Tool::Console => "console",
            Tool::Landing => "landing",
        }
    }

    pub fn parse(s: &str) -> Option<Tool> {
        match s {
            "panel" => Some(Tool::Panel),
            "console" => Some(Tool::Console),
            "landing" => Some(Tool::Landing),
            _ => None,
        }
    }

    /// EVERY PASS NAME THE TOOL ASKS ABOUT, in the order the payload's code mentions them. This is the
    /// vocabulary `wants()` is evaluated over, and it is what the parity harness compares.
    pub fn vocabulary(self) -> &'static [&'static str] {
        match self {
            // `needsPage` first (the matrix's own gate), then the axes in the order the payload runs
            // them, then the four single-pass blocks after the body.
            Tool::Panel => &[
                "pages", "focus", "timing", "hover", "press", "idle", "targets", "ack", "unstyled",
                "motion", "reflow",
            ],
            // The console's `wants()` names, in the order the payload gates them.
            Tool::Console => &[
                "dark", "reflow", "contrast", "names", "unstyled", "focus", "press", "ack", "idle",
                "motion", "targets",
            ],
            Tool::Landing => &[
                "contrast", "names", "focus", "idle", "unstyled", "targets", "press", "reflow",
                "motion",
            ],
        }
    }

    /// WHERE THE PAGES ARE SERVED FROM. The panel and the landing are served by routes inside the
    /// payload (`http://summrise.test`); the console is measured at its real origin.
    pub fn origin(self) -> &'static str {
        match self {
            Tool::Panel | Tool::Landing => "http://summrise.test",
            Tool::Console => "https://ai.saisi.online",
        }
    }

    /// THE PANEL'S `wants()` IS A STRING TEST; THE OTHER TWO ARE LIST TESTS, and they disagree in two
    /// ways that are visible from a caller:
    ///
    /// | input | panel | console / landing |
    /// |---|---|---|
    /// | absent | `"all"` → everything | empty list → everything |
    /// | `--passes=` | `""` → **nothing** | empty list → **everything** |
    /// | `--passes=a, b` | trims each entry → `b` wanted | does NOT trim → `" b"` is not `b` |
    ///
    /// Both are reproduced rather than unified: this landing moves the decision, it does not change
    /// it. The divergences are reported in `notes` so that a later round can decide.
    pub fn wants(self, spec: &PassSpec, name: &str) -> bool {
        match (self, spec) {
            (Tool::Panel, PassSpec::Raw(s)) => {
                s == "all" || s.split(',').map(str::trim).any(|p| p == name)
            }
            (_, PassSpec::List(list)) => {
                list.is_empty() || list.iter().any(|p| p == "all") || list.iter().any(|p| p == name)
            }
            // The panel never carries a list and the other two never carry a raw string; a mismatch is a
            // programming error rather than an input, so it is loud.
            _ => unreachable!("tool and pass-spec shape disagree"),
        }
    }
}

/// WHAT THE `--passes` FLAG BECAME. The panel's emitter keeps the raw string and writes it into the
/// report (`report.passes`); the console's and the landing's split it into a list and filter empties.
#[derive(Clone, Debug, PartialEq)]
pub enum PassSpec {
    Raw(String),
    List(Vec<String>),
}

impl PassSpec {
    /// The value the tool's `pieces.config.passes` carries — a string for the panel, a list for the
    /// console and the landing. This is what the report records, so it is what the plan publishes.
    pub fn to_json(&self) -> Value {
        match self {
            PassSpec::Raw(s) => Value::String(s.clone()),
            PassSpec::List(l) => Value::Array(l.iter().cloned().map(Value::String).collect()),
        }
    }
}

/// A viewport, in the shape the payloads pass to `page.setViewportSize`.
#[derive(Clone, Copy, Serialize, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

/// ONE SURFACE THE RUN VISITS, with the passes that run on it.
///
/// The fields are the ones a payload needs to render the visit and to label its rows: where to go
/// (`origin` + `path` + `query`, or `hash` for the console's SPA), how big the window is, which theme
/// and mode the surface is, what the report calls it, and which axes measure it.
#[derive(Clone, Serialize, Debug)]
pub struct Surface {
    /// Stable and unique within a plan: `<kind>.<density>.<theme>.<mode>.<discriminator>`.
    pub id: String,
    /// The block of the payload this surface belongs to. The payload looks its surfaces up by kind.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// ORDERED query pairs. Order is preserved because the emitted URL is what the payload navigates
    /// to, and the parity harness compares it character for character.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<Vec<(String, String)>>,
    pub viewport: Viewport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The label the report's rows carry, when it is fixed. `None` means the payload derives it from
    /// what it measured (the rail walk names a surface after the page the app reports active).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The console drives its SPA by hash AFTER the navigation, not by URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// `page.emulateMedia(...)` for the surfaces that need it (the landing's colour scheme, the motion
    /// pass's reduced-motion emulation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Value>,
    /// Set when the surface EXPANDS at run time into one surface per thing the page reports — the rail
    /// walk, whose page names come from the DOM. A named exception rather than a silent gap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<&'static str>,
    /// `page.reload()` after the navigation. Every panel surface loads, sets a flag, and reloads; the
    /// console and the landing do not.
    pub reload: bool,
    /// Whether this surface NAVIGATES at all. One exception, and it is real: the panel's motion pass
    /// loads the page once and then RELOADS it with the preference emulated, because the preference
    /// only takes effect on a fresh style resolution. Always serialized (see `set_viewport`).
    pub navigate: bool,
    /// Steps the payload performs ONCE, immediately before this surface. The panel's
    /// `emulateMedia({reducedMotion: null})` runs after the motion block whether or not that pass was
    /// wanted, and BEFORE the reflow block — so it belongs to whatever surface comes next, and to the
    /// plan's tail when nothing does.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pre: Vec<Value>,
    /// Whether the payload RESIZES before this surface. It is not always true and the exceptions are
    /// real: the console and the landing set the viewport once per WIDTH and then walk the pages at
    /// it, and the landing's motion pass sets none at all — so it runs at whatever the previous block
    /// left behind. Transcribed rather than tidied, because the report has to match byte for byte.
    ///
    /// ALWAYS SERIALIZED, like `navigate` and `reload`: a field the payload reads with a default is a
    /// decision the JavaScript is still making, and the point of this crate is that it makes none.
    pub set_viewport: bool,
    /// Steps the payload performs ONCE, right after this surface's measurement. One vocabulary for
    /// now: `{"emulateMedia": {...}}`. The panel's motion block resets the reduced-motion emulation
    /// after its loop and BEFORE the reflow block, which is why this is a step on a surface rather
    /// than a tail on the plan.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub post: Vec<Value>,
    /// VALUES THE BLOCK'S OWN MEASUREMENT NEEDS, which the plan therefore owns: the rail button a
    /// fixture surface lands on, the direction a monitor event is dispatched with, which record tab a
    /// surface shows and whether its trail is trimmed. They live here rather than being re-derived in
    /// the payload from an id, a mode name or a query string — parsing a URL back into the fact it was
    /// built from is how two copies of one fact start to disagree.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub vars: Map<String, Value>,
    /// The axes that measure at this surface.
    pub passes: Vec<&'static str>,
}

/// A query string, from ordered pairs.
pub fn q(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub fn vp(width: u32, height: u32) -> Viewport {
    Viewport { width, height }
}

/// THE PLAN. Serialized straight to stdout by the binary and embedded verbatim in the emitted bundle.
#[derive(Serialize, Debug)]
pub struct Plan {
    pub tool: &'static str,
    /// The value the report records for `passes` — a string for the panel, a list for the other two.
    pub passes: Value,
    /// Every pass name this tool asks about, in payload order.
    pub vocab: Vec<&'static str>,
    /// The wanted ones, in the same order.
    pub wanted: Vec<&'static str>,
    /// `name -> bool`, so the payload's `wants(name)` is a LOOKUP rather than a second computation.
    pub wants: Map<String, Value>,
    pub origin: &'static str,
    /// Policy numbers and lists: caps, floors and the curated target lists. Anything here is a decision
    /// about HOW MUCH to measure, not a measurement.
    pub caps: Value,
    /// The surfaces, IN THE ORDER THE RUN VISITS THEM.
    pub surfaces: Vec<Surface>,
    /// Steps performed AFTER the last surface. Only reachable when a step's own block ran with no
    /// surface at all — `--passes=` on the panel wants nothing, and the reset still executes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub post: Vec<Value>,
    /// Known divergences between the three implementations, so a later round can decide rather than
    /// rediscover. These are REPORTED, never smoothed over.
    pub notes: Vec<String>,
}

impl Plan {
    pub fn new(tool: Tool, spec: PassSpec) -> Plan {
        let vocab = tool.vocabulary().to_vec();
        let mut wants = Map::new();
        let mut wanted = Vec::new();
        for name in &vocab {
            let w = tool.wants(&spec, name);
            wants.insert((*name).to_string(), Value::Bool(w));
            if w {
                wanted.push(*name);
            }
        }
        Plan {
            tool: tool.name(),
            passes: spec.to_json(),
            vocab,
            wanted,
            wants,
            origin: tool.origin(),
            caps: Value::Null,
            surfaces: Vec::new(),
            post: Vec::new(),
            notes: notes(tool, &spec),
        }
    }

    /// Is this pass wanted? The one place a payload's `wants(name)` reads.
    pub fn wants(&self, name: &str) -> bool {
        self.wants
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn surfaces_of(&self, kind: &str) -> Vec<&Surface> {
        self.surfaces.iter().filter(|s| s.kind == kind).collect()
    }

    pub fn push(&mut self, s: Surface) {
        self.surfaces.push(s);
    }
}

/// THE DIVERGENCES, as data rather than as a paragraph in a commit message.
pub fn notes(tool: Tool, spec: &PassSpec) -> Vec<String> {
    let mut out = Vec::new();
    if tool == Tool::Panel {
        out.push(
            "the panel's wants() reads the RAW --passes string and trims each entry; the console's and \
             the landing's split it into a list and do not trim. `--passes=a, b` therefore wants only \
             `a` on the console. Reproduced, not fixed."
                .to_string(),
        );
        out.push(
            "an EMPTY --passes= wants NOTHING on the panel and EVERYTHING on the console and the \
             landing, because the panel tests the string (`\"\"` is not `all`) while the other two \
             test a list (`!PASSES.length` is true). Reproduced, not fixed."
                .to_string(),
        );
    }
    if let PassSpec::Raw(s) = spec {
        if s.is_empty() {
            out.push("--passes= was given EMPTY: this run wants no pass at all".to_string());
        }
    }
    out
}
