//! Whether an outside reply to an existing thread runs the agent or waits for the team.
//!
//! Not to be confused with `manual_handoffs` (see `entities::attention::NewManualHandoff`), which
//! is a generic, human-created work item with no source message and no generation.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    attention::BusinessPriority, message::CanonicalMessageId, transport::PrincipalId,
};

/// How a channel treats an eligible outside reply to a thread it already carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalReplyHandling {
    #[default]
    Automatic,
    ManualHandoff,
}

impl ExternalReplyHandling {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::ManualHandoff => "manual_handoff",
        }
    }

    /// Whether an eligible outside reply waits for the team instead of running the agent.
    pub const fn holds_outside_replies(self) -> bool {
        matches!(self, Self::ManualHandoff)
    }
}

impl FromStr for ExternalReplyHandling {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "automatic" => Ok(Self::Automatic),
            "manual_handoff" => Ok(Self::ManualHandoff),
            _ => Err(format!("invalid external reply handling policy '{value}'")),
        }
    }
}

/// A company default, a channel's optional override, and the value the two resolve to.
///
/// Lives in the domain rather than beside the port because it carries the resolution rule: the
/// override is never copied into the channel row, so "inherit" keeps following the company.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalReplyHandlingPolicy {
    pub company_default: ExternalReplyHandling,
    pub channel_override: Option<ExternalReplyHandling>,
    effective: ExternalReplyHandling,
}

impl ExternalReplyHandlingPolicy {
    /// `None` inherits the company default live; a stored override wins.
    pub const fn resolve(
        company_default: ExternalReplyHandling,
        channel_override: Option<ExternalReplyHandling>,
    ) -> Self {
        // `Option::unwrap_or` is not `const`, and this decision is the whole point of the type.
        let effective = match channel_override {
            Some(value) => value,
            None => company_default,
        };
        Self {
            company_default,
            channel_override,
            effective,
        }
    }

    pub const fn effective(&self) -> ExternalReplyHandling {
        self.effective
    }

    pub const fn is_inherited(&self) -> bool {
        self.channel_override.is_none()
    }
}

/// Everything the hold rule looks at, named so a call site cannot swap two bools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutsideReplyFacts {
    /// The channel's *effective* reply-handling policy, already resolved through `COALESCE`.
    pub handling: ExternalReplyHandling,
    /// Whether this message continues a thread that already existed.
    pub continues_existing_thread: bool,
    /// Whether the sender is outside the company -- `CompanyMembership::None`.
    pub sender_is_outside: bool,
    /// Whether this channel would have answered: `To`, or a `Cc` the body named.
    pub channel_answers: bool,
    /// Whether the message asked for an answer at all, after the `.quiet` fold.
    pub asks_for_an_answer: bool,
    /// Whether this copy closes an outreach the platform was awaiting.
    pub closes_outreach: bool,
}

/// Whether this channel's copy of an outside reply waits for the team.
///
/// The whole hold rule, as one conjunction over already-loaded values: no `self`, no `async`, no
/// persistence. Every term is load-bearing -- the unit tests below flip each one in turn -- and a
/// fact that is not named here cannot be accidentally depended on.
pub const fn holds_outside_reply(facts: OutsideReplyFacts) -> bool {
    facts.handling.holds_outside_replies()
        && facts.continues_existing_thread
        && facts.sender_is_outside
        && facts.channel_answers
        && facts.asks_for_an_answer
        && !facts.closes_outreach
}

/// Why this channel's copy is, or is not, being held. Stated per channel in the read-only phase.
///
/// An enum rather than a bool so the reason survives into the logs and the tests: "the channel
/// answers automatically" and "the channel holds, but this message is not an eligible reply" are
/// different answers to a support question, and a bool collapses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoldDecision {
    Hold,
    /// The channel holds outside replies, but this message is not an eligible one.
    NotEligible,
    /// The channel answers automatically.
    Automatic,
}

/// Where one generation of a thread handoff has got to.
///
/// Actionability is a property of the state, not a column: `needs_instruction` and `draft_ready`
/// are work somebody must do, `drafting` is visible progress, and the two terminal states are
/// neither. Phase 3's projection asks this rather than re-listing states in SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHandoffState {
    NeedsInstruction,
    Drafting,
    DraftReady,
    Resolved,
    Dismissed,
}

