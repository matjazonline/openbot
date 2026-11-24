//! The terminal completion writer is the only producer of final response decisions.
use super::*;
use crate::application::agent::{ChannelFinalOrigin, ToolCallOrigin};
use crate::application::workflow::{
    actions::*, activation::*, batch::PureCompletion, channel_reply::*, decisions::DecisionNotices,
    message::*,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

#[derive(sqlx::FromRow)]
pub(super) struct RootReply {
    /// The response owner's key: the ingress intent, or the call's source message (AWC-P04-D1 D1).
    pub owner_id: Uuid,
    pub owner_kind: String,
    pub message_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
    pub thread_id: Uuid,
    pub actor_id: Uuid,
    pub contract: Value,
    pub approval_policy: Option<Value>,
    pub source: Value,
    pub deadline: DateTime<Utc>,
    binding_id: Uuid,
}

impl RootReply {
    /// A channel call owns this response, rather than ingress.
    pub(super) fn is_call(&self) -> bool {
        self.owner_kind == "call"
    }

    fn target(&self) -> AppResult<AddressTarget> {
        AddressTarget::from_parts(
            self.channel_id,
            self.principal_id
                .map(crate::entities::transport::PrincipalId::new),
        )
        .ok_or_else(invalid)
    }
}

pub(super) async fn root_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<RootReply>> {
    Ok(sqlx::query_as("SELECT owner.owner_id,owner.owner_kind,owner.message_id,owner.channel_id,owner.principal_id,owner.thread_id,owner.actor_id, \
        owner.contract,owner.approval_policy,owner.source,run.deadline,run.binding_id \
        FROM channel_reply_owners AS owner \
        JOIN workflow_runs AS run ON run.company_id=owner.company_id AND run.id=owner.run_id \
        WHERE owner.company_id=$1 AND owner.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(db).await?)
}

pub(super) fn pending_reply_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> impl std::future::Future<Output = Result<bool, sqlx::Error>> + '_ {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM channel_reply_runs AS owned \
         JOIN workflow_executions AS execution ON execution.company_id=owned.company_id AND execution.run_id=owned.run_id \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE owned.company_id=$1 AND owned.run_id=$2 AND execution.id=$3 AND job.id=$4 AND execution.completed_at IS NULL)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(scope.job.0)
    .fetch_one(db)
}

/// Account/company locks precede run locks; immutable completed facts need no live recipients.
pub(super) async fn authorize_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
) -> AppResult<()> {
    authorize_source_on(db, scope, root).await?;
    if !policy_current_on(db, scope, root).await? {
        return Err(AppError::BadRequest("Final response policy changed".into()));
    }
    Ok(())
}

pub(super) async fn authorize_source_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
) -> AppResult<()> {
    let actor = WorkflowActor::authenticated(root.actor_id)?;
    message::current_message_on(
        db,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
        actor,
        MessageKind::Reply,
    )
    .await?;
    management::authorize_association_on(
        db,
        scope.company,
        actor,
        RelatedAssociation::Thread {
            target: root.target()?,
            thread_id: RelatedThreadId::new(root.thread_id),
        },
    )
    .await?;
    Ok(())
}

pub(super) async fn policy_current_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
) -> AppResult<bool> {
    // A call reviews under the policy its record captured; a later desk edit neither fails nor
    // replaces it (AWC-P04-D1 r2 D8).
    if root.is_call() {
        return Ok(true);
    }
    let current: Option<Option<Value>> = sqlx::query_scalar(
        "SELECT approval_policy FROM channel_reply_settings \
        WHERE company_id=$1 AND channel_id IS NOT DISTINCT FROM $2::uuid \
            AND principal_id IS NOT DISTINCT FROM $3::uuid AND binding_id IS NOT NULL FOR SHARE",
    )
    .bind(scope.company.as_uuid())
    .bind(root.channel_id)
    .bind(root.principal_id)
    .fetch_optional(&mut *db)
    .await?;
    Ok(current.as_ref() == Some(&root.approval_policy))
}

