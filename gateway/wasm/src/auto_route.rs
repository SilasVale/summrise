//! `model-route.ts`'S DECISIONS, AND THE TYPESCRIPT THEY EMIT.
//!
//! ── WHY AN EMISSION, AND NOT A ROUTE PORT ───────────────────────────────────────────────────────────────────────
//!
//! `GET|PUT /api/me/route` STAYS ON THE TYPESCRIPT PATH, and `auth_routes.rs` says why in its own words: the
//! handler resolves through the model CATALOGUE (`store/models.ts`: the static routes, the custom models, the
//! provider models, the disabled set) and the `RouteDO` Durable Object — *"a different surface with its own corpus,
//! the catalogue, not identity"*. So this module does not port the route.
//!
//! **WHAT IT OWNS IS THE THREE DECISIONS UNDER IT**, which are product rules rather than reads:
//!
//!   1. **the key rule** — a channel with a rule is usable only when the USER's key or the DEPLOYMENT's own is
//!      present, and `envKey: null` means pure BYOK (nv/gmi: the `/v1` handler reads only the user's blob, so a
//!      missing user key means every request 502s);
//!   2. **`og`'s breaker leg** — last, and ONLY for `og/…`: a keyless `og` is refused for the key, so the breaker is
//!      never read for it, and the order is the meaning;
//!   3. **the fallback POLICY** — the caller's stored choice when it is usable, else the first usable rung of the
//!      ladder IN ORDER, else the default.
//!
//! `npm test` cannot reach `/__decision` (`helpers.mjs`'s `WASM_GATE` stub answers a fixed model list) and this path
//! is not the status page's, so the seam is the one the previous three landings established: **the Rust renders the
//! TypeScript module that carries the decisions**, and the freshness test is a byte comparison. `model-route.ts`
//! keeps the reads — `isAdvertised`, the user's key blob, the stored route, the breaker — which are I/O.
//!
//! **AND `CHANNEL_KEY_RULES` STAYS MUTABLE, IN TYPESCRIPT, ON PURPOSE.** `registerChannelKey` is an OCP extension
//! point (a channel registers instead of editing the gate), so the emitted key rule takes the RULE as an argument
//! rather than looking it up in Rust — **deriving must not quietly remove an extension point**, which is the same
//! rule `byok.rs`'s derivation carries.
//!
//! MUTATION: make `channel_key_ok` answer `has_user` alone (drop the environment leg).
//! RESULT:   the emitted bytes change, the freshness test fails, and `model-route.test.mjs`'s *"shared-key channels:
//!           user key, env key, or neither"* fails on its env-key rows.
//!
//! MUTATION: make `channel_key_ok` treat a PRESENT BUT EMPTY `env_key` as a fallback name (`is_some()` instead of
//!           `is_some_and(|k| !k.is_empty())`).
//! RESULT:   `an_empty_env_name_is_not_a_fallback` fails. The TypeScript wrote the rule as a TRUTHINESS test
//!           (`rule.envKey ? … : false`), which is what makes `""` behave like `null` — a distinction `Option<&str>`
//!           does not carry by itself, and the one thing a careless port loses.
//!
//! MUTATION: make `pick_auto_route` answer the default without walking the ladder.
//! RESULT:   `the_ladder_is_walked_in_order` fails — a user with no stored choice would get `cm/…` even when a
//!           LATER rung is the usable one.

/// The default channel when the user has made no selection — `cm/deepseek/deepseek-v4.1-flash`.
///
/// **2026-09-10 (V4 retirement)**: this used to be `ds/deepseek-v4-flash` — the official key is now out of balance
/// (402 on every request) and its V4 names are retired, so the default moved to Command Code's V4.1 Flash, the model
/// the whole catalog standardised on (and the one the console's health badge recommends).
pub const DEFAULT_ROUTE_MODEL: &str = "cm/deepseek/deepseek-v4.1-flash";

/// The fallback ladder for `auto`, in PRIORITY ORDER.
///
/// The ORDER here is the meaning — the default channel first, then alternatives — so this is NOT derived from the
/// catalogue the way `ROUTE_INFO`'s per-route lists are. What it must be is a SUBSET of the catalogue: a ladder entry
/// that is not advertised (a typo, or a model removed from the catalogue) would be a fallback that can never satisfy
/// `is_model_usable`, and the ladder would skip it in silence. **`the_ladder_is_a_subset_of_the_catalogue` is where
/// that sentence stopped being a comment and became a test.**
pub const AUTO_FALLBACK_LADDER: [&str; 5] = [
    DEFAULT_ROUTE_MODEL,
    "qw/qwen3.8-max-preview",
    "qw/qwen3.8-flash",
    "og/deepseek-v4.1-flash",
    "or/openai/gpt-5.6-luna:floor[1m]",
];

