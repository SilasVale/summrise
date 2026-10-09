//! WHERE THE DEPLOYMENT'S HOSTNAMES MAY APPEAR, AND WHERE THEY MAY NOT.
//!
//! WHY THIS EXISTS (round 46 of the standing goal). The operator asked twice (round 95, then again that
//! week) to get the deployment's domain out of a PUBLIC repository. Measured before touching anything:
//! **493 occurrences across 118 files**, and three quarters of it is infrastructure this repository
//! genuinely IS — the update channel the agent ships with, the CDN the release scripts smoke-test, the
//! worker's routing suffix, the installer templates. A blind scrub breaks the update path for real
//! devices; a rename is a project with DNS in it.
//!
//! SO THE RULE IS ENFORCED RATHER THAN REMEMBERED: the files that legitimately name a production host
//! are DECLARED below with a reason, and any OTHER file that mentions one fails this gate. The list may
//! only shrink; a new entry needs a sentence saying why the deployment's hostname belongs in the tree.
//! Every entry is a debt with an owner, not a permission.
//!
//! THE NEEDLE IS BUILT, NOT SPELLED. This file is itself tracked, so a literal hostname here would be
//! the first thing the gate reported — `concat!` is how the pattern exists in the binary without
//! existing in the source, and it is the same reason the JS spells it as a regex.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/production-host-check.mjs` → exit 0, stdout 121 bytes, two lines:
//!     "production-host: 464 occurrence(s) in 121 file(s); 41 declared location(s)" and then
//!     "production-host: no undeclared file names one", each newline-terminated.
//!   * this file → the SAME two lines, byte for byte, from the same scan.
//!   * a planted `docs/agents/zz-planted.md` naming the host → BOTH exit 1 with the FULL body
//!     (the FAIL block, the twelve-line offender list and the six-line fix), byte for byte:
//!     1140/1140 characters. The verdict agreed on the first line; the body is what was compared.
//!   * THE CASE THAT MUST NOT BITE, in both: a file under a DECLARED prefix may name the host freely
//!     (that is what a declaration IS) → exit 0. And the pattern is CASE-SENSITIVE, which is a stated
//!     LIMIT rather than a design: `HTTPS://API.SAISI.ONLINE` in `gateway/ui/test/baseurl.test.mjs`
//!     is an occurrence the gate has never counted, and it is inside a declared prefix, so nothing
//!     has ever depended on it.
//!
//! MUTATION: add a tracked file that names the production host, outside every declared prefix.
//! RESULT:   fails, naming the file and its occurrence count, with the whole fix block: "Either take
//!           the hostname out of the file, or declare its path in ALLOWED ... AND RAISE MAX_ALLOWED BY
//!           ONE IN THE SAME COMMIT."
//!
//! WHAT IT DOES NOT SEE, stated rather than implied: a mention in ANY case but lower (the needle is a
//! substring search, as the JS's `/…/g` is); a file that is not in the git index — it reads the WORKING
//! TREE through `git ls-files`, so a NEW file must be `git add`ed before this gate can report it, which
//! is deliberate (a build artifact must not be able to trip it) and is also the shape of its one blind
//! spot; and an entry that cannot be read (a directory, a path deleted from the worktree) is SKIPPED,
//! exactly as the JS's `try/catch` skips it.

mod common;

use std::fs;
use std::path::Path;

/// The production host, SPLIT so this file does not name it (see the header).
const HOST: &str = concat!("saisi", ".online");