impl ThreadHandoffState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeedsInstruction => "needs_instruction",
            Self::Drafting => "drafting",
            Self::DraftReady => "draft_ready",
            Self::Resolved => "resolved",
            Self::Dismissed => "dismissed",
        }
    }

    /// Whether this state is work the team must act on. `drafting` is not.
    pub const fn is_actionable(self) -> bool {
        matches!(self, Self::NeedsInstruction | Self::DraftReady)
    }

    /// Whether a new outside reply may still change this generation.
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Resolved | Self::Dismissed)
    }
}

impl FromStr for ThreadHandoffState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "needs_instruction" => Ok(Self::NeedsInstruction),
            "drafting" => Ok(Self::Drafting),
            "draft_ready" => Ok(Self::DraftReady),
            "resolved" => Ok(Self::Resolved),
            "dismissed" => Ok(Self::Dismissed),
            _ => Err(format!("invalid thread handoff state '{value}'")),
        }
    }
}

/// One thread's handoff, at its current generation.
///
/// At most one row per thread: a second outside reply replaces the generation rather than opening
/// a second handoff. Deliberately **not** `Serialize`: nothing about a handoff belongs in a durable
/// task payload, because a worker holding a snapshotted generation could not notice that a newer
/// customer message had replaced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadHandoff {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub generation: Uuid,
    pub state: ThreadHandoffState,
    pub source_message_id: CanonicalMessageId,
    pub responsible_principal_id: Option<PrincipalId>,
    /// Reused from `entities::attention`, so the attention projection sorts handoffs and manual
    /// handoffs on one set of ranks rather than on two enums that could drift apart.
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub version: u64,
    pub generation_opened_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What a responsibility command does. Named, because `Option<Option<PrincipalId>>` is not a
/// vocabulary: "claim", "release" and "reassign" are three intents that all write one column.
///
/// Internally tagged so the wire form is `{"kind": "claim"}` / `{"kind": "reassign", "to": "…"}`
/// and an unrecognised kind is a serde error rather than a silent default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadHandoffOperation {
    /// Take an unclaimed handoff. Only from `responsible_principal_id IS NULL`.
    Claim,
    /// Give a claimed handoff back to the channel team.
    Release,
    /// Move it to another principal.
    Reassign { to: PrincipalId },
    /// Priority and due time only; responsibility is untouched.
    SetAttributes,
}

/// One accepted change to a handoff's responsibility or business attributes.
///
/// Both fences travel with the command: `expected_version` is the monotonic counter every write
/// bumps, and `expected_generation` is the customer message this instruction was written for. A
/// command that names a generation the thread has moved past is refused rather than applied to
/// whatever arrived since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadHandoffCommand {
    pub company_id: Uuid,
    pub handoff_id: Uuid,
    pub command_id: Uuid,
    pub expected_version: u64,
    pub expected_generation: Uuid,
    pub operation: ThreadHandoffOperation,
    /// Always stated, never `Option`: a command carries the whole attribute pair, so "set priority
    /// high" cannot silently drop a due time somebody else set. The version fence is what makes
    /// restating both safe.
    pub priority: BusinessPriority,
    pub due_at: Option<DateTime<Utc>>,
    pub actor_principal_id: PrincipalId,
    /// Authorization context, not command semantics; omitted from the idempotency fingerprint.
    #[serde(skip)]
    pub visible_channel_ids: Vec<Uuid>,
}

impl ThreadHandoffCommand {
    /// The rules that need no database, checked before anything is locked.
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_version == 0 || self.expected_version > i64::MAX as u64 {
            return Err(format!(
                "Thread handoff expected version must be between 1 and {}.",
                i64::MAX
            ));
        }
        if self.expected_generation.is_nil() {
            return Err("Thread handoff expected generation must name a generation.".into());
        }
        // Rejected rather than silently rewritten, so the audit's `operation` cannot record a
        // reassignment that was really somebody taking the work themselves.
        if let ThreadHandoffOperation::Reassign { to } = self.operation
            && to == self.actor_principal_id
        {
            return Err("Reassigning a thread handoff to yourself is a claim; use claim.".into());
        }
        Ok(())
    }
}