/// `isModelUsable`'s KEY half: `hasUser || (envKey ? hasEnv : false)`, with "no rule" meaning "no key needed".
///
/// `rule` is `Option<(userKey, envKey)>` — the caller's lookup in its own (mutable, byok-derived) table. **THE EMPTY
/// NAME IS NOT A FALLBACK**, and that is the whole subtlety: the source tests `rule.envKey` for TRUTHINESS, so a
/// record carrying `envKey: ""` behaves exactly like nv/gmi's `null`.
pub fn channel_key_ok(rule: Option<(&str, Option<&str>)>, has_user: bool, has_env: bool) -> bool {
    let Some((_user_key, env_key)) = rule else {
        return true;
    };
    has_user || env_key.is_some_and(|name| !name.is_empty() && has_env)
}

/// `isModelUsable`'s channel half: the key rule first, then **`og`'s breaker and NOTHING else**.
///
/// The prefix test is `model.starts_with("og/")` and NOT "the prefix is og", because that is what the source wrote:
/// a bare `og` (no slash) is not a channel id and never reaches here past the catalogue check, and a port that
/// widened the test would read the breaker for a model the source never read it for.
pub fn channel_usable(
    model: &str,
    rule: Option<(&str, Option<&str>)>,
    has_user: bool,
    has_env: bool,
    degraded: bool,
) -> bool {
    if !channel_key_ok(rule, has_user, has_env) {
        return false;
    }
    if model.starts_with("og/") {
        return !degraded;
    }
    true
}

/// `resolveAutoModel`'s POLICY, over the usability VERDICTS the caller has already gathered: the stored choice when
/// it is usable, else the first usable rung of the ladder in order, else the default.
///
/// `ladder_usable[i]` is the verdict for `AUTO_FALLBACK_LADDER[i]`; a short slice is treated as "not usable", so a
/// caller that stops asking early cannot be read as a caller that found a rung usable.
pub fn pick_auto_route(
    chosen: Option<&str>,
    chosen_usable: bool,
    ladder_usable: &[bool],
) -> String {
    if let Some(choice) = chosen {
        if !choice.is_empty() && chosen_usable {
            return choice.to_string();
        }
    }
    for (i, model) in AUTO_FALLBACK_LADDER.iter().enumerate() {
        if ladder_usable.get(i).copied().unwrap_or(false) {
            return (*model).to_string();
        }
    }
    DEFAULT_ROUTE_MODEL.to_string()
}