pub(super) enum PreparedFinal {
    Unowned,
    Suppressed {
        owner: Uuid,
    },
    Automatic {
        owner: Uuid,
        proposal: Uuid,
        integrity: ReplyIntegrity,
    },
    Parked,
}

pub(super) fn final_response_disposition(
    completion: &PureCompletion,
) -> Option<ResponseDisposition> {
    (completion.state == RunState::Succeeded && completion.target == TransitionTarget::End)
        .then_some(completion.response_disposition)
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    completion: &PureCompletion,
    owner: completion_job::CompletionOwner,
    notices: Option<&DecisionNotices>,
) -> AppResult<PreparedFinal> {
    let Some(disposition) = final_response_disposition(completion) else {
        return Ok(PreparedFinal::Unowned);
    };
    let Some(root) = root_on(db, scope).await? else {
        return Ok(PreparedFinal::Unowned);
    };
    if disposition == ResponseDisposition::Suppressed {
        return Ok(PreparedFinal::Suppressed {
            owner: root.owner_id,
        });
    }
    authorize_on(db, scope, &root).await?;
    if let Some(saved) = super::channel_reply_proposals::existing_on(db, scope).await? {
        return super::channel_reply_proposals::resume_on(
            db, scope, &root, completion, owner, saved,
        )
        .await;
    }
    let slots: i64 = sqlx::query_scalar("SELECT workflow_message_slots($1,$2)")
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_one(&mut *db)
        .await?;
    if slots >= i64::try_from(MAX_MESSAGE_ACTIONS_PER_EXECUTION).map_err(|_| invalid())? {
        return Err(AppError::BadRequest(
            "Workflow message action limit reached".into(),
        ));
    }
    Box::pin(stage_on(db, scope, &root, completion, owner, notices)).await
}

struct ReplyProjection {
    route_snapshot: Value,
    content: Value,
    closure: ReplyClosure,
    fields: Option<Value>,
    recipients: Vec<knowledge_release::MessageRecipient>,
}

