//! The control contract: one function decides whether a caller may use a tool (design §4.3).
//!
//! The four rules in [`authorize`] are the whole security model. Nothing else in the device checks
//! an origin, a scope or an approval: the panel, the console and MCP render the [`Decision`] this
//! module returns, so `Allow`, `Ask` and `Deny` mean the same thing on every surface.
//!
//! The module is pure on purpose. Time is an input ([`Ctx::now_ms`]) rather than a call to the
//! clock, which is what makes a grant's expiry testable at its exact boundary.

use std::fmt;

/// A tool a caller wants to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// The tool's stable name, as the capability registry spells it (`terminal.create`).
    pub name: String,
    /// Whether the tool does something a later read cannot undo.
    ///
    /// The capability registry declares this per tool (design §4.5); it is data here and is never
    /// inferred from the name.
    pub destructive: bool,
}

impl ToolCall {
    /// A call to `name`; `destructive` comes from the registry.
    pub fn new(name: impl Into<String>, destructive: bool) -> Self {
        Self {
            name: name.into(),
            destructive,
        }
    }
}

/// The caller's identity, as the device established it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// The owner, reachable only with the install token (design §9.1, §14).
    ///
    /// Loopback is never this: on Windows every process of every user can open a loopback socket.
    Owner,
    /// A device key issued to a client.
    Key {
        /// The key's id, for audit and for revoking the right key.
        id: String,
        /// The scopes the console recorded with the key.
        ///
        /// Carried so the console and audit can render them; the four rules of [`authorize`] do not
        /// consult them, because the grant is the permission. A scope that should restrict a call
        /// belongs in the grant's `tool_patterns`, where one function reads it.
        scopes: Vec<String>,
    },
    /// Nobody in particular.
    Anonymous,
}

/// How the request reached the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// A socket on this machine — a transport, never an identity.
    Loopback,
    /// The self-hosted relay (or a direct mesh peer path).
    Relay,
    /// The operator's console.
    Console,
}

/// A permission the owner issued, possibly to a remote client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The tool names the grant covers.
    ///
    /// `*` matches any run of characters; every other byte is literal. `terminal.*` therefore
    /// covers `terminal.create` but not `terminalx.pty`. An empty list covers nothing.
    pub tool_patterns: Vec<String>,
    /// Unix milliseconds after which the grant is dead.
    pub expires_at_ms: i64,
    /// Whether destructive calls are covered without asking the owner each time.
    pub auto_for_destructive: bool,
}

impl Grant {
    /// Whether this grant covers `tool`.
    pub fn covers(&self, tool: &ToolCall) -> bool {
        self.tool_patterns
            .iter()
            .any(|pattern| pattern_matches(pattern, &tool.name))
    }

    /// Whether `now_ms` is at or past the grant's expiry.
    ///
    /// The `exp` reading: a grant is valid strictly before `expires_at_ms`, so the boundary instant
    /// belongs to the expired side. An off-by-one here is a grant that outlives its stated life.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// The decision's view of the console's signed grant.
///
/// The signed wire type is [`crate::grant::Grant`]: it carries the device it is for and a `u64`
/// expiry because that is what the console signs. Authorization needs neither the device id (the
/// channel already proved which device this is) nor a value wider than its clock. It does inherit
/// the signature's destructive consent: `allow_destructive` is the operator's standing answer, and
/// a grant that does not carry it becomes an `Ask`, never an `Allow`. The conversion is deliberately
/// one-way: a policy grant cannot be re-signed into someone else's facts.
impl From<&crate::grant::Grant> for Grant {
    fn from(signed: &crate::grant::Grant) -> Self {
        Self {
            tool_patterns: signed.tools.clone(),
            expires_at_ms: i64::try_from(signed.expires_at_ms).unwrap_or(i64::MAX),
            auto_for_destructive: signed.allow_destructive,
        }
    }
}

/// Whether a human has answered the question a destructive call raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalState {
    /// No approval flow is in play.
    NotRequired,
    /// An approval has been requested and is unanswered.
    Pending,
    /// A human approved at `at_ms`.
    ///
    /// The instant is carried for the audit trail and for rendering; the decision only needs to
    /// know that the question was answered, because `Ctx` has no request timestamp to compare it
    /// with.
    Granted { at_ms: i64 },
}