/// What an accepted command actually did, in `thread_handoff_events`' vocabulary.
///
/// Pure and total over its three inputs rather than an `if` chain inside the write: four cases in
/// a chain beside an `UPDATE` is where an audit log starts disagreeing with the row it describes.
/// The requested intent is an input because `None -> Some(x)` is ambiguous on its own -- a manager
/// handing an unclaimed handoff to somebody else is a reassignment, not that person's claim.
pub const fn thread_handoff_operation_name(
    old_responsible: Option<PrincipalId>,
    new_responsible: Option<PrincipalId>,
    requested: ThreadHandoffOperation,
) -> &'static str {
    if same_principal(old_responsible, new_responsible) {
        // Includes `Claim` on a row that was already theirs: nothing moved, so nothing was
        // claimed, and recording a second claim would make the trail lie.
        return "attributes_changed";
    }
    match requested {
        ThreadHandoffOperation::Claim => "claimed",
        ThreadHandoffOperation::Release => "released",
        ThreadHandoffOperation::Reassign { .. } => "reassigned",
        // Unreachable: `SetAttributes` derives the new responsibility from the old one, so it
        // cannot reach here. Stated rather than `unreachable!()` so this stays a `const fn`.
        ThreadHandoffOperation::SetAttributes => "attributes_changed",
    }
}

