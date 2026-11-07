//! Immutable final artifacts and current publication ownership, scoped before decoding.
use super::*;
use crate::application::agent::{ChannelFinalOrigin, ToolCallOrigin};
use crate::application::workflow::actions::{ActionScope, FrozenAction};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::channel_reply::*;
use crate::application::workflow::message::{
    MessageArguments, MessageKind, MessageRoutes, is_trusted_message_snapshot,
};
use crate::domain::workflow::{ActionInvocationId, ReplyIntegrity, ReplyProposalId};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgConnection;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicationState {
    Pending,
    Claimed,
    Published,
    Failed,
}
impl TryFrom<String> for PublicationState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "pending" => Ok(Self::Pending),
            "claimed" => Ok(Self::Claimed),
            "published" => Ok(Self::Published),
            "failed" => Ok(Self::Failed),
            _ => Err("Invalid automatic publication state"),
        }
    }
}

/// Immutable final identity is not a publication lease or a workflow fence.
#[derive(Clone, Copy)]
pub(super) struct FinalIdentity {
    pub company: CompanyId,
    pub run: RunId,
    pub proposal: ReplyProposalId,
    pub integrity: ReplyIntegrity,
}
impl From<ReplyPublicationClaim> for FinalIdentity {
    fn from(claim: ReplyPublicationClaim) -> Self {
        Self {
            company: claim.company,
            run: claim.run,
            proposal: claim.proposal,
            integrity: claim.integrity,
        }
    }
}

pub(super) enum ReadLock {
    Observe,
    Publish,
}

pub(super) struct PublicationArtifact {
    pub action: FrozenAction,
    pub draft: ReplyPublicationDraft,
    pub recipients: Vec<super::knowledge_release::MessageRecipient>,
}

#[derive(sqlx::FromRow)]
pub(super) struct PublicationRow {
    pub company_id: Uuid,
    pub run_id: Uuid,
    pub execution_id: Uuid,
    pub job_id: Uuid,
    pub intent_id: Uuid,
    pub invocation_id: Uuid,
    pub approval_decision_id: Option<Uuid>,
    pub approval_deadline: Option<DateTime<Utc>>,
    pub approval_policy: Option<Value>,
    pub recipients: Value,
    pub content: Value,
    pub route_snapshot: Value,
    pub operation: Value,
    #[sqlx(try_from = "String")]
    pub integrity: ReplyIntegrity,
    #[sqlx(try_from = "String")]
    pub state: PublicationState,
    pub live: bool,
    pub worker_id: Option<Uuid>,
    pub generation: Option<Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub attempts: i32,
    pub message_id: Option<Uuid>,
    pub delivery_id: Option<Uuid>,
    pub diagnostic_code: Option<String>,
}

const SELECT: &str = "SELECT final_reply.company_id,final_reply.run_id,final_reply.execution_id,
    proposal.job_id,proposal.intent_id,proposal.invocation_id,proposal.approval_decision_id,
    proposal.approval_deadline,proposal.approval_policy,proposal.recipients,proposal.content,
    proposal.route_snapshot,intent.operation,final_reply.integrity,final_reply.state,
    (final_reply.state='claimed' AND final_reply.lease_expires_at>clock_timestamp()) AS live,
    final_reply.worker_id,final_reply.generation,final_reply.lease_expires_at,final_reply.attempts,
    final_reply.message_id,final_reply.delivery_id,final_reply.diagnostic_code
    FROM channel_reply_decisions AS final_reply
    JOIN channel_reply_proposals AS proposal ON proposal.company_id=final_reply.company_id
        AND proposal.run_id=final_reply.run_id AND proposal.id=final_reply.proposal_id
    JOIN workflow_action_intents AS intent ON intent.company_id=proposal.company_id AND intent.id=proposal.invocation_id
    WHERE final_reply.company_id=$1 AND final_reply.run_id=$2 AND final_reply.proposal_id=$3
        AND final_reply.integrity=$4 AND final_reply.disposition='automatic'";