/// Everything a decision depends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ctx<'a> {
    /// Who is calling.
    pub subject: Subject,
    /// The grant presented with the call, if any.
    pub grant: Option<&'a Grant>,
    /// How the call arrived.
    pub transport: Transport,
    /// Whether a human has answered for this call.
    pub approval: ApprovalState,
    /// The decision's clock, in Unix milliseconds.
    pub now_ms: i64,
}

/// Why a call was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DenyReason {
    /// The subject is [`Subject::Anonymous`]; no grant can authorize it.
    Anonymous,
    /// A remote transport carried no grant at all.
    NoGrant,
    /// The presented grant is at or past its expiry.
    GrantExpired {
        /// The grant's expiry, in Unix milliseconds.
        expires_at_ms: i64,
        /// The instant the decision was asked at.
        now_ms: i64,
    },
    /// The grant is live but no pattern covers the tool.
    ToolNotCovered {
        /// The refused tool's name.
        tool: String,
    },
    /// No rule allowed the call — the loopback default.
    DefaultDeny,
}

impl fmt::Display for DenyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DenyReason::Anonymous => write!(f, "the caller is anonymous; a grant is required"),
            DenyReason::NoGrant => {
                write!(f, "a remote call requires a grant, and none was presented")
            }
            DenyReason::GrantExpired {
                expires_at_ms,
                now_ms,
            } => write!(
                f,
                "the grant expired at {expires_at_ms} ms; the decision was asked at {now_ms} ms"
            ),
            DenyReason::ToolNotCovered { tool } => {
                write!(f, "the grant does not cover '{tool}'")
            }
            DenyReason::DefaultDeny => write!(f, "no rule allows this call"),
        }
    }
}

/// What a caller may do with a tool.
///
/// A value, not a boolean with a note: every surface renders one of these three, so a client that
/// receives [`Decision::Ask`] does not have to guess whether it may retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The call may proceed.
    Allow,
    /// The call needs a human's answer first.
    Ask {
        /// The question to show, worded once here so every surface asks it the same way.
        prompt: String,
    },
    /// The call may not proceed.
    Deny {
        /// Why, in a form the console can render without matching English.
        reason: DenyReason,
    },
}