async fn project_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
    completion: &PureCompletion,
) -> AppResult<ReplyProjection> {
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(root.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let definition = binding.bundle().compiled().graph().definition();
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let kind = definition
        .steps
        .iter()
        .find(|(id, _)| id.as_str() == step)
        .map(|(_, step)| step.step_type.as_str())
        .ok_or_else(invalid)?;
    // The terminal step fixes the response kind (AWC-P04-D1 D2); `no_reply` never projects.
    let closure = ReplyClosure::of_step(kind, &completion.output)?.ok_or_else(invalid)?;
    let output_fields = || {
        completion
            .output
            .get("fields")
            .filter(|fields| fields.is_object())
            .cloned()
    };
    let (content, fields) = match closure {
        ReplyClosure::Done => {
            let contract: ChannelReplyContract =
                serde_json::from_value(root.contract.clone()).map_err(|_| invalid())?;
            let schema = definition
                .output_schema
                .as_ref()
                .unwrap_or(&Value::Bool(true));
            let content = contract.project(schema, &completion.output)?;
            (
                json!({"subject":content.subject(),"body":content.body_text()}),
                output_fields(),
            )
        }
        ReplyClosure::Denied => {
            let subject: String =
                sqlx::query_scalar("SELECT subject FROM messages WHERE company_id=$1 AND id=$2")
                    .bind(scope.company.as_uuid())
                    .bind(root.message_id)
                    .fetch_one(&mut *db)
                    .await?;
            let reason = completion.output.get("reason").and_then(Value::as_str);
            let content = denial(&subject, reason.ok_or_else(invalid)?)?;
            (
                json!({"subject":content.subject(),"body":content.body_text()}),
                output_fields(),
            )
        }
        ReplyClosure::HandedOff(run) => {
            // The nested call's disclosed `done` result, which the terminal already proved.
            let result = super::channel_call_results::handed_off_on(db, scope, run)
                .await?
                .ok_or_else(invalid)?;
            (
                result.get("content").cloned().ok_or_else(invalid)?,
                result.get("fields").cloned(),
            )
        }
    };
    let route = message::final_reply_route_on(
        db,
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
        crate::entities::message::CanonicalMessageId::new(root.message_id),
    )
    .await?;
    let route_snapshot = serde_json::to_value(&route).map_err(|_| invalid())?;
    let address = match route.targets.into_iter().next().ok_or_else(invalid)? {
        RouteTarget::ReplyEmail { recipient, .. } => Some(recipient),
        RouteTarget::ReplyInternal { .. } => None,
        _ => return Err(invalid()),
    };
    let recipients = vec![knowledge_release::MessageRecipient::reply(
        root.message_id,
        address,
    )];
    // Result fields are a call's alone: mail never renders them. A call result carries at most a
    // tool result, so larger fields fail the child's terminal rather than reach review.
    let fields = if root.is_call() { fields } else { None };
    if let Some(fields) = &fields
        && serde_json::to_vec(fields).map_err(|_| invalid())?.len()
            > crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
    {
        return Err(AppError::BadRequest(
            "The answer's result fields are larger than a call result may carry".into(),
        ));
    }
    Ok(ReplyProjection {
        content,
        closure,
        fields,
        recipients,
        route_snapshot,
    })
}

async fn stage_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
    completion: &PureCompletion,
    owner: completion_job::CompletionOwner,
    notices: Option<&DecisionNotices>,
) -> AppResult<PreparedFinal> {
    let ReplyProjection {
        content,
        closure,
        fields,
        recipients,
        route_snapshot,
    } = project_on(db, scope, root, completion).await?;
    let action_scope = ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    let policy = root
        .approval_policy
        .clone()
        .map(serde_json::from_value::<ReplyApprovalPolicy>)
        .transpose()
        .map_err(|_| invalid())?;
    let proposal = Uuid::new_v4();
    let approval =
        super::channel_reply_approvals::prepare_on(db, scope, root, policy.as_ref(), notices)
            .await?;
    let document = json!({"company":scope.company.as_uuid(),"run":scope.run.as_uuid(),"execution":scope.execution.as_uuid(),
        "proposal":proposal,"owner":root.owner_id,"owner_kind":root.owner_kind,"source":root.source,"source_message":root.message_id,
        "actor":root.actor_id,"target":root.target()?,"thread":root.thread_id,"output":completion.output,
        "content":content,"closure":closure.as_str(),"handed_off_run_id":closure.handed_off_run().map(|run|run.as_uuid()),"fields":fields,"recipients":recipients,"route_snapshot":route_snapshot,"approval_policy":root.approval_policy,"approval_deadline":approval.as_ref().map(|a|a.deadline)});
    let integrity = integrity_on(db, &document).await?;
    let action = final_action(action_scope, root, proposal, integrity, content.clone())?;
    let levels =
        knowledge_release::evaluate_on(db, &action, knowledge_release::MessageRecipients::Current)
            .await?;
    // Recheck the captured concrete address too; current routing alone is insufficient.
    knowledge_release::evaluate_on(
        db,
        &action,
        knowledge_release::MessageRecipients::Frozen(&recipients),
    )
    .await?;
    let staged = super::channel_reply_proposals::NewProposal {
        scope,
        root,
        id: proposal,
        integrity,
        document: &document,
        completion,
        content: &content,
        closure,
        fields: fields.as_ref(),
        route_snapshot: &route_snapshot,
        recipients: &recipients,
        action: &action,
        levels,
        owner,
        approval: approval.as_ref(),
    };
    let invocation = Box::pin(super::channel_reply_proposals::insert_on(db, staged)).await?;
    if let Some(approval) = approval {
        Box::pin(super::channel_reply_approvals::park_on(
            db, scope, proposal, invocation, &action, owner, approval,
        ))
        .await?;
        tracing::info!(company_id=%scope.company.as_uuid(), run_id=%scope.run.as_uuid(),
            execution_id=%scope.execution.as_uuid(), proposal_id=%proposal,
            status="waiting_for_review", "Final response wait staged");
        return Ok(PreparedFinal::Parked);
    }
    Ok(PreparedFinal::Automatic {
        owner: root.owner_id,
        proposal,
        integrity,
    })
}