pub(super) async fn read_on(
    db: &mut PgConnection,
    identity: FinalIdentity,
    lock: ReadLock,
) -> AppResult<Option<PublicationRow>> {
    let lock = match lock {
        ReadLock::Observe => "",
        ReadLock::Publish => " FOR UPDATE OF final_reply",
    };
    Ok(sqlx::query_as(&format!("{SELECT}{lock}"))
        .bind(identity.company.as_uuid())
        .bind(identity.run.as_uuid())
        .bind(identity.proposal.as_uuid())
        .bind(identity.integrity.as_hex())
        .fetch_optional(db)
        .await?)
}

impl PublicationRow {
    pub fn scope(&self) -> ActionScope {
        ActionScope {
            company: CompanyId::new(self.company_id),
            run: RunId::new(self.run_id),
            execution: ExecutionId::new(self.execution_id),
        }
    }

    pub fn activation(&self) -> ActivationRequest {
        let scope = self.scope();
        ActivationRequest {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
            job: crate::application::workflow::activation::WorkflowJobId(self.job_id),
        }
    }

    pub fn owned_by(&self, claim: ReplyPublicationClaim) -> bool {
        self.state == PublicationState::Claimed
            && self.live
            && self.integrity == claim.integrity
            && self.worker_id == Some(claim.worker.0)
            && self.generation == Some(claim.generation)
            && self.lease_expires_at == Some(claim.expires_at)
            && self.attempts == i32::from(claim.attempt)
    }

    pub fn receipt(&self) -> AppResult<ReplyPublicationReceipt> {
        Ok(ReplyPublicationReceipt {
            message: crate::entities::message::CanonicalMessageId::new(
                self.message_id.ok_or_else(invalid)?,
            ),
            delivery: self
                .delivery_id
                .map(crate::entities::transport::DeliveryId::new),
        })
    }

    pub fn failure(&self) -> AppResult<ReplyPublicationFailure> {
        serde_json::from_value(Value::String(
            self.diagnostic_code.clone().ok_or_else(invalid)?,
        ))
        .map_err(|_| invalid())
    }

    pub fn action(&self, identity: FinalIdentity) -> AppResult<FrozenAction> {
        let action = FrozenAction::restore(self.operation.clone())?;
        if action.scope() != self.scope()
            || self.scope().company != identity.company
            || self.scope().run != identity.run
            || self.integrity != identity.integrity
            || action.request().operation_key.as_str() != "message.reply"
            || !is_trusted_message_snapshot(&action.request().contract)?
            || !matches!(&action.request().context.origin, ToolCallOrigin::ChannelFinal(origin)
                if origin.proposal==identity.proposal && origin.integrity==identity.integrity)
        {
            return Err(invalid());
        }
        Ok(action)
    }

    pub fn draft(&self, identity: FinalIdentity) -> AppResult<ReplyPublicationDraft> {
        Ok(self.artifact(identity)?.draft)
    }

    pub fn artifact(&self, identity: FinalIdentity) -> AppResult<PublicationArtifact> {
        let action = self.action(identity)?;
        let MessageArguments::Reply { source, content } =
            MessageArguments::parse(MessageKind::Reply, &action.request().arguments)?
        else {
            return Err(invalid());
        };
        if json!({"subject":content.subject(),"body":content.body_text()}) != self.content {
            return Err(invalid());
        }
        let routes: MessageRoutes =
            serde_json::from_value(self.route_snapshot.clone()).map_err(|_| invalid())?;
        let draft = ReplyPublicationDraft {
            scope: self.scope(),
            origin: ChannelFinalOrigin {
                proposal: identity.proposal,
                integrity: identity.integrity,
            },
            invocation: ActionInvocationId::new(self.invocation_id),
            source,
            content,
            routes,
        };
        Ok(PublicationArtifact {
            action,
            draft,
            recipients: serde_json::from_value(self.recipients.clone()).map_err(|_| invalid())?,
        })
    }
}

pub(super) fn invalid() -> AppError {
    AppError::BadRequest("Invalid final reply publication artifact".into())
}
