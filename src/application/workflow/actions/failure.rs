//! One classification for every action-owned workflow step (`http.request`, `tool.call`,
//! `mcp.call`, `message.send`, `message.reply`). The handler always reports `SafeToRetry`: the shared database predicate
//! `workflow_action_retry_safe` is the only source of `EffectOutcomeUnknown`. It keeps this
//! label when the execution has no possible-dispatch marker (nothing can have been sent),
//! and replaces it with the marker/receipt/evidence truth whenever a marker exists.
use crate::app_error::AppError;
use crate::application::workflow::lease::WorkflowFailure;
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

/// Step kinds whose only effect path is ActionService's marker-first protocol.
///
/// Invariant (04.10-D3, F011): every `WorkflowHandler` that supports one of these kinds
/// must perform effects solely through `ActionService` remote/local dispatch, so that no
/// effect-bearing I/O happens before a committed possible-dispatch marker. Then "no marker
/// for this execution" proves "no effect", and an interrupted attempt without a marker may
/// retry. A new handler or tool bridge for these kinds (phases 06/08) must keep the
/// invariant or remove its kind from this list first.
///
/// Messaging joins (04.11-D2 §5.2): its only effect-bearing write is the publication
/// transaction, whose first write is the `message` marker, and provider I/O happens only in
/// the delivery engine after that commit.
///
/// `agent.run` joins (06-D4): its only effects are saved model tool calls dispatched through
/// the same ActionService entries, and its provider calls are charged checkpoint
/// reservations, not effects; an unresolved reservation stays charged across a retry.
pub fn marker_owned_effect_kind(kind: &str) -> bool {
    matches!(
        kind,
        "http.request" | "tool.call" | "mcp.call" | "message.send" | "message.reply" | "agent.run"
    )
}

/// Step kinds with no external effect (06-D6 E): pure, wait and decision kinds, bounded
/// reads, and the tool-free model steps, whose provider calls are ledger-charged
/// reservations (an unresolved one stays charged across a retry), not effects.
pub fn effect_free_kind(kind: &str) -> bool {
    matches!(
        kind,
        "data.map"
            | "decision.rule"
            | "decision.human"
            | "decision.agent"
            | "context.history"
            | "memory.load"
            | "ai.classify"
            | "jev.evaluate"
            | "wait.event"
            | "wait.timer"
    )
}

/// The one predicate for Rust lease retirement and pending recovery: without a
/// possible-dispatch marker, these kinds cannot have produced an unknown effect.
pub fn retry_safe_without_marker(kind: &str) -> bool {
    effect_free_kind(kind) || marker_owned_effect_kind(kind)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionFamily {
    Http,
    Mcp,
    Message,
}
impl ActionFamily {
    fn prefix(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Mcp => "mcp",
            Self::Message => "message",
        }
    }
}