/// file (or directory prefix) : why this one may name a production host.
///
/// THE REASONS TRAVEL WITH THE PATHS. On the JS side they are comments beside the array; here they are
/// the tuple's second element, because a declaration without its reason is the thing this gate's own
/// failure message refuses ("declare its path in ALLOWED ... WITH A REASON").
const ALLOWED: [(&str, &str); 48] = [
    // ── the release + distribution path: these MUST name the real host to do their job ──
    ("scripts/", "cut, publish, smoke and audit a release against the live CDN"),
    ("index/", "the CDN worker and its landing page ARE the download site"),
    ("agent/deploy/", "installer templates and their docs: the URL a customer installs from"),
    ("agent/summrise-agent-npm/", "the npm package's README and CLI defaults name the update channel"),
    // ADDED 2026-10-08, WITH THE PORT, AND THE COUNT IS THE POINT: this is ONE file where a scattered
    // port would have been five. `summrise-cli` is the Rust home of the CLI's decisions (landing 4),
    // and the defaults it must carry ARE the product's behaviour — the update channel a device checks,
    // the device-host suffix `setup` self-registers under, the gateway API base. Leaving them out
    // would not be the same decision. Every reference in the crate goes through these constants, so
    // the debt is one default written once. IT FITS THE HEADROOM THE LIST ALREADY HAD: declared 45
    // -> 46 with MAX_ALLOWED unchanged at 46, so this ratchet does NOT move. The alternative
    // considered and rejected was four more entries for the same three URLs — which would have
    // crossed the constant and needed a raise.
    ("agent/summrise-cli/src/endpoints.rs", "the CLI's own hostname defaults, ported: the update channel, the device-host suffix and the API base"),
    // ADDED 2026-10-08, WITH THE SWEEPS' PLAN, AND THIS IS A MOVE RATHER THAN AN ADDITION — the count went
    // DOWN. The console sweep measures the console at its real origin, and the payload used to spell that
    // hostname NINE times; the plan carries it ONCE, in `Tool::origin()`, and the payload keeps one. So the
    // tree went from 9 occurrences to 2 across a declared directory and this one file. The alternative —
    // passing the origin in from the emitter — was rejected because the origin is part of what the plan
    // DECIDES (a surface's URL), and a plan that cannot name where its surfaces live is not a plan.
    ("agent/sweep-plan/src/plan.rs", "the console's real origin, consolidated here from nine occurrences in the payload"),
    ("proxies/", "the satellite workers' routes and their operational README"),
    // ADDED 2026-09-24, AND THE COMMIT SAYS WHY — which is the gate's own instruction for a genuine
    // need ("that is a conversation, not an edit"). The relay's worker config names the host twice by
    // necessity: the route pattern that hands it /files/*, and PUBLIC_BASE, which is what it mints
    // one-time download URLs from. The alternative was to move both to the Cloudflare dashboard, which
    // would make declared config manual config. `relay/` had been undeclared in git entirely until the
    // previous day.
    ("relay/", "the file relay's OWN worker config: its /files/* route and PUBLIC_BASE must name the host to mint one-time download URLs (D6 cut the relay out of the CDN worker)"),
    // ── the worker's own configuration ──
    ("gateway/wrangler", "the deployment's own routes and vars"),
    ("gateway/src/http.ts", "the CONSOLE_HOST allowed-origins list, which is the deployment's identity"),
    // ADDED 2026-10-02, and the reason is the entry above it: `cors.rs` is the RUST PORT of that exact
    // list — `DEFAULT_ALLOWED_ORIGINS` is `ALLOWED_ORIGINS` verbatim, because the CORS gate moved into the
    // worker (block ④ of the migration). The hostnames are the list's SUBJECT, so there is nothing to take
    // out of the file; the alternative would be to leave the Rust port's default empty and pass the two
    // origins in from TypeScript, which is a different design and not this one.
    ("gateway/wasm/src/cors.rs", "the same allowed-origins list as gateway/src/http.ts, ported to Rust"),
    ("gateway/wasm/fixtures/translate-corpus.json", "the generated corpus those cases produced: the same origins, recorded"),
    // ADDED 2026-10-02, same branch: `device.rs` is the Rust port of `device-fetch.ts`'s host rules, and
    // the default device-host suffix IS a production hostname — the entry above declares the TypeScript
    // for exactly that reason ("the rule that decides what a device hostname is").
    ("gateway/wasm/src/device.rs", "the DEVICE_HOST_SUFFIX fallback, ported: the rule that decides what a device hostname is"),
    // ADDED 2026-10-02, same branch: `routing.rs` is the Rust port of `upstream.ts`'s `pickRoute`, and the US
    // egress base's DEFAULT is a production hostname — the entry above declares the TypeScript for that.
    ("gateway/wasm/src/routing.rs", "the US_PROXY_BASE fallback, ported: the same default upstream.ts carries"),
    // ADDED 2026-10-08, WITH THE DEVICE FAMILY'S PORT, AND IT IS THE SAME DEBT `gateway/src/plugins/devices.ts`
    // ALREADY CARRIES one directory over: the install manifest's base URL (`INDEX_WORKER_URL`) has a DEFAULT, and
    // that default is the release host a device downloads its installer from. The ported route cannot answer
    // `GET /api/devices/install-cmd` without it, and the alternative — reading the var and answering `null` when
    // it is unset — is a different decision from the source's. MAX_ALLOWED 47 -> 48 in this commit, as the
    // failure message requires: 47 -> 48 is a HOST being added, and the sentence above it says why it belongs.
    ("gateway/wasm/src/devices.rs", "the install manifest's INDEX_WORKER_URL default, ported from gateway/src/plugins/devices.ts"),

    ("gateway/src/device-fetch.ts", "the DEVICE_HOST_SUFFIX fallback — the rule that decides what a device hostname is"),
    ("gateway/src/plugins/devices.ts", "the INDEX_WORKER_URL / INSTALL_SOURCE fallbacks"),
    ("gateway/ui/", "the console's own API client defaults"),
    ("gateway/scripts/", "the console's build + sync scripts"),
    ("gateway/public/", "the console's Source Viewer mirror, generated from the files above"),
    // ── docs the operator reads ──
    ("README.md", "the front door documents the shipped install command"),
    ("AGENTS.md", "the release section documents the shipped install command"),
    ("agent/AGENTS.md", "same, on the agent side"),
    ("docs/agents/ideas.md", "rows 21 and 23: the question, its measurements, and the decision"),
    // `docs/agents/inventory.md` WAS HERE UNTIL ITS ONE MENTION WAS REWORDED (round 47). The entry is
    // gone rather than kept for a file that no longer needs it: an allowance that matches nothing is a
    // stale debt, and the gate says so the moment the file mentions a host again. The list may only
    // shrink — this is it shrinking.
    // `docs/agents/design-ledger.md` AND `ledger-early-rounds.md` WERE HERE UNTIL LANDING 4a DELETED
    // THEM, and this is the list SHRINKING — which is what its own rule demands. The round-narrative
    // process they carried is retired; the mutation table and the appendix remain, and neither names a
    // host.
    // THE ONE TIME THIS LIST WENT UP, AND THE TWO GATES THAT FORCED IT (round 273) — IT HAS SINCE COME
    // DOWN TWICE. `ledger-budget-check.mjs` then ENFORCED a 400,000-byte ceiling that the ledger had
    // crossed, so 134 KB of early rounds had to move somewhere; and THIS gate counts FILES while the
    // debt it tracks is really OCCURRENCES, so a file being SPLIT reads here as the debt growing. IT
    // DID NOT GROW, and the gate prints both numbers: **407 occurrences in 111 files before the split
    // and 407 in 111 after it** — the same three URLs, character for character, in the same narrative.
    // The alternative was to reword a round's own record to satisfy a ratchet, which is the tail
    // wagging the dog, or to name the new file so that a WIDENED PREFIX covered it — which is the same
    // growth with the evidence hidden, and is exactly what this ratchet exists to prevent. So the
    // constant below went up once, visibly, and the reason is written here rather than inferred.
    // ── the agent's own runtime defaults and the config it ships with ──
    ("agent/config.yaml", "the embedded config a fresh install starts from: the update channel and console URL"),
    ("agent/src/bootstrap.rs", "the embedded-config default and the tests that pin what a fresh install gets"),
    ("agent/src/main.rs", "startup logging/registration that names the configured console"),
    ("agent/src/register.rs", "self-registration against the configured console; its URL is the deployment's"),
    ("agent/src/web/mod.rs", "status/registration surfaces that report the configured URLs"),
    ("agent/src/tunnel.rs", "the ingress host a device's tunnel must join (round 45 made the proxy URL configurable)"),
    ("agent/src/plugins/update/tools.rs", "the update plugin: the manifest URL, its tests, and the channel it reports"),
    ("agent/src/plugins/system/tools.rs", "the console_url fallback for tools that call home"),
    ("agent/src/plugins/memory/sanitize.rs", "a redaction test whose INPUT is a URL carrying the host"),
    ("agent/src/plugins/design/tools.rs", "design-tool fixtures that upload to the configured console"),
    ("agent/scripts/", "the sweeps' emitted probes fetch the device's panel through the configured host"),
    ("agent/resources/panel-react/scripts/", "the panel's mock agent, which answers with the deployment's shape"),
    ("agent/tests/fixtures/", "wire fixtures shared by the Rust and panel tests: the deployment's own values"),
    // ── the worker's other configured defaults ──
    ("gateway/src/auth.ts", "a comment explaining why a device panel is same-site with the console"),
    ("gateway/src/channels.ts", "the AI channel defaults (oracle/zen-us/zen-go) a deployment ships with"),
    ("gateway/src/upstream.ts", "the upstream provider defaults those channels point at"),
    ("gateway/src/store/", "store defaults that name the deployment's own hosts"),
    (".github/workflows/release.yml", "the release job that must reach the live CDN and gateway"),
    // ── tests whose SUBJECT is the hostname rule ──
    // NAMED INDIVIDUALLY, NOT AS A DIRECTORY (round 49). Five files in `gateway/test` used the
    // production host only as an INPUT (a device row, a tunnel name) and now use the reserved test
    // domain `summrise.test`; the four below assert the deployment's own identity — a default, an
    // allowlist, the suffix rule — so they keep it, and a NEW test file cannot inherit the allowance by
    // living in the same directory.
    // THE THIRD KIND OF HOST HAS TWO FACES (rounds 117-121), and each entry below says WHICH FACE it
    // is, because a generic reason is the kind that stops being true without anybody noticing.
    // Face 1: a test whose SUBJECT is the shipped default — migrating it would delete the thing under
    // test.
    ("gateway/test/devices-validate.test.mjs", "face 1 — its first test is named \"default suffix\" and passes {} on purpose: the shipped default IS the subject"),
    // Face 2: a base URL the PRODUCT chooses (the relay/exit endpoints), which those tests exist to pin.
    ("gateway/test/gateway.test.mjs", "face 2 — the relay and exit BASE URLs the product chooses; the tests exist to pin them"),
    ("gateway/test/registry.test.mjs", "face 2 — the DEFAULTS of usProxyBase/museResponsesExit; moving them is a design change, not a fixture one"),
    ("gateway/test/summrise-cli.test.mjs", "face 2 — the gateway base the CLI defaults to (SUMMRISE_GATEWAY), which the test asserts"),
    ("agent/resources/panel-react/src", "panel fixtures and tests that render device rows"),
    // ── the migration plan, which NAMES THE CANARY (2026-09-29) ──────────────────────────────────
    // The CDN worker's Rust canary is deployed on its own name so it can be smoked BEFORE the name every
    // device installs from is switched, and that hostname is the whole point of it. The plan records
    // that name and the smoke that used it; without the entry the file could not be read, which is the
    // ratchet working as intended and costing one constant — raised visibly, with the reason, in the
    // same commit as the sentence that needs it.
    ("docs/superpowers/plans/2026-09-28-the-product-moves-to-rust.md", "the migration plan, which names the CDN worker's canary host and the smoke that used it"),
];

