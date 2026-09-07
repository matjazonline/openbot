//! Owner-only task transfer/release tool for a running agent.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    entities::{
        task::{
            TaskOwner, TaskOwnerTarget, TaskOwnershipActor, TaskOwnershipAuthority,
            TaskOwnershipCommand, TaskOwnershipOperation, TaskOwnershipReason,
        },
        value_objects::ToolId,
    },
    services::harness::{NativeToolDeclaration, NativeToolSafety, ToolInvocation},
    task_queue::TaskPersistence,
};

pub use crate::entities::tool_catalogue::TASK_OWNERSHIP_TOOL_ID;

#[derive(Debug, Clone)]
pub struct TaskOwnershipToolContext {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub lease: crate::entities::task::TaskLeaseRef,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OwnershipToolOperation {
    Transfer,
    Release,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OwnershipTargetKind {
    Agent,
    Human,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OwnershipToolReason {
    Delegated,
    WorkloadRebalance,
    OwnerUnavailable,
    Released,
}

impl From<OwnershipToolReason> for TaskOwnershipReason {
    fn from(value: OwnershipToolReason) -> Self {
        match value {
            OwnershipToolReason::Delegated => Self::Delegated,
            OwnershipToolReason::WorkloadRebalance => Self::WorkloadRebalance,
            OwnershipToolReason::OwnerUnavailable => Self::OwnerUnavailable,
            OwnershipToolReason::Released => Self::Released,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OwnershipToolInput {
    operation: OwnershipToolOperation,
    /// Agent id for an agent target, or account user id for a human target.
    target_id: Option<String>,
    target_kind: Option<OwnershipTargetKind>,
    /// Required for transfer; private and never inserted into message history.
    handoff_instruction: Option<String>,
    reason: Option<OwnershipToolReason>,
    reason_detail: Option<String>,
}

pub struct TaskOwnershipTool {
    persistence: Arc<dyn TaskPersistence>,
    context: TaskOwnershipToolContext,
}

impl TaskOwnershipTool {
    pub fn new(persistence: Arc<dyn TaskPersistence>, context: TaskOwnershipToolContext) -> Self {
        Self {
            persistence,
            context,
        }
    }

    pub fn declaration() -> NativeToolDeclaration {
        NativeToolDeclaration {
            id: ToolId::from(TASK_OWNERSHIP_TOOL_ID),
            name: "Transfer or Release Task",
            description: "End your current run by transferring this task to an eligible agent or teammate, or release it as unassigned. A transfer requires a private handoff instruction.",
            input_schema: serde_json::to_value(schemars::schema_for!(OwnershipToolInput))
                .unwrap_or_else(|_| serde_json::json!({})),
            safety: NativeToolSafety {
                read_only: false,
                concurrency_safe: false,
                has_external_effect: false,
                requires_network: false,
                destructive: false,
                open_world: false,
                requires_approval_by_default: false,
                max_output_chars: 2_000,
                max_result_chars: 4_000,
            },
        }
    }

    pub async fn call(&self, call_id: &str, args: serde_json::Value) -> ToolInvocation {
        let input: OwnershipToolInput = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => return ToolInvocation::failure(format!("Invalid input: {error}")),
        };
        let TaskOwner::Agent(actor) = self.context.lease.claimed_owner else {
            return ToolInvocation::failure("Only the task's owning agent may use this tool.");
        };
        let (operation, new_owner, default_reason) = match input.operation {
            OwnershipToolOperation::Release => {
                if input.target_id.is_some() || input.target_kind.is_some() {
                    return ToolInvocation::failure("Release does not accept a target.");
                }
                (
                    TaskOwnershipOperation::Release,
                    TaskOwner::Unassigned,
                    TaskOwnershipReason::Released,
                )
            }
            OwnershipToolOperation::Transfer => {
                let Some(target_id) = input.target_id else {
                    return ToolInvocation::failure("Transfer requires target_id.");
                };
                let target_id = match Uuid::parse_str(target_id.trim()) {
                    Ok(id) => id,
                    Err(error) => {
                        return ToolInvocation::failure(format!(
                            "target_id must be a UUID: {error}"
                        ));
                    }
                };
                let Some(target_kind) = input.target_kind else {
                    return ToolInvocation::failure("Transfer requires target_kind.");
                };
                let selector = match target_kind {
                    OwnershipTargetKind::Agent => TaskOwnerTarget::Agent(target_id),
                    OwnershipTargetKind::Human => TaskOwnerTarget::HumanUser(target_id),
                };
                let owner = match self
                    .persistence
                    .resolve_task_owner_target(
                        self.context.company_id,
                        self.context.channel_id,
                        selector,
                    )
                    .await
                {
                    Ok(Some(owner)) => owner,
                    Ok(None) => {
                        return ToolInvocation::failure(
                            "The requested owner is not eligible for this task.",
                        );
                    }
                    Err(error) => {
                        return ToolInvocation::failure(format!(
                            "Could not resolve the requested owner: {error}"
                        ));
                    }
                };
                (
                    TaskOwnershipOperation::Transfer,
                    owner,
                    TaskOwnershipReason::Delegated,
                )
            }
        };
        let command = TaskOwnershipCommand {
            task_id: self.context.lease.task_id,
            company_id: self.context.company_id,
            command_id: stable_command_id(self.context.lease.execution_generation, call_id),
            expected_version: self.context.lease.ownership_version,
            actor: TaskOwnershipActor {
                principal_id: actor,
                authority: TaskOwnershipAuthority::CurrentOwner,
            },
            operation,
            new_owner,
            reason: input.reason.map(Into::into).unwrap_or(default_reason),
            reason_detail: input.reason_detail,
            handoff_instruction: input.handoff_instruction,
        };
        match self.persistence.change_task_ownership(command).await {
            Ok(event) => ToolInvocation::suspended(serde_json::json!({
                "task_id": event.task_id,
                "operation": event.operation.as_str(),
                "ownership_version": event.to_version,
                "owner_kind": event.new_owner.as_str(),
                "run_ended": true,
            })),
            Err(error) => ToolInvocation::failure(format!("Ownership change failed: {error}")),
        }
    }
}

fn stable_command_id(execution_generation: Uuid, call_id: &str) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(execution_generation.as_bytes());
    digest.update([0]);
    digest.update(call_id.as_bytes());
    let bytes: [u8; 16] = digest.finalize()[..16]
        .try_into()
        .expect("a SHA-256 digest always contains sixteen bytes");
    Uuid::from_bytes(bytes)
}