/// Whether two responsibilities name the same principal, comparing through `u128` because
/// `PartialEq` is not `const`.
const fn same_principal(left: Option<PrincipalId>, right: Option<PrincipalId>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.as_uuid().as_u128() == right.as_uuid().as_u128(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_inherits_the_company_default_when_the_channel_has_no_override() {
        for company_default in [
            ExternalReplyHandling::Automatic,
            ExternalReplyHandling::ManualHandoff,
        ] {
            let policy = ExternalReplyHandlingPolicy::resolve(company_default, None);
            assert_eq!(policy.effective(), company_default);
            assert_eq!(policy.company_default, company_default);
            assert_eq!(policy.channel_override, None);
            assert!(policy.is_inherited());
        }
    }

    #[test]
    fn resolve_prefers_the_channel_override_over_any_company_default() {
        for company_default in [
            ExternalReplyHandling::Automatic,
            ExternalReplyHandling::ManualHandoff,
        ] {
            for channel_override in [
                ExternalReplyHandling::Automatic,
                ExternalReplyHandling::ManualHandoff,
            ] {
                let policy =
                    ExternalReplyHandlingPolicy::resolve(company_default, Some(channel_override));
                assert_eq!(policy.effective(), channel_override);
                assert_eq!(policy.company_default, company_default);
                assert!(
                    !policy.is_inherited(),
                    "an override equal to the company default is still an override"
                );
            }
        }
    }

    #[test]
    fn as_str_and_from_str_round_trip_every_variant() {
        for policy in [
            ExternalReplyHandling::Automatic,
            ExternalReplyHandling::ManualHandoff,
        ] {
            assert_eq!(policy.as_str().parse(), Ok(policy));
        }
        assert_eq!(ExternalReplyHandling::Automatic.as_str(), "automatic");
        assert_eq!(
            ExternalReplyHandling::ManualHandoff.as_str(),
            "manual_handoff"
        );
    }

    #[test]
    fn from_str_refuses_the_sibling_policys_vocabulary_and_near_misses() {
        for value in ["review_all_external", "autonomous", "", "Automatic"] {
            assert_eq!(
                value.parse::<ExternalReplyHandling>(),
                Err(format!("invalid external reply handling policy '{value}'")),
            );
        }
    }

    #[test]
    fn the_default_is_automatic_and_holds_nothing() {
        assert_eq!(
            ExternalReplyHandling::default(),
            ExternalReplyHandling::Automatic
        );
        assert!(!ExternalReplyHandling::default().holds_outside_replies());
        assert!(ExternalReplyHandling::ManualHandoff.holds_outside_replies());
    }

    #[test]
    fn serde_round_trips_both_variants_as_snake_case() {
        for (policy, json) in [
            (ExternalReplyHandling::Automatic, "\"automatic\""),
            (ExternalReplyHandling::ManualHandoff, "\"manual_handoff\""),
        ] {
            assert_eq!(serde_json::to_string(&policy).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<ExternalReplyHandling>(json).unwrap(),
                policy
            );
        }
        assert!(serde_json::from_str::<ExternalReplyHandling>("\"review_all_external\"").is_err());
    }

    /// The one case the whole feature exists for: every term satisfied.
    ///
    /// Every other test in this section is this value with exactly one field changed, which is
    /// what makes "the conjunction has no redundant term" an assertion rather than a claim.
    const fn held_case() -> OutsideReplyFacts {
        OutsideReplyFacts {
            handling: ExternalReplyHandling::ManualHandoff,
            continues_existing_thread: true,
            sender_is_outside: true,
            channel_answers: true,
            asks_for_an_answer: true,
            closes_outreach: false,
        }
    }

    #[test]
    fn an_outside_reply_to_an_existing_thread_on_a_manual_handoff_channel_is_held() {
        assert!(holds_outside_reply(held_case()));
    }

    #[test]
    fn an_automatic_channel_holds_nothing() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            handling: ExternalReplyHandling::Automatic,
            ..held_case()
        }));
    }

    #[test]
    fn a_new_conversation_is_never_held() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            continues_existing_thread: false,
            ..held_case()
        }));
    }

    #[test]
    fn a_teammates_reply_is_never_held() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            sender_is_outside: false,
            ..held_case()
        }));
    }

    #[test]
    fn a_quiet_message_is_never_held() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            asks_for_an_answer: false,
            ..held_case()
        }));
    }

    #[test]
    fn a_reply_that_closes_an_outreach_is_never_held() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            closes_outreach: true,
            ..held_case()
        }));
    }

    #[test]
    fn a_passively_copied_channel_is_never_held() {
        assert!(!holds_outside_reply(OutsideReplyFacts {
            channel_answers: false,
            ..held_case()
        }));
    }

    /// Flipping any single term must flip the verdict. A term that can be removed without failing
    /// this is a term that should not be in [`OutsideReplyFacts`].
    #[test]
    fn every_single_term_is_load_bearing() {
        /// One term of the conjunction, and the change that should stop the hold on its own.
        struct Flip {
            term: &'static str,
            apply: fn(&mut OutsideReplyFacts),
        }

        let flips = [
            Flip {
                term: "handling",
                apply: |facts| facts.handling = ExternalReplyHandling::Automatic,
            },
            Flip {
                term: "continues_existing_thread",
                apply: |facts| facts.continues_existing_thread = false,
            },
            Flip {
                term: "sender_is_outside",
                apply: |facts| facts.sender_is_outside = false,
            },
            Flip {
                term: "channel_answers",
                apply: |facts| facts.channel_answers = false,
            },
            Flip {
                term: "asks_for_an_answer",
                apply: |facts| facts.asks_for_an_answer = false,
            },
            Flip {
                term: "closes_outreach",
                apply: |facts| facts.closes_outreach = true,
            },
        ];
        for Flip { term, apply } in flips {
            let mut facts = held_case();
            apply(&mut facts);
            assert!(
                !holds_outside_reply(facts),
                "flipping {term} alone must stop the hold"
            );
        }
    }

    #[test]
    fn handoff_state_round_trips_every_variant() {
        for state in [
            ThreadHandoffState::NeedsInstruction,
            ThreadHandoffState::Drafting,
            ThreadHandoffState::DraftReady,
            ThreadHandoffState::Resolved,
            ThreadHandoffState::Dismissed,
        ] {
            assert_eq!(state.as_str().parse(), Ok(state));
        }
        assert_eq!(
            ThreadHandoffState::NeedsInstruction.as_str(),
            "needs_instruction"
        );
        assert_eq!(ThreadHandoffState::DraftReady.as_str(), "draft_ready");
    }

    #[test]
    fn handoff_state_refuses_a_vocabulary_it_does_not_own() {
        for value in ["open", "handoff", "withdrawn", ""] {
            assert_eq!(
                value.parse::<ThreadHandoffState>(),
                Err(format!("invalid thread handoff state '{value}'")),
            );
        }
    }

    /// A valid claim, which every command test below is one field away from.
    fn claim_command(actor: PrincipalId) -> ThreadHandoffCommand {
        ThreadHandoffCommand {
            company_id: Uuid::new_v4(),
            handoff_id: Uuid::new_v4(),
            command_id: Uuid::new_v4(),
            expected_version: 1,
            expected_generation: Uuid::new_v4(),
            operation: ThreadHandoffOperation::Claim,
            priority: BusinessPriority::Normal,
            due_at: None,
            actor_principal_id: actor,
            visible_channel_ids: vec![Uuid::new_v4()],
        }
    }

    /// Case 1.
    #[test]
    fn validate_accepts_all_four_operations_and_refuses_only_the_three_rules() {
        let actor = PrincipalId::random();
        let other = PrincipalId::random();
        for operation in [
            ThreadHandoffOperation::Claim,
            ThreadHandoffOperation::Release,
            ThreadHandoffOperation::Reassign { to: other },
            ThreadHandoffOperation::SetAttributes,
        ] {
            assert_eq!(
                ThreadHandoffCommand {
                    operation,
                    ..claim_command(actor)
                }
                .validate(),
                Ok(()),
                "{operation:?} is a valid command"
            );
        }

        for expected_version in [0, i64::MAX as u64 + 1, u64::MAX] {
            assert!(
                ThreadHandoffCommand {
                    expected_version,
                    ..claim_command(actor)
                }
                .validate()
                .is_err(),
                "version {expected_version} is outside what the column can hold"
            );
        }
        // The boundary itself is accepted, so the rule above is a bound and not a blanket refusal.
        assert_eq!(
            ThreadHandoffCommand {
                expected_version: i64::MAX as u64,
                ..claim_command(actor)
            }
            .validate(),
            Ok(())
        );

        assert!(
            ThreadHandoffCommand {
                expected_generation: Uuid::nil(),
                ..claim_command(actor)
            }
            .validate()
            .is_err(),
            "the nil UUID is not a generation any thread ever opened"
        );

        let self_reassign = ThreadHandoffCommand {
            operation: ThreadHandoffOperation::Reassign { to: actor },
            ..claim_command(actor)
        }
        .validate()
        .expect_err("reassigning to yourself is a claim");
        assert!(self_reassign.contains("claim"), "{self_reassign}");
    }

    /// Case 2.
    #[test]
    fn the_audited_operation_is_what_the_responsibility_actually_did() {
        let actor = PrincipalId::random();
        let other = PrincipalId::random();
        use ThreadHandoffOperation::{Claim, Reassign, Release, SetAttributes};

        assert_eq!(
            thread_handoff_operation_name(None, Some(actor), Claim),
            "claimed"
        );
        assert_eq!(
            thread_handoff_operation_name(Some(actor), None, Release),
            "released"
        );
        assert_eq!(
            thread_handoff_operation_name(Some(actor), Some(other), Reassign { to: other }),
            "reassigned"
        );
        assert_eq!(
            thread_handoff_operation_name(Some(actor), Some(actor), SetAttributes),
            "attributes_changed"
        );
        assert_eq!(
            thread_handoff_operation_name(None, None, SetAttributes),
            "attributes_changed"
        );
        assert_eq!(
            thread_handoff_operation_name(Some(actor), Some(actor), Claim),
            "attributes_changed",
            "claiming what is already yours moves nothing, so it is not a second claim"
        );
        assert_eq!(
            thread_handoff_operation_name(None, Some(other), Reassign { to: other }),
            "reassigned",
            "a manager handing out unclaimed work is not that person claiming it"
        );
    }

    #[test]
    fn the_operation_is_tagged_by_kind_and_refuses_a_kind_it_does_not_know() {
        assert_eq!(
            serde_json::to_value(ThreadHandoffOperation::Claim).unwrap(),
            serde_json::json!({ "kind": "claim" })
        );
        let target = PrincipalId::random();
        assert_eq!(
            serde_json::to_value(ThreadHandoffOperation::Reassign { to: target }).unwrap(),
            serde_json::json!({ "kind": "reassign", "to": target.as_uuid() })
        );
        assert_eq!(
            serde_json::from_value::<ThreadHandoffOperation>(
                serde_json::json!({ "kind": "reassign", "to": target.as_uuid() })
            )
            .unwrap(),
            ThreadHandoffOperation::Reassign { to: target }
        );
        for body in [
            serde_json::json!({ "kind": "resolve" }),
            serde_json::json!({ "kind": "reassign" }),
            serde_json::json!({}),
        ] {
            assert!(
                serde_json::from_value::<ThreadHandoffOperation>(body.clone()).is_err(),
                "{body} must be a deserialization error, never a default"
            );
        }
    }

    #[test]
    fn exactly_needs_instruction_and_draft_ready_are_actionable() {
        for (state, actionable, open) in [
            (ThreadHandoffState::NeedsInstruction, true, true),
            (ThreadHandoffState::Drafting, false, true),
            (ThreadHandoffState::DraftReady, true, true),
            (ThreadHandoffState::Resolved, false, false),
            (ThreadHandoffState::Dismissed, false, false),
        ] {
            assert_eq!(state.is_actionable(), actionable, "{state:?}");
            assert_eq!(state.is_open(), open, "{state:?}");
        }
    }
}