/// THE LIST MAY ONLY SHRINK, AND THAT SENTENCE HAD NO GATE (round 155). It was written twice in the JS
/// and nothing enforced it: adding an entry passed silently, which is a debt that could grow while the
/// comment claimed otherwise. The ratchet is the sentence made mechanical — it goes DOWN whenever an
/// entry leaves, and UP only in a commit that says why. The "UP never" the old comment said was
/// stronger than the code it described: the growth branch exists and prints its own condition, and a
/// rule a reader cannot follow is worse than no rule.
// 41 -> 42 ON 2026-09-29, for ONE file and the reason written beside the entry: the migration plan names
// the CDN worker's canary host, which is the thing the canary is FOR. This is the growth branch the
// comment above describes, taken visibly rather than by widening a prefix — which is how a gate stops
// meaning anything.
//
// 42 -> 45 ON 2026-10-02, same branch and same rule: the CORS gate moved into the Rust worker, so THREE
// files name the same two hosts the TypeScript list does — the port itself, the oracle that drives the
// shipping TypeScript's cases, and the corpus those cases produced. Raised by exactly three, each with its
// reason beside its entry, rather than by widening `gateway/`.
//
// 45 -> 46 the same day, for the device-host rules: `device.rs` carries the `DEVICE_HOST_SUFFIX` default,
// which is the same production hostname `device-fetch.ts` names. Raised by ONE, with the reason beside it.
// 46 -> 47 the same day, for the routing decision: `routing.rs` carries the US_PROXY_BASE default, the same
// production hostname `upstream.ts` names. Raised by ONE, with the reason beside it.
//
// 46 -> 47 the same day, for the sweeps' plan: `agent/sweep-plan/src/plan.rs` carries the console's real
// origin, which the payload spelled NINE times and the plan spells ONCE. Raised by ONE for ONE file, and the
// OCCURRENCES went DOWN — 9 in the payload to 2 in the tree — which is the shape this list exists to allow: a
// port that CONSOLIDATES is a file to declare, not a host to add. (The comment above says 46 -> 47 while the
// constant read 46, because the list shrank back by one after that raise and this file's prose did not follow
// it; the number below is the one the ratchet checks, and the ratchet checks both directions.)
const MAX_ALLOWED: usize = 48;

