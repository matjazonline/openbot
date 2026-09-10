//! Explicit checkpoints own their approval decision; never wrap them in an automatic tool gate.
use super::harness::{NativeToolDeclaration, NativeToolSafety, ToolInvocation};
use crate::{
    app_error::{AppError, AppResult},
    entities::tool_catalogue::REQUEST_APPROVAL_TOOL_ID,
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalProposal {
    #[schemars(length(min = 1, max = 120))]
    pub title: String,
    #[schemars(length(min = 1, max = 8000))]
    pub proposal: String,
}

impl ApprovalProposal {
    pub fn parse(args: Value) -> AppResult<Self> {
        if args.to_string().len() > 65_536 {
            return Err(invalid("Approval arguments exceed 64 KiB"));
        }
        let proposal: Self =
            serde_json::from_value(args).map_err(|_| invalid("Invalid approval proposal"))?;
        if proposal.title.trim().is_empty()
            || proposal.title.chars().count() > 120
            || proposal.proposal.trim().is_empty()
            || proposal.proposal.chars().count() > 8000
        {
            return Err(invalid(
                "Approval requires a title of 1–120 characters and proposal of 1–8000 characters",
            ));
        }
        Ok(proposal)
    }
}

/// Supplied only by a durable task execution owner. Step 6 implements the transaction/recovery
/// protocol behind this port: freeze the proposal under the trusted invocation identity, atomically
/// park with its notice, or return the saved decision. No delegation auto-approval is permitted.
/// The correlator locates the owner's saved invocation; it is not itself an approval key.
#[async_trait]
pub trait CheckpointApprovals: Send + Sync {
    async fn checkpoint(
        &self,
        correlation_id: &str,
        proposal: ApprovalProposal,
    ) -> AppResult<CheckpointDecision>;
}

pub enum CheckpointDecision {
    Approved,
    Suspended,
}

pub struct RequestApprovalTool {
    approvals: Arc<dyn CheckpointApprovals>,
}

impl RequestApprovalTool {
    pub fn new(approvals: Arc<dyn CheckpointApprovals>) -> Self {
        Self { approvals }
    }

    pub fn declaration() -> NativeToolDeclaration {
        NativeToolDeclaration {
            id: REQUEST_APPROVAL_TOOL_ID.into(),
            name: "Request human approval",
            description: "Present a concrete proposal for human approval and wait before continuing.",
            input_schema: serde_json::to_value(schemars::schema_for!(ApprovalProposal))
                .expect("static schema serializes"),
            safety: NativeToolSafety {
                read_only: false,
                concurrency_safe: false,
                has_external_effect: true,
                requires_network: false,
                destructive: false,
                open_world: false,
                requires_approval_by_default: false,
                max_output_chars: 1024,
                max_result_chars: 1024,
            },
        }
    }

    pub async fn call(&self, correlation_id: &str, args: Value) -> AppResult<ToolInvocation> {
        if correlation_id.is_empty()
            || correlation_id.len() > 256
            || !correlation_id.bytes().all(|c| c.is_ascii_graphic())
        {
            return Err(invalid("Invalid checkpoint correlation ID"));
        }
        let proposal = ApprovalProposal::parse(args)?;
        match self.approvals.checkpoint(correlation_id, proposal).await? {
            CheckpointDecision::Approved => {
                Ok(ToolInvocation::success(json!({"status":"approved"})))
            }
            CheckpointDecision::Suspended => Ok(ToolInvocation::suspended(Value::Null)),
        }
    }
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

/// The loop constructs this only after preparing the exact invocation. Model arguments cannot
/// select a task, recipient, approval identity, or callback.
pub struct ScopedCheckpointApprovals {
    pub approvals: Arc<dyn super::harness::HarnessApprovals>,
    pub invocation: super::harness::runs::InvocationRef,
}

#[async_trait]
impl CheckpointApprovals for ScopedCheckpointApprovals {
    async fn checkpoint(
        &self,
        correlation_id: &str,
        proposal: ApprovalProposal,
    ) -> AppResult<CheckpointDecision> {
        use super::harness::{ApprovalAsk, ApprovalTrigger, ApprovalVerdict};
        use sha2::{Digest, Sha256};
        if correlation_id != self.invocation.invocation_id.0.to_string() {
            return Err(invalid("Checkpoint invocation identity mismatch"));
        }
        let fingerprint = format!("{:x}", Sha256::digest(proposal.proposal.as_bytes()));
        let verdict = self
            .approvals
            .decide(ApprovalAsk {
                invocation: Some(self.invocation),
                trigger: ApprovalTrigger::Checkpoint {
                    invocation_id: self.invocation.invocation_id,
                    proposal_fingerprint: &fingerprint,
                    title: &proposal.title,
                    proposal: &proposal.proposal,
                },
                message: &proposal.proposal,
                context: &Value::Null,
            })
            .await?;
        match verdict {
            ApprovalVerdict::Approved => Ok(CheckpointDecision::Approved),
            ApprovalVerdict::Pending { .. } => Ok(CheckpointDecision::Suspended),
            ApprovalVerdict::Rejected { .. } => Err(AppError::Execution(
                crate::app_error::ExecutionFailure::CheckpointDenied,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proposal_schema_and_runtime_enforce_the_checkpoint_boundary() {
        let declaration = RequestApprovalTool::declaration();
        assert_eq!(declaration.input_schema["additionalProperties"], false);
        assert!(!declaration.safety.requires_approval_by_default);
        for args in [
            json!({"title":" ","proposal":"work"}),
            json!({"title":"ok","proposal":"x".repeat(8001)}),
            json!({"title":"ok","proposal":"work","approver":"model-chosen"}),
        ] {
            assert!(ApprovalProposal::parse(args).is_err());
        }
        assert!(
            ApprovalProposal::parse(json!({"title":"Review","proposal":"Concrete proposal"}))
                .is_ok()
        );
    }
}
