//! The application's answer to [`HarnessApprovals`]: park the run and mail a human, unless a
//! prior decision or an explicit policy says otherwise.
//!
//! This is an authorization boundary, and every uncertain case in it resolves towards the human.
//! `src/application/AGENTS.md`: *"Tool approval remains the security boundary: select an approver
//! by an explicit deterministic policy, propagate directory lookup errors, and fail closed on
//! malformed approval configuration."*
//!
//! Nothing here names a runtime. The harness adapter translates its own approval callback into an
//! [`ApprovalAsk`] and this decides it, which is what keeps the approval mail, the step-key hash
//! and the delegation policy in one place no matter what is executing the agent.

use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

use crate::app_error::AppResult;
use crate::entities::{
    approval::{ApprovalAction, ApprovalStatus, ApprovalSubject},
    tool_catalogue::OUTREACH_TOOL_ID,
    transport::ChannelSelector,
};
use crate::use_cases::{
    approval::ApprovalUseCases,
    channel::{ChannelPersistence, InternalTargetOutcome, resolve_internal_target},
};

use super::ports::{ApprovalAsk, ApprovalTrigger, ApprovalVerdict, HarnessApprovals};

/// Everything needed to decide whether one outreach call is purely internal, and whether that
/// earns it a pass on human approval.
///
/// Approval is keyed by tool ID, so the outreach tool alone cannot distinguish "ask a colleague"
/// from "mail a stranger". This carries the resolved answer instead: the recipients are classified
/// against the channel directory, not taken on the model's word.
#[derive(Clone)]
pub struct InternalDelegationPolicy {
    pub channel_persistence: Arc<dyn ChannelPersistence>,
    pub company_id: Uuid,
    pub source_channel_id: Uuid,
    /// When false, a call whose recipients are *all* same-company agent channels skips the human.
    /// Defaults to true, so behaviour is unchanged until an operator opts in.
    pub requires_approval: bool,
}