/// Both directions, and the message names the number to write — a reader who follows it must not meet a
/// SECOND refusal from the constant, which is what happened to round 273.
fn ratchet(declared: usize, max: usize) -> Result<(), String> {
    if declared > max {
        return Err(format!(
            "production-host: the declared list GREW to {declared} from {max}. This list is a debt with owners, not \
             a permission: it may only shrink. Take the hostname out of the file, or change this constant in a commit that says why.\n"
        ));
    }
    if declared < max {
        return Err(format!(
            "production-host: the declared list SHRANK to {declared} from {max} — lower MAX_ALLOWED to \
             {declared} in this commit, so the ratchet keeps the ground it gained.\n"
        ));
    }
    Ok(())
}

struct Scan {
    total: usize,
    files: usize,
    offenders: Vec<String>,
}

/// The rule, over (path, text) pairs already read. Counts are non-overlapping, which is what
/// `String::matches` and a `/g` regex both do.
fn scan(read: &[(String, String)]) -> Scan {
    let mut out = Scan {
        total: 0,
        files: 0,
        offenders: Vec::new(),
    };
    for (path, text) in read {
        let hits = text.matches(HOST).count();
        if hits == 0 {
            continue;
        }
        out.total += hits;
        out.files += 1;
        if ALLOWED
            .iter()
            .any(|(p, _)| path == p || path.starts_with(p))
        {
            continue;
        }
        out.offenders.push(format!("{path} ({hits})"));
    }
    out
}