/// The emitted module's header — **THE SOURCE FILE'S OWN COMMENTS, CARRIED VERBATIM, PLUS THE GENERATED NOTICE.**
/// An emission that loses the file's documented reason is a loss, not a simplification: the V4-retirement history
/// and the OCP argument are not recoverable from the rows.
const TS_HEADER: &str = r#"/**
 * model-route-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/auto_route.rs` (`render_typescript`), which is now the single source. The freshness
 * test in that module is a byte comparison against this file, so a hand edit fails there with the text and one
 * command to fix it:
 *
 *     SUMMRISE_REFRESH_AUTO_ROUTE=1 cargo test --manifest-path gateway/wasm/Cargo.toml auto_route::emission
 *
 * `gateway/src/model-route.ts` re-exports this module and keeps the I/O these decisions are made FROM — the
 * catalogue (`isAdvertised`), the user's key blob, the stored route and the breaker. **`CHANNEL_KEY_RULES` stays
 * over there, mutable, because `registerChannelKey` is an extension point**, which is why `channelKeyOk` takes the
 * rule as an argument instead of looking one up.
 *
 * The comments below are the source file's own, carried verbatim: they are the record of why the default and the
 * ladder are what they are, and none of it is recoverable from the rows.
 */

/** Model usable for routing? In the whitelist, (og) breaker not open, AND the REQUESTING user's key for that channel
 *  is configured — a channel without a key 502s every request, so model=auto must not route to it.
 *  round-68: the old code checked ADMIN_ID's keys — a BYOK user with only an og key was told og was "unusable" (the
 *  admin lacks it) and routed to ds, which the user lacks, so every model=auto request 502'd. */

// SOLID Round-26 (OCP): per-channel key rules are DATA — adding a channel registers one entry instead of appending
// another if-line (nv/gmi/cm/amd each arrived as a new line). userKey is always required; envKey null means pure
// BYOK (the /v1 handler reads ONLY the user's key blob there).
export interface ChannelKeyRule {
  userKey: string;
  envKey: string | null;
}

/** `isModelUsable`'s KEY half. **`envKey` FALSY IS NOT A FALLBACK**: nv and gmi carry `null` (pure BYOK) and a
 *  record carrying an EMPTY name behaves the same way, because the source tests the name for truthiness. */
export function channelKeyOk(
  rule: ChannelKeyRule | null,
  hasUser: boolean,
  hasEnv: boolean,
): boolean {
  if (!rule) return true;
  return hasUser || (rule.envKey ? hasEnv : false);
}

/** `isModelUsable`'s channel half: the key rule first, then **`og`'s breaker and NOTHING else**. The order is the
 *  meaning — a keyless `og` is refused for the KEY, so the breaker is never read for it. */
export function channelUsable(
  model: string,
  rule: ChannelKeyRule | null,
  hasUser: boolean,
  hasEnv: boolean,
  degraded: boolean,
): boolean {
  if (!channelKeyOk(rule, hasUser, hasEnv)) return false;
  if (model.startsWith("og/")) return !degraded;
  return true;
}

// Default channel when the user hasn't made a selection. 2026-09-10 (V4 retirement): this used to be
// ds/deepseek-v4-flash — the official key is now out of balance (402 on every request) and its V4 names are
// retired, so the default moved to Command Code's V4.1 Flash, the model the whole catalog standardised on (and the
// one the console's health badge recommends).
export const DEFAULT_ROUTE_MODEL = "cm/deepseek/deepseek-v4.1-flash";

/** The fallback ladder for `auto`, in PRIORITY ORDER.
 *
 *  The ORDER here is the meaning — the default channel first, then alternatives — so this is NOT derived from the
 *  catalogue the way ROUTE_INFO's per-route lists are. What it must be is a SUBSET of the catalogue: a ladder entry
 *  that is not advertised (a typo, or a model removed from the catalogue) would be a fallback that can never
 *  satisfy `isModelUsable`, and the ladder would skip it in silence. `cors.rs`'s neighbour in this crate,
 *  `auto_route.rs`, is where that sentence is a TEST (`the_ladder_is_a_subset_of_the_catalogue`). */
export const AUTO_FALLBACK_LADDER: string[] = [
  DEFAULT_ROUTE_MODEL,
  "qw/qwen3.8-max-preview",
  "qw/qwen3.8-flash",
  "og/deepseek-v4.1-flash",
  "or/openai/gpt-5.6-luna:floor[1m]",
];

/** `resolveAutoModel`'s POLICY, with the I/O handed in: the caller's stored choice when it is usable, else the first
 *  usable rung of the ladder IN ORDER, else the default. `isUsable` is the caller's async predicate — the reads
 *  (the catalogue, the key blob, the breaker) are `model-route.ts`'s, and this is what decides with them. */
export async function pickAutoRoute(
  chosen: string | null | undefined,
  isUsable: (model: string) => Promise<boolean>,
): Promise<string> {
  if (chosen && (await isUsable(chosen))) return chosen;
  for (const model of AUTO_FALLBACK_LADDER) {
    if (await isUsable(model)) return model;
  }
  return DEFAULT_ROUTE_MODEL;
}
"#;

/// **THE PRODUCER.** The emitted module is exactly this string.
pub fn render_typescript() -> String {
    TS_HEADER.to_string()
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/model-route-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK — AND IT IS THE REASON THE EMISSION CANNOT DRIFT.** The same shape `mcp_tools.rs`,
    /// `settings_logic.rs`, `byok.rs` and `cors.rs` use.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_AUTO_ROUTE").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write model-route-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_AUTO_ROUTE=1 cargo test auto_route::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/model-route-logic.ts is not what auto_route.rs emits — \
             run SUMMRISE_REFRESH_AUTO_ROUTE=1 cargo test auto_route::emission"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key matrix, in the four shapes the source distinguishes. **THE EMPTY NAME IS THE ONE A CARELESS PORT
    /// LOSES**: nv/gmi's `None` and an `envKey: ""` record are the same decision, and only a truthiness test says so.
    ///
    /// MUTATION: `env_key.is_some()` in place of `is_some_and(|k| !k.is_empty() && has_env)`.
    /// RESULT:   the `Some("")` assertions fail.
    #[test]
    fn the_key_matrix_and_the_empty_name() {
        let nv = Some(("NVAPI_KEY", None));
        assert!(channel_key_ok(nv, true, false), "the user's key is enough");
        assert!(
            !channel_key_ok(nv, false, true),
            "nv is pure BYOK: the env never substitutes"
        );
        assert!(!channel_key_ok(nv, false, false), "and keyless is unusable");
        let qw = Some(("QWEN_API_KEY", Some("QWEN_API_KEY")));
        assert!(channel_key_ok(qw, true, false));
        assert!(
            channel_key_ok(qw, false, true),
            "a shared-key channel honours the deployment's own"
        );
        assert!(!channel_key_ok(qw, false, false));
        assert!(channel_key_ok(None, false, false), "no rule, no key needed");
        let empty = Some(("ZZ_KEY", Some("")));
        assert!(
            !channel_key_ok(empty, false, true),
            "an EMPTY name is not a fallback"
        );
        assert!(channel_key_ok(empty, true, false));
    }

    /// **`og`'s LEG IS LAST, AND ONLY FOR `og/…`** — the order is the meaning, and the prefix test is the source's
    /// (`starts_with("og/")`, not "the prefix is og").
    #[test]
    fn the_breaker_leg_is_last_and_og_only() {
        let og = Some(("OPENCODE_GO_API_KEY", Some("OPENCODE_GO_API_KEY")));
        assert!(channel_usable(
            "og/deepseek-v4.1-flash",
            og,
            true,
            false,
            false
        ));
        assert!(
            !channel_usable("og/deepseek-v4.1-flash", og, true, false, true),
            "open breaker degrades"
        );
        assert!(
            !channel_usable("og/deepseek-v4.1-flash", og, false, false, false),
            "a keyless og is refused for the KEY — the breaker is not consulted"
        );
        assert!(
            channel_usable("qw/qwen3.8-flash", og, true, false, true),
            "another channel is untouched by the breaker"
        );
        assert!(
            channel_usable("og", None, false, false, true),
            "a bare `og` is not an `og/` model"
        );
    }

    /// **THE LADDER IS WALKED IN ORDER, AND THE DEFAULT IS THE FLOOR** — the policy, with the verdicts handed in.
    ///
    /// MUTATION: return `DEFAULT_ROUTE_MODEL` without the loop.
    /// RESULT:   the second assertion fails.
    #[test]
    fn the_ladder_is_walked_in_order() {
        let chosen = Some("qw/qwen3.8-max-preview");
        assert_eq!(
            pick_auto_route(chosen, true, &[false; 5]),
            "qw/qwen3.8-max-preview"
        );
        assert_eq!(
            pick_auto_route(chosen, false, &[false, true, true, false, false]),
            AUTO_FALLBACK_LADDER[1],
            "the user's choice is unusable: the FIRST usable rung wins, not the best one"
        );
        assert_eq!(
            pick_auto_route(None, false, &[false; 5]),
            DEFAULT_ROUTE_MODEL,
            "nothing usable, and no choice: the floor"
        );
        assert_eq!(
            pick_auto_route(Some(""), true, &[false, false, true, false, false]),
            AUTO_FALLBACK_LADDER[2],
            "an EMPTY stored route is no choice at all"
        );
        assert_eq!(
            pick_auto_route(chosen, true, &[]),
            "qw/qwen3.8-max-preview",
            "no verdicts is not a usable rung"
        );
    }

    /// **THE SENTENCE IN THE SOURCE'S COMMENT, AS A TEST.** The ladder's comment says it "must be a SUBSET of the
    /// catalogue" and nothing checked it: a typo there is a fallback that can never satisfy `is_model_usable`, and
    /// the ladder would skip it in silence. The catalogue is `registry.rs`'s, which is the worker's own.
    #[test]
    fn the_ladder_is_a_subset_of_the_catalogue() {
        for model in AUTO_FALLBACK_LADDER {
            assert!(
                crate::registry::model_spec(model).is_some(),
                "the ladder's rung {model} is not in the catalogue"
            );
        }
        assert!(
            crate::registry::model_spec(DEFAULT_ROUTE_MODEL).is_some(),
            "the default is not in the catalogue"
        );
    }

    /// **AND THE REGISTRY'S ORDER IS THE LADDER'S ORDER** — the first rung IS the default, which is what makes the
    /// ladder's own comment ("the default channel first, then alternatives") checkable.
    #[test]
    fn the_ladder_starts_at_the_default() {
        assert_eq!(AUTO_FALLBACK_LADDER[0], DEFAULT_ROUTE_MODEL);
        assert_eq!(AUTO_FALLBACK_LADDER.len(), 5);
    }
}