/// Read `tool_security.tools.outreach_and_await_quorum.config.internal_requires_approval`.
///
/// Absent, malformed, or non-boolean all mean `true`: this gates outbound mail, so anything other
/// than an explicit `false` fails closed.
pub fn internal_requires_approval(config: &serde_json::Value) -> bool {
    config
        .get("tool_security")
        .and_then(|v| v.get("tools"))
        .and_then(|v| v.get(OUTREACH_TOOL_ID))
        .and_then(|v| v.get("config"))
        .and_then(|v| v.get("internal_requires_approval"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

impl InternalDelegationPolicy {
    /// Whether this trigger is an outreach call whose every recipient is a callable same-company
    /// agent channel, and policy lets such a call skip the human.
    ///
    /// Every uncertain value answers `false`. Persistence failures are different: they propagate
    /// so the task can retry instead of being parked behind an approval request that may not have
    /// been durably created.
    async fn approves_without_human(&self, trigger: &ApprovalTrigger<'_>) -> AppResult<bool> {
        if self.requires_approval {
            return Ok(false);
        }
        let ApprovalTrigger::Tool { name, args } = trigger else {
            return Ok(false);
        };
        if *name != OUTREACH_TOOL_ID {
            return Ok(false);
        }
        let Some(targets) = args.get("target_channels").and_then(|v| v.as_array()) else {
            return Ok(false);
        };
        if args
            .get("target_emails")
            .and_then(|value| value.as_array())
            .is_some_and(|targets| !targets.is_empty())
        {
            return Ok(false);
        }
        // An empty list is not "all internal"; it is a malformed call.
        if targets.is_empty() {
            return Ok(false);
        }

        for target in targets {
            let Some(value) = target.as_str() else {
                return Ok(false);
            };
            let Ok(selector) = ChannelSelector::parse(value) else {
                return Ok(false);
            };
            let outcome = resolve_internal_target(
                &selector,
                self.company_id,
                self.source_channel_id,
                self.channel_persistence.as_ref(),
            )
            .await?;
            match outcome {
                InternalTargetOutcome::Callable(_) => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
}

pub struct AgentApprovalHandler {
    pub approval_use_cases: Arc<ApprovalUseCases>,
    pub context: ApprovalSubject,
    /// `None` when the run has no outreach tool, so nothing can be auto-approved.
    pub delegation: Option<InternalDelegationPolicy>,
}

#[async_trait]
impl HarnessApprovals for AgentApprovalHandler {
    async fn decide(&self, ask: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        // Ahead of the approver check on purpose: delegating to a colleague needs no approver, and
        // a coordinator channel with no configured participant must still be able to do it.
        if let Some(policy) = self.delegation.as_ref()
            && policy.approves_without_human(&ask.trigger).await?
        {
            info!("Outreach targets only same-company agent channels; approval not required");
            return Ok(ApprovalVerdict::Approved);
        }

        if self.context.approver_email.trim().is_empty() {
            return Ok(ApprovalVerdict::rejected(
                "No channel participant or company team member is configured to approve this action.",
            ));
        }

        let step_raw = step_text(&ask.trigger);
        let step_key = self.step_key(&step_raw);

        match self
            .approval_use_cases
            .check_step_approval(
                self.context.company_id,
                self.context.channel_id,
                self.context.thread_id,
                &step_key,
            )
            .await?
        {
            Some(ApprovalStatus::Approved) => return Ok(ApprovalVerdict::Approved),
            Some(ApprovalStatus::Rejected) => {
                return Ok(ApprovalVerdict::rejected(
                    "Approval previously rejected by human",
                ));
            }
            Some(ApprovalStatus::Pending | ApprovalStatus::Expired) | None => {}
        }

        let action_summary = if ask.message.is_empty() {
            step_raw.clone()
        } else {
            ask.message.to_string()
        };
        let payload = serde_json::json!({
            "trigger": ask.trigger,
            "context": ask.context,
        });

        let approval = self
            .approval_use_cases
            .create_and_send_approval_request(
                &self.context,
                ApprovalAction {
                    step_key,
                    action_type: ask.trigger.kind().to_string(),
                    title: action_title(&ask.trigger),
                    summary: action_summary,
                    payload,
                },
            )
            .await?;

        Ok(match approval.status {
            ApprovalStatus::Approved => ApprovalVerdict::Approved,
            ApprovalStatus::Rejected => {
                ApprovalVerdict::rejected("Approval previously rejected by human")
            }
            ApprovalStatus::Pending | ApprovalStatus::Expired => ApprovalVerdict::pending(
                "Approval requested via email link; task paused waiting for human decision.",
            ),
        })
    }
}

impl AgentApprovalHandler {
    /// The identity of one approval *step*, stable across process restarts.
    ///
    /// It is what lets a second server instance recognise a decision the first one asked for, so
    /// it must stay derived from the same three things in the same order: the durable task, the
    /// thread, and the exact action. Changing this silently re-asks for every approval already
    /// granted.
    fn step_key(&self, step_raw: &str) -> String {
        let thread = self.context.thread_id;
        let task = self
            .context
            .suspension
            .map(|suspension| suspension.task_id().to_string())
            .unwrap_or_default();
        format!(
            "{:x}",
            Sha256::digest(format!("{task}:{thread}:{step_raw}").as_bytes())
        )
    }
}

/// The action, rendered so that two identical actions hash alike and two different ones do not.
///
/// Not a display string -- it feeds [`AgentApprovalHandler::step_key`], so its exact shape is
/// load-bearing for every approval already stored.
fn step_text(trigger: &ApprovalTrigger<'_>) -> String {
    match trigger {
        ApprovalTrigger::Tool { name, args } => format!(
            "tool:{}:{}",
            name,
            serde_json::to_string(args).unwrap_or_default()
        ),
        ApprovalTrigger::Condition { name, matched } => format!("condition:{name}:{matched}"),
        ApprovalTrigger::State { from, to } => format!("state:{from:?}:{to}"),
    }
}

/// The subject line a human sees in the approval mail.
fn action_title(trigger: &ApprovalTrigger<'_>) -> String {
    match trigger {
        ApprovalTrigger::Tool { name, .. } => format!("Tool Execution: {name}"),
        ApprovalTrigger::Condition { name, .. } => format!("Condition Approval: {name}"),
        ApprovalTrigger::State { to, .. } => format!("State Transition: {to}"),
    }
}

#[cfg(test)]
#[path = "approvals_tests.rs"]
mod tests;