/// Decide whether `tool` may be used in `ctx` (design §4.3).
///
/// The rules, in order:
///
/// 1. [`Subject::Owner`] → [`Decision::Allow`]. The owner is the install token, never the transport,
///    so an expired grant or a pending approval does not constrain an owner.
/// 2. A remote transport ([`Transport::Relay`], [`Transport::Console`]) requires a grant that covers
///    the tool and has not expired; without one the call is denied. The grant authorizes on every
///    transport — it is the owner's permission — and the transport only decides what an absent grant
///    means: `NoGrant` for a remote call, the default deny for a loopback one.
/// 3. A destructive tool → [`Decision::Ask`], unless the grant says `auto_for_destructive` or the
///    approval is already [`ApprovalState::Granted`]. A live grant that matches the tool but neither
///    covers destruction is exactly the case a human is asked about.
/// 4. Everything else → [`Decision::Deny`]; an [`Subject::Anonymous`] caller is denied before any
///    grant is considered.
///
/// An expired or non-matching grant is a denial, not something to ask about: asking would offer a
/// human a decision the grant already answered.
pub fn authorize(ctx: &Ctx<'_>, tool: &ToolCall) -> Decision {
    // Rule 1 — the owner, whatever the grant says (or does not say).
    if matches!(ctx.subject, Subject::Owner) {
        return Decision::Allow;
    }

    // Rule 4's explicit half — anonymity is not a weaker identity; it is no identity at all.
    if matches!(ctx.subject, Subject::Anonymous) {
        return Decision::Deny {
            reason: DenyReason::Anonymous,
        };
    }

    // Rule 2 — a grant that covers the tool and has not expired.
    let Some(grant) = ctx.grant else {
        return Decision::Deny {
            reason: match ctx.transport {
                Transport::Relay | Transport::Console => DenyReason::NoGrant,
                Transport::Loopback => DenyReason::DefaultDeny,
            },
        };
    };
    if grant.is_expired(ctx.now_ms) {
        return Decision::Deny {
            reason: DenyReason::GrantExpired {
                expires_at_ms: grant.expires_at_ms,
                now_ms: ctx.now_ms,
            },
        };
    }
    if !grant.covers(tool) {
        return Decision::Deny {
            reason: DenyReason::ToolNotCovered {
                tool: tool.name.clone(),
            },
        };
    }

    // Rule 3 — destruction asks once. The grant's `auto` is the owner's standing answer; a
    // `Granted` approval is the answer to this call. `Pending` is not an answer yet, so it asks
    // again rather than pretending the call is authorized.
    if !tool.destructive || grant.auto_for_destructive {
        return Decision::Allow;
    }
    match ctx.approval {
        ApprovalState::Granted { .. } => Decision::Allow,
        ApprovalState::NotRequired | ApprovalState::Pending => Decision::Ask {
            prompt: format!("'{}' is destructive and needs approval", tool.name),
        },
    }
}