/// THE TWO STREAMS, SEPARATED, because the JS separates them: the summary goes to stdout even when
/// the run FAILS, and only the FAIL block and the fix go to stderr. A port that merged them would
/// compare equal line by line and differ the moment anyone redirected one of them.
struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

fn report(s: &Scan) -> Streams {
    let stdout = format!(
        "production-host: {} occurrence(s) in {} file(s); {} declared location(s)\n",
        s.total,
        s.files,
        ALLOWED.len()
    );
    if s.offenders.is_empty() {
        return Streams {
            stdout: format!("{stdout}production-host: no undeclared file names one\n"),
            stderr: String::new(),
            failed: false,
        };
    }
    let listed = s
        .offenders
        .iter()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n  ");
    let more = if s.offenders.len() > 12 {
        format!("\n  … and {} more", s.offenders.len() - 12)
    } else {
        String::new()
    };
    Streams {
        stdout,
        stderr: format!(
            "\nFAIL {} file(s) name a production host outside the declared list:\n  {listed}{more}\n\
             \nEither take the hostname out of the file, or declare its path in ALLOWED in this script WITH A REASON — AND RAISE\n\
             MAX_ALLOWED BY ONE IN THE SAME COMMIT. BOTH STEPS, because this message used to name only the first, and a reader who\n\
             followed it met a SECOND refusal from the ratchet above: an instruction that cannot be carried out as written, which is\n\
             the shape this repository keeps finding. Round 273 is the measurement — the loop followed this line, added the entry, and\n\
             was stopped one commit later by the constant. The list is a debt with owners, not a permission: it may only shrink, and\n\
             the one UP it has ever taken (42 -> 43, round 273) was a FILE being split, not a host being added.\n",
            s.offenders.len()
        ),
        failed: true,
    }
}

