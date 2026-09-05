//! The `ai-agents` side of [`HarnessApprovals`].
//!
//! Translation only, in both directions: an `ApprovalRequest` becomes an [`ApprovalAsk`] and an
//! [`ApprovalVerdict`] becomes an `ApprovalResult`. Every decision -- the delegation exemption,
//! the step-key hash, the prior-decision lookup and the approval mail -- lives in
//! `services::harness::approvals`, so a second harness reuses all of it by writing its own thirty
//! lines of this and nothing else.

use std::sync::Arc;

use crate::services::harness::{ApprovalAsk, ApprovalTrigger, ApprovalVerdict, HarnessApprovals};

pub struct AiAgentsApprovalShim {
    pub approvals: Arc<dyn HarnessApprovals>,
}

#[async_trait::async_trait]
impl ai_agents::hitl::ApprovalHandler for AiAgentsApprovalShim {
    async fn request_approval(
        &self,
        req: ai_agents::hitl::ApprovalRequest,
    ) -> ai_agents::hitl::ApprovalResult {
        let trigger = match &req.trigger {
            ai_agents::hitl::ApprovalTrigger::Tool { name, args } => {
                ApprovalTrigger::Tool { name, args }
            }
            ai_agents::hitl::ApprovalTrigger::Condition { name, matched } => {
                ApprovalTrigger::Condition { name, matched }
            }
            ai_agents::hitl::ApprovalTrigger::State { from, to } => ApprovalTrigger::State {
                from: from.as_deref(),
                to,
            },
        };
        let context = serde_json::to_value(&req.context).unwrap_or(serde_json::Value::Null);
        let ask = ApprovalAsk {
            trigger,
            message: &req.message,
            context: &context,
        };

        match self.approvals.decide(ask).await {
            Ok(ApprovalVerdict::Approved) => ai_agents::hitl::ApprovalResult::Approved,
            Ok(ApprovalVerdict::Rejected { reason }) => {
                ai_agents::hitl::ApprovalResult::rejected_with_reason(reason)
            }
            // Explicit rather than a `?`: an approval that could not be decided rejects, and
            // `src/AGENTS.md` wants that choice written down instead of collapsed into a default.
            Err(error) => ai_agents::hitl::ApprovalResult::rejected_with_reason(error.to_string()),
        }
    }
}