/// Whether `pattern` matches `name`, with `*` as the only special byte.
///
/// A linear backtracking matcher: it allocates nothing and cannot blow up on a pathological pattern
/// the way a recursive one can. `*` matches any run of bytes, including the empty one, so
/// `terminal.*` covers both `terminal.create` and `terminal.`.
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.as_bytes();
    let name = name.as_bytes();
    let mut p = 0;
    let mut n = 0;
    let mut star = None;
    let mut resumed = 0;

    while n < name.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            resumed = n;
            p += 1;
        } else if p < pattern.len() && pattern[p] == name[n] {
            p += 1;
            n += 1;
        } else if let Some(star_at) = star {
            p = star_at + 1;
            resumed += 1;
            n = resumed;
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|byte| *byte == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grant that covers every tool, live at the `now_ms` the tests use.
    fn live(auto_for_destructive: bool) -> Grant {
        Grant {
            tool_patterns: vec!["*".to_owned()],
            expires_at_ms: 2_000,
            auto_for_destructive,
        }
    }

    fn key() -> Subject {
        Subject::Key {
            id: "key-1".to_owned(),
            scopes: vec!["terminal.*".to_owned()],
        }
    }

    fn tool(name: &str, destructive: bool) -> ToolCall {
        ToolCall {
            name: name.to_owned(),
            destructive,
        }
    }

    fn ctx<'a>(
        subject: Subject,
        grant: Option<&'a Grant>,
        transport: Transport,
        approval: ApprovalState,
        now_ms: i64,
    ) -> Ctx<'a> {
        Ctx {
            subject,
            grant,
            transport,
            approval,
            now_ms,
        }
    }

    fn deny(reason: DenyReason) -> Decision {
        Decision::Deny { reason }
    }

    #[test]
    fn the_owner_is_allowed_before_every_other_rule() {
        let expired = Grant {
            tool_patterns: Vec::new(),
            expires_at_ms: 10,
            auto_for_destructive: false,
        };
        let grant = live(false);

        let cases = [
            (
                "no grant over the relay",
                Transport::Relay,
                None,
                ApprovalState::NotRequired,
            ),
            (
                "an expired grant over the console",
                Transport::Console,
                Some(&expired),
                ApprovalState::NotRequired,
            ),
            (
                "a pending approval on loopback",
                Transport::Loopback,
                Some(&grant),
                ApprovalState::Pending,
            ),
        ];

        for (case, transport, presented, approval) in cases {
            for destructive in [false, true] {
                let context = ctx(Subject::Owner, presented, transport, approval, 1_000);
                assert_eq!(
                    authorize(&context, &tool("files.write", destructive)),
                    Decision::Allow,
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn an_anonymous_caller_is_always_denied() {
        let auto = live(true);
        let cases = [
            ("loopback, no grant", Transport::Loopback, None),
            ("relay, an auto grant", Transport::Relay, Some(&auto)),
            (
                "console, an auto grant and an approval",
                Transport::Console,
                Some(&auto),
            ),
        ];

        for (case, transport, presented) in cases {
            for approval in [
                ApprovalState::NotRequired,
                ApprovalState::Pending,
                ApprovalState::Granted { at_ms: 1_500 },
            ] {
                for destructive in [false, true] {
                    let context = ctx(Subject::Anonymous, presented, transport, approval, 1_000);
                    assert_eq!(
                        authorize(&context, &tool("terminal.create", destructive)),
                        deny(DenyReason::Anonymous),
                        "{case}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_remote_call_without_a_grant_is_denied_and_loopback_falls_to_default_deny() {
        let cases = [
            ("relay", Transport::Relay, DenyReason::NoGrant),
            ("console", Transport::Console, DenyReason::NoGrant),
            ("loopback", Transport::Loopback, DenyReason::DefaultDeny),
        ];

        for (case, transport, reason) in cases {
            for destructive in [false, true] {
                let context = ctx(key(), None, transport, ApprovalState::NotRequired, 1_000);
                assert_eq!(
                    authorize(&context, &tool("terminal.create", destructive)),
                    deny(reason.clone()),
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn an_expired_grant_authorizes_nothing() {
        let grant = Grant {
            tool_patterns: vec!["*".to_owned()],
            expires_at_ms: 1_000,
            auto_for_destructive: true,
        };

        // A grant is live strictly before its expiry: at the instant and after it, it is dead. An
        // approval does not revive it, and neither does `auto_for_destructive`.
        for now_ms in [1_000, 1_001, 9_999] {
            let context = ctx(
                key(),
                Some(&grant),
                Transport::Relay,
                ApprovalState::Granted { at_ms: 900 },
                now_ms,
            );
            for destructive in [false, true] {
                assert_eq!(
                    authorize(&context, &tool("terminal.create", destructive)),
                    deny(DenyReason::GrantExpired {
                        expires_at_ms: 1_000,
                        now_ms,
                    }),
                    "now_ms = {now_ms}"
                );
            }
        }

        // One millisecond earlier the same grant is live, auto covers the destructive call, and the
        // only thing that changed is the clock.
        let context = ctx(
            key(),
            Some(&grant),
            Transport::Relay,
            ApprovalState::NotRequired,
            999,
        );
        assert_eq!(
            authorize(&context, &tool("terminal.create", true)),
            Decision::Allow
        );
    }

    #[test]
    fn a_grant_covers_only_the_tools_its_patterns_match() {
        let grant = Grant {
            tool_patterns: vec![
                "terminal.*".to_owned(),
                "files.read".to_owned(),
                "memory".to_owned(),
            ],
            expires_at_ms: 2_000,
            auto_for_destructive: false,
        };

        let cases = [
            ("terminal.create", true),
            ("terminal.ssh", true),
            ("terminal.serial", true),
            ("terminal.", true),
            ("files.read", true),
            ("memory", true),
            ("files.write", false),
            ("files.read.all", false),
            ("terminalx.pty", false),
            ("Memory", false),
        ];

        for (name, covered) in cases {
            let context = ctx(
                key(),
                Some(&grant),
                Transport::Relay,
                ApprovalState::NotRequired,
                1_000,
            );
            let decision = authorize(&context, &tool(name, false));
            if covered {
                assert_eq!(decision, Decision::Allow, "{name} is covered");
            } else {
                assert_eq!(
                    decision,
                    deny(DenyReason::ToolNotCovered {
                        tool: name.to_owned()
                    }),
                    "{name} is not covered"
                );
            }
        }
    }

    #[test]
    fn a_grant_with_no_patterns_covers_nothing() {
        let grant = Grant {
            tool_patterns: Vec::new(),
            expires_at_ms: 2_000,
            auto_for_destructive: true,
        };
        let context = ctx(
            key(),
            Some(&grant),
            Transport::Loopback,
            ApprovalState::Granted { at_ms: 1_500 },
            1_000,
        );

        assert_eq!(
            authorize(&context, &tool("terminal.create", true)),
            deny(DenyReason::ToolNotCovered {
                tool: "terminal.create".to_owned()
            }),
            "an empty pattern list covers no tool, not every tool"
        );
    }

    #[test]
    fn a_live_grant_allows_a_non_destructive_call_on_every_transport() {
        let grant = live(false);

        for transport in [Transport::Loopback, Transport::Relay, Transport::Console] {
            for approval in [
                ApprovalState::NotRequired,
                ApprovalState::Pending,
                ApprovalState::Granted { at_ms: 1_500 },
            ] {
                let context = ctx(key(), Some(&grant), transport, approval, 1_000);
                assert_eq!(
                    authorize(&context, &tool("terminal.create", false)),
                    Decision::Allow,
                    "{transport:?}"
                );
            }
        }
    }

    #[test]
    fn a_destructive_call_asks_when_neither_auto_nor_an_approval_applies() {
        let grant = live(false);

        for approval in [ApprovalState::NotRequired, ApprovalState::Pending] {
            let context = ctx(key(), Some(&grant), Transport::Relay, approval, 1_000);
            assert_eq!(
                authorize(&context, &tool("terminal.create", true)),
                Decision::Ask {
                    prompt: "'terminal.create' is destructive and needs approval".to_owned()
                },
                "{approval:?}"
            );
        }
    }

    #[test]
    fn a_destructive_call_is_allowed_when_the_grant_says_auto() {
        let grant = live(true);

        for approval in [ApprovalState::NotRequired, ApprovalState::Pending] {
            let context = ctx(key(), Some(&grant), Transport::Console, approval, 1_000);
            assert_eq!(
                authorize(&context, &tool("terminal.create", true)),
                Decision::Allow,
                "{approval:?}"
            );
        }
    }

    #[test]
    fn a_granted_approval_allows_the_destructive_call() {
        let grant = live(false);
        let context = ctx(
            key(),
            Some(&grant),
            Transport::Relay,
            ApprovalState::Granted { at_ms: 1_500 },
            1_000,
        );

        assert_eq!(
            authorize(&context, &tool("terminal.create", true)),
            Decision::Allow
        );
    }

    #[test]
    fn a_signed_console_grant_becomes_a_policy_grant_carrying_its_destructive_consent() {
        let signed = crate::grant::Grant {
            device_id: "ab".repeat(32),
            tools: vec!["files.read".to_owned()],
            expires_at_ms: u64::MAX,
            allow_destructive: false,
            nonce: String::new(),
        };

        let policy = Grant::from(&signed);

        assert_eq!(policy.tool_patterns, signed.tools);
        assert_eq!(
            policy.expires_at_ms,
            i64::MAX,
            "an expiry beyond the decision's clock is clamped, never wrapped negative"
        );
        assert!(
            !policy.auto_for_destructive,
            "a grant that did not consent does not preauthorize destruction"
        );
        assert!(policy.covers(&tool("files.read", false)));
        assert!(!policy.covers(&tool("files.write", true)));

        let consenting = crate::grant::Grant {
            allow_destructive: true,
            ..signed
        };
        assert!(
            Grant::from(&consenting).auto_for_destructive,
            "the signature's consent is the policy's standing answer"
        );
    }
}