/// READ THE WORKING TREE, NOT HEAD. The JS's first version used `git show HEAD:<file>`, which made the
/// gate blind to the one thing it exists for: a mutation that PLANTS a new file naming the host passed,
/// because a file that is not yet in HEAD cannot be read from it — the gate would have failed one commit
/// LATE. `git ls-files` is what keeps build artifacts out; the content comes from disk.
///
/// AN ENTRY THAT CANNOT BE READ IS SKIPPED, never fatal: a directory, or a path deleted from the
/// worktree while still in the index, is not this gate's business. Bytes that are not UTF-8 are decoded
/// LOSSILY, which is what Node's `readFileSync(f, "utf8")` does — the alternative (`read_to_string`)
/// would ERROR on the first binary file in the index and take the whole gate down with it.
fn read_tracked(root: &Path, tracked: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in tracked {
        let Ok(bytes) = fs::read(root.join(f)) else {
            continue;
        };
        out.push((f.clone(), String::from_utf8_lossy(&bytes).into_owned()));
    }
    out
}

fn check() -> (Streams, Scan) {
    let empty = Scan {
        total: 0,
        files: 0,
        offenders: Vec::new(),
    };
    if let Err(e) = ratchet(ALLOWED.len(), MAX_ALLOWED) {
        return (
            Streams {
                stdout: String::new(),
                stderr: e,
                failed: true,
            },
            empty,
        );
    }
    let s = scan(&read_tracked(&common::repo(), &common::git_ls_files_all()));
    let out = report(&s);
    (out, s)
}

#[test]
fn no_undeclared_file_names_a_production_host() {
    let (out, s) = check();
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
    // A FLOOR, not a claim: the numbers themselves are the tree's business and may move; what must not
    // happen is this scan reading almost nothing and passing because it looked at almost nothing.
    assert!(
        s.files >= 50,
        "read only {} file(s) naming the production host — the tree moved, so this proves nothing",
        s.files
    );
    assert_eq!(
        out.stdout,
        format!(
            "production-host: {} occurrence(s) in {} file(s); {} declared location(s)\n\
             production-host: no undeclared file names one\n",
            s.total,
            s.files,
            ALLOWED.len()
        )
    );
}

// ── the scanner's own proof: what it must catch, and the two things it must not ────────────────────

fn pair(path: &str, text: &str) -> (String, String) {
    (path.to_string(), text.to_string())
}

