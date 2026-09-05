//! The `ai-agents` side of [`HarnessApprovals`].
//!
//! Translation only, in both directions: an `ApprovalRequest` becomes an [`ApprovalAsk`] and an
//! [`ApprovalVerdict`] becomes an `ApprovalResult`. Every decision -- the delegation exemption,
//! the step-key hash, the prior-decision lookup and the approval mail -- lives in
//! `services::harness::approvals`, so a second harness reuses all of it by writing its own thirty
//! lines of this and nothing else.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    app_error::AppError,
    services::harness::{ApprovalAsk, ApprovalTrigger, ApprovalVerdict, HarnessApprovals},
};

pub struct AiAgentsApprovalShim {
    pub approvals: Arc<dyn HarnessApprovals>,
    pub suspended: Arc<AtomicBool>,
    /// The runtime callback cannot return an application error. Preserve it out-of-band inside
    /// this adapter so `AgentHarness::run` can return the typed failure after chat unwinds.
    pub failure: Arc<Mutex<Option<AppError>>>,
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
            Ok(ApprovalVerdict::Pending { reason }) => {
                self.suspended.store(true, Ordering::SeqCst);
                ai_agents::hitl::ApprovalResult::rejected_with_reason(reason)
            }
            Ok(ApprovalVerdict::Rejected { reason }) => {
                ai_agents::hitl::ApprovalResult::rejected_with_reason(reason)
            }
            // Explicit rather than a `?`: an approval that could not be decided rejects, and
            // `src/AGENTS.md` wants that choice written down instead of collapsed into a default.
            // The exact persistence error is kept for the harness result, not shown to the model.
            Err(error) => {
                *self
                    .failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
                ai_agents::hitl::ApprovalResult::rejected_with_reason(
                    "Approval could not be decided; aborting this run.",
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    use crate::app_error::AppResult;

    struct FixedApproval(AppResult<ApprovalVerdict>);

    #[async_trait::async_trait]
    impl HarnessApprovals for FixedApproval {
        async fn decide(&self, _ask: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
            match &self.0 {
                Ok(verdict) => Ok(verdict.clone()),
                Err(error) => Err(AppError::Database(error.to_string())),
            }
        }
    }

    fn request() -> ai_agents::hitl::ApprovalRequest {
        ai_agents::hitl::ApprovalRequest::new(
            ai_agents::hitl::ApprovalTrigger::tool("outreach", serde_json::json!({})),
            "approve outreach",
        )
    }

    fn shim(result: AppResult<ApprovalVerdict>) -> AiAgentsApprovalShim {
        AiAgentsApprovalShim {
            approvals: Arc::new(FixedApproval(result)),
            suspended: Arc::new(AtomicBool::new(false)),
            failure: Arc::new(Mutex::new(None)),
        }
    }

    #[tokio::test]
    async fn only_a_pending_decision_suspends_the_run() {
        use ai_agents::hitl::ApprovalHandler;

        let pending = shim(Ok(ApprovalVerdict::pending("waiting")));
        assert!(pending.request_approval(request()).await.is_rejected());
        assert!(pending.suspended.load(Ordering::SeqCst));

        let rejected = shim(Ok(ApprovalVerdict::rejected("denied")));
        assert!(rejected.request_approval(request()).await.is_rejected());
        assert!(!rejected.suspended.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn an_application_failure_is_preserved_for_the_harness_result() {
        use ai_agents::hitl::ApprovalHandler;

        let failed = shim(Err(AppError::Database("approval store unavailable".into())));
        let result = failed.request_approval(request()).await;
        assert!(result.is_rejected());
        let ai_agents::hitl::ApprovalResult::Rejected { reason } = result else {
            unreachable!("the result was checked above")
        };
        assert_eq!(
            reason.as_deref(),
            Some("Approval could not be decided; aborting this run.")
        );
        assert!(!failed.suspended.load(Ordering::SeqCst));
        assert!(matches!(
            *failed.failure.lock().unwrap(),
            Some(AppError::Database(_))
        ));
    }
}