/// Every non-success result of one action-owned step attempt.
#[derive(Debug)]
pub enum ActionStepOutcome<'a> {
    /// Failed before `dispatch_remote`: request, intent or (MCP) pre-reservation checks.
    /// Messaging also reports a publication refused while no marker exists this way.
    Preparation(&'a AppError),
    /// `dispatch_remote` failed: reservation refusal, ambiguity, provider or post-entry error.
    Dispatch(&'a AppError),
    ApprovalRequired,
    PossibleDispatchExists,
    Interrupted,
    /// MCP pre-reservation transport failure: nothing was reserved or sent.
    Unavailable,
    /// MCP pre-reservation discovery no longer matches the frozen tool contract.
    ContractChanged,
    /// A committed MCP receipt whose result reports `isError: true`.
    ToolError,
    /// Message admission refused at `MAX_MESSAGE_ACTIONS_PER_EXECUTION`; nothing was written.
    ActionLimit,
}

pub fn action_step_failure(
    family: ActionFamily,
    outcome: ActionStepOutcome<'_>,
) -> WorkflowFailure {
    use ActionStepOutcome::*;
    use FailureClass::{Retryable, Terminal};
    let (class, suffix) = match outcome {
        Preparation(
            AppError::NotFound(_) | AppError::BadRequest(_) | AppError::InvalidCredentials,
        ) => (Terminal, "rejected"),
        Preparation(_) | Dispatch(_) | PossibleDispatchExists => (Retryable, "unresolved"),
        ApprovalRequired => (Terminal, "approval_required"),
        Interrupted => (Retryable, "interrupted"),
        Unavailable => (Retryable, "unavailable"),
        ContractChanged => (Terminal, "contract_changed"),
        ToolError => (Terminal, "tool_error"),
        ActionLimit => (Terminal, "action_limit"),
    };
    let code = FailureCode::parse(format!("{}.{suffix}", family.prefix()))
        .expect("static action failure code");
    WorkflowFailure {
        failure: StepFailure::new(class, code, None).expect("bounded action failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(family: ActionFamily, outcome: ActionStepOutcome<'_>) -> (String, FailureClass) {
        let failure = action_step_failure(family, outcome);
        assert_eq!(failure.safety, RetrySafety::SafeToRetry);
        (
            failure.failure.code().as_str().into(),
            failure.failure.class(),
        )
    }

    #[test]
    fn workflow_action_failure_mapping_is_shared_and_never_claims_unknown() {
        use ActionStepOutcome::*;
        use FailureClass::{Retryable, Terminal};
        let not_found = AppError::NotFound("x".into());
        let bad = AppError::BadRequest("x".into());
        let conflict = AppError::Conflict("x".into());
        let database = AppError::Database("x".into());
        for family in [ActionFamily::Http, ActionFamily::Mcp] {
            let p = family.prefix();
            assert_eq!(
                code(family, Preparation(&not_found)),
                (format!("{p}.rejected"), Terminal)
            );
            assert_eq!(
                code(family, Preparation(&bad)),
                (format!("{p}.rejected"), Terminal)
            );
            assert_eq!(
                code(family, Preparation(&AppError::InvalidCredentials)),
                (format!("{p}.rejected"), Terminal)
            );
            assert_eq!(
                code(family, Preparation(&conflict)),
                (format!("{p}.unresolved"), Retryable)
            );
            assert_eq!(
                code(family, Preparation(&database)),
                (format!("{p}.unresolved"), Retryable)
            );
            // Only preparation errors may be rejected: dispatch errors can follow a marker.
            for error in [&not_found, &bad, &conflict] {
                assert_eq!(
                    code(family, Dispatch(error)),
                    (format!("{p}.unresolved"), Retryable)
                );
            }
            assert_eq!(
                code(family, ApprovalRequired),
                (format!("{p}.approval_required"), Terminal)
            );
            assert_eq!(
                code(family, PossibleDispatchExists),
                (format!("{p}.unresolved"), Retryable)
            );
            assert_eq!(
                code(family, Interrupted),
                (format!("{p}.interrupted"), Retryable)
            );
        }
        assert_eq!(
            code(ActionFamily::Mcp, Unavailable),
            ("mcp.unavailable".into(), Retryable)
        );
        assert_eq!(
            code(ActionFamily::Mcp, ContractChanged),
            ("mcp.contract_changed".into(), Terminal)
        );
        assert_eq!(
            code(ActionFamily::Mcp, ToolError),
            ("mcp.tool_error".into(), Terminal)
        );
        assert_eq!(
            code(ActionFamily::Message, ActionLimit),
            ("message.action_limit".into(), Terminal)
        );
        assert_eq!(
            code(ActionFamily::Message, Preparation(&bad)),
            ("message.rejected".into(), Terminal)
        );
        assert_eq!(
            code(ActionFamily::Message, ApprovalRequired),
            ("message.approval_required".into(), Terminal)
        );
    }

    #[test]
    fn workflow_action_marker_owned_kinds_are_exactly_the_action_service_owners() {
        // 04.11-D2 §5.2: messaging deliberately joins; its marker is the first publication write.
        for kind in [
            "http.request",
            "tool.call",
            "mcp.call",
            "message.send",
            "message.reply",
            "agent.run",
        ] {
            assert!(marker_owned_effect_kind(kind));
        }
        for kind in ["memory.save", "context.history", "data.map"] {
            assert!(!marker_owned_effect_kind(kind), "{kind}");
        }
    }

    #[test]
    fn workflow_effect_free_kinds_are_exactly_the_reads_pure_waits_and_model_steps() {
        let unknown = ["memory.save", "workflow.call", "flow.repeat"];
        for kind in crate::application::workflow::registry::TYPES {
            let effect_free = effect_free_kind(kind);
            assert!(
                !(effect_free && marker_owned_effect_kind(kind)),
                "{kind} is in both sets"
            );
            assert_eq!(
                retry_safe_without_marker(kind),
                !unknown.contains(&kind),
                "{kind}"
            );
        }
        for kind in ["context.history", "ai.classify", "decision.agent"] {
            assert!(effect_free_kind(kind), "{kind}");
        }
    }
}