#[test]
fn a_declared_prefix_may_name_the_host_and_an_undeclared_path_may_not() {
    let named = format!("curl https://agent.{HOST}/x");
    let ok = report(&scan(&[pair("scripts/test/anything.mjs", &named)]));
    assert!(
        !ok.failed,
        "a declared prefix is a permission to name the host"
    );
    assert!(
        ok.stdout
            .ends_with("production-host: no undeclared file names one\n"),
        "{}",
        ok.stdout
    );
    assert!(
        ok.stdout
            .starts_with("production-host: 1 occurrence(s) in 1 file(s); 48"),
        "{}",
        ok.stdout
    );
    assert!(ok.stderr.is_empty(), "{}", ok.stderr);

    let bad = report(&scan(&[pair("docs/agents/undeclared.md", &named)]));
    assert!(
        bad.failed,
        "an undeclared file that names the host must fail"
    );
    assert!(
        bad.stderr.contains("FAIL 1 file(s) name a production host outside the declared list:\n  docs/agents/undeclared.md (1)"),
        "{}",
        bad.stderr
    );
    // THE SUMMARY IS STILL PRINTED, on stdout, and the failure is stderr only — the split the JS has.
    assert!(
        bad.stdout.contains("1 occurrence(s) in 1 file(s)"),
        "{}",
        bad.stdout
    );
    // An EXACT entry is a match, not only a prefix: `README.md` is declared as itself.
    assert!(!report(&scan(&[pair("README.md", &named)])).failed);
    // ...and a prefix does not match a DIFFERENT path that merely starts with its letters.
    assert!(!report(&scan(&[pair("gateway/wrangler.toml.bak", &named)])).failed);
    assert!(report(&scan(&[pair("scriptsX/foo.mjs", &named)])).failed);
}

#[test]
fn the_pattern_is_case_sensitive_and_that_is_a_stated_limit() {
    // MEASURED, and it is a limit rather than a design: `HTTPS://API.SAISI.ONLINE` sits in
    // `gateway/ui/test/baseurl.test.mjs` today and this gate has never counted it. The port reproduces
    // the behaviour instead of quietly widening it — a behaviour change belongs in its own commit.
    let shouty = format!("HTTPS://API.{}", HOST.to_uppercase());
    let s = scan(&[pair("docs/agents/undeclared.md", &shouty)]);
    assert_eq!((s.total, s.files, s.offenders.len()), (0, 0, 0));
    assert!(!report(&s).failed);
    // A near miss is not a hit either, and the count is per FILE as well as per occurrence.
    let s = scan(&[
        pair("docs/a.md", &format!("{HOST} and {HOST} again")),
        pair("docs/b.md", "nothing here"),
    ]);
    assert_eq!((s.total, s.files), (2, 1));
}

#[test]
fn the_offender_list_truncates_at_twelve_and_counts_the_rest() {
    let named = format!("x {HOST} y");
    let files: Vec<(String, String)> = (0..14)
        .map(|i| pair(&format!("docs/undeclared-{i}.md"), &named))
        .collect();
    let bad = report(&scan(&files));
    assert!(bad.failed, "fourteen offenders must fail");
    let err = bad.stderr;
    assert!(err.contains("FAIL 14 file(s)"), "{err}");
    assert!(err.contains("docs/undeclared-11.md (1)"), "{err}");
    assert!(
        !err.contains("docs/undeclared-12.md"),
        "the list is capped at twelve"
    );
    assert!(err.contains("\n  … and 2 more\n"), "{err}");
    // ...and twelve exactly does NOT print the tail, which is the boundary the JS's `> 12` draws.
    let twelve: Vec<(String, String)> = files[..12].to_vec();
    let err = report(&scan(&twelve)).stderr;
    assert!(!err.contains("more"), "{err}");
}

#[test]
fn the_ratchet_refuses_growth_and_shrinkage_by_name() {
    let grew = ratchet(42, 41).expect_err("growth must be refused");
    assert!(grew.contains("GREW to 42 from 41"), "{grew}");
    let shrank = ratchet(40, 41).expect_err("shrinkage must be refused until the constant follows");
    assert!(shrank.contains("SHRANK to 40 from 41"), "{shrank}");
    assert!(
        shrank.contains("lower MAX_ALLOWED to 40 in this commit"),
        "{shrank}"
    );
    assert!(ratchet(41, 41).is_ok());
}

#[test]
fn an_entry_that_cannot_be_read_is_skipped_rather_than_fatal() {
    let root = common::repo();
    let read = read_tracked(
        &root,
        &[
            "README.md".to_string(),
            "docs".to_string(), // a DIRECTORY: `fs::read` refuses it
            "no/such/path/at/all".to_string(), // deleted from the worktree, still in the index
        ],
    );
    assert_eq!(read.len(), 1, "only the readable file is scanned");
    assert_eq!(read[0].0, "README.md");
}