#[derive(sqlx::FromRow)]
struct IntegrityRow {
    digest: String,
    bounded: bool,
}
async fn integrity_on(db: &mut PgConnection, document: &Value) -> AppResult<ReplyIntegrity> {
    let row: IntegrityRow=sqlx::query_as("SELECT encode(sha256(convert_to($1::jsonb::text,'UTF8')),'hex') AS digest, \
        octet_length($1::jsonb::text)<=1200000 AND octet_length(($1::jsonb->'route_snapshot')::text)<=16384 \
        AND octet_length(($1::jsonb->'content')::text)<=65536 AND octet_length(($1::jsonb->'recipients')::text)<=16384 \
        AND octet_length(($1::jsonb->'output')::text)<=1048576 AS bounded")
        .bind(document).fetch_one(db).await?;
    if !row.bounded {
        return Err(AppError::BadRequest(
            "Final response evidence exceeds its limit".into(),
        ));
    }
    row.digest.try_into().map_err(|_| invalid())
}

fn final_action(
    scope: ActionScope,
    root: &RootReply,
    proposal: Uuid,
    integrity: ReplyIntegrity,
    content: Value,
) -> AppResult<FrozenAction> {
    FrozenAction::channel_final(ActionRequest {
        scope,
        operation_key: TypeName::parse("message.reply").map_err(|_| invalid())?,
        target: ActionTarget::Local {
            resource: scope.company.as_uuid(),
            resource_kind: TypeName::parse("messaging").map_err(|_| invalid())?,
        },
        contract: builtin_message_snapshot(scope.company, MessageKind::Reply)?,
        arguments: json!({"source_message":root.message_id,"content":content}),
        context: ActionPolicyContext {
            integration: None,
            actor: root.actor_id,
            capability_ceiling: vec![TypeName::parse("message.reply").map_err(|_| invalid())?],
            approval_required: root.approval_policy.is_some(),
            origin: ToolCallOrigin::ChannelFinal(ChannelFinalOrigin {
                proposal: ReplyProposalId::new(proposal),
                integrity,
            }),
        },
    })
}

pub(super) async fn commit_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    prepared: PreparedFinal,
) -> AppResult<()> {
    struct Columns {
        owner: Uuid,
        disposition: ResponseDisposition,
        proposal: Option<Uuid>,
        integrity: Option<ReplyIntegrity>,
    }
    let columns = match prepared {
        PreparedFinal::Unowned => return Ok(()),
        PreparedFinal::Parked => return Err(invalid()),
        PreparedFinal::Suppressed { owner } => Columns {
            owner,
            disposition: ResponseDisposition::Suppressed,
            proposal: None,
            integrity: None,
        },
        PreparedFinal::Automatic {
            owner,
            proposal,
            integrity,
        } => Columns {
            owner,
            disposition: ResponseDisposition::Automatic,
            proposal: Some(proposal),
            integrity: Some(integrity),
        },
    };
    sqlx::query("INSERT INTO channel_reply_decisions(company_id,run_id,execution_id,owner_id,disposition,proposal_id,integrity,state) \
        VALUES($1,$2,$3,$4,$5,$6,$7,CASE WHEN $5='suppressed' THEN 'suppressed' ELSE 'pending' END)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(columns.owner)
        .bind(columns.disposition.as_str()).bind(columns.proposal).bind(columns.integrity.map(|value| value.as_hex())).execute(db).await?;
    tracing::info!(company_id=%scope.company.as_uuid(), run_id=%scope.run.as_uuid(),
        execution_id=%scope.execution.as_uuid(), owner_id=%columns.owner,
        disposition=columns.disposition.as_str(), proposal_id=?columns.proposal,
        "Final response disposition staged");
    Ok(())
}
