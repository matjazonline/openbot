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
    pub intent_id: Uuid,
    pub message_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub actor_id: Uuid,
    pub contract: Value,
    pub approval_policy: Option<Value>,
    pub source: Value,
    pub deadline: DateTime<Utc>,
    binding_id: Uuid,
}

pub(super) async fn root_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<RootReply>> {
    Ok(sqlx::query_as("SELECT context.intent_id,context.message_id,context.channel_id,context.thread_id,context.actor_id, \
        context.contract,context.approval_policy,context.source,run.deadline,run.binding_id \
        FROM channel_reply_runs AS owned JOIN channel_reply_contexts AS context USING(company_id,intent_id) \
        JOIN workflow_runs AS run ON run.company_id=owned.company_id AND run.id=owned.run_id \
        WHERE owned.company_id=$1 AND owned.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(db).await?)
}

/// Account/company locks precede run locks even when suppression needs no message grant.
pub(super) async fn prelock_on(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<()> {
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM channel_reply_runs WHERE company_id=$1 AND run_id=$2)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    if owned {
        action_authority::prelock_on(
            db,
            ActionScope {
                company: scope.company,
                run: scope.run,
                execution: scope.execution,
            },
        )
        .await?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ControlScope {
    execution_id: Uuid,
}
pub(super) async fn prelock_control_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: WorkflowActor,
) -> AppResult<()> {
    let scope: Option<ControlScope>=sqlx::query_as("SELECT execution.id AS execution_id FROM channel_reply_runs AS owned \
        JOIN workflow_executions AS execution ON execution.company_id=owned.company_id AND execution.run_id=owned.run_id \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
        WHERE owned.company_id=$1 AND owned.run_id=$2 ORDER BY execution.activation DESC LIMIT 1")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_optional(&mut *db).await?;
    if let Some(scope) = scope {
        action_authority::prelock_accounts_on(
            db,
            ActionScope {
                company,
                run,
                execution: ExecutionId::new(scope.execution_id),
            },
            Some(actor),
        )
        .await?;
        // The scope is a lock hint only. The control reloads the real run under its own lock.
    }
    Ok(())
}

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
    message::current_message_on(db, scope.company, actor, MessageKind::Reply).await?;
    association::authorize_association(
        db,
        scope.company,
        actor,
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(root.channel_id),
            thread_id: RelatedThreadId::new(root.thread_id),
        },
    )
    .await
}

pub(super) async fn policy_current_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    root: &RootReply,
) -> AppResult<bool> {
    let current: Option<Option<Value>> = sqlx::query_scalar(
        "SELECT approval_policy FROM channel_reply_settings \
        WHERE company_id=$1 AND channel_id=$2 AND binding_id IS NOT NULL FOR SHARE",
    )
    .bind(scope.company.as_uuid())
    .bind(root.channel_id)
    .fetch_optional(&mut *db)
    .await?;
    Ok(current.as_ref() == Some(&root.approval_policy))
}

pub(super) enum PreparedFinal {
    Unowned,
    Suppressed {
        intent: Uuid,
    },
    Automatic {
        intent: Uuid,
        proposal: Uuid,
        integrity: ReplyIntegrity,
    },
    Parked,
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    completion: &PureCompletion,
    owner: completion_job::CompletionOwner,
    notices: Option<&DecisionNotices>,
) -> AppResult<PreparedFinal> {
    if completion.state != RunState::Succeeded || completion.target != TransitionTarget::End {
        return Ok(PreparedFinal::Unowned);
    }
    let Some(root) = root_on(db, scope).await? else {
        return Ok(PreparedFinal::Unowned);
    };
    if completion.response_disposition == ResponseDisposition::Suppressed {
        return Ok(PreparedFinal::Suppressed {
            intent: root.intent_id,
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
    let contract: ChannelReplyContract =
        serde_json::from_value(root.contract.clone()).map_err(|_| invalid())?;
    let schema = binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .output_schema
        .as_ref()
        .unwrap_or(&Value::Bool(true));
    let content = contract.project(schema, &completion.output)?;
    let content = json!({"subject":content.subject(),"body":content.body_text()});
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
    Ok(ReplyProjection {
        content,
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
        "proposal":proposal,"intent":root.intent_id,"source":root.source,"source_message":root.message_id,
        "actor":root.actor_id,"channel":root.channel_id,"thread":root.thread_id,"output":completion.output,
        "content":content,"recipients":recipients,"route_snapshot":route_snapshot,"approval_policy":root.approval_policy,"approval_deadline":approval.as_ref().map(|a|a.deadline)});
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
        intent: root.intent_id,
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
        intent: Uuid,
        disposition: ResponseDisposition,
        proposal: Option<Uuid>,
        integrity: Option<ReplyIntegrity>,
    }
    let columns = match prepared {
        PreparedFinal::Unowned => return Ok(()),
        PreparedFinal::Parked => return Err(invalid()),
        PreparedFinal::Suppressed { intent } => Columns {
            intent,
            disposition: ResponseDisposition::Suppressed,
            proposal: None,
            integrity: None,
        },
        PreparedFinal::Automatic {
            intent,
            proposal,
            integrity,
        } => Columns {
            intent,
            disposition: ResponseDisposition::Automatic,
            proposal: Some(proposal),
            integrity: Some(integrity),
        },
    };
    sqlx::query("INSERT INTO channel_reply_decisions(company_id,run_id,execution_id,intent_id,disposition,proposal_id,integrity,state) \
        VALUES($1,$2,$3,$4,$5,$6,$7,CASE WHEN $5='suppressed' THEN 'suppressed' ELSE 'pending' END)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(columns.intent)
        .bind(columns.disposition.as_str()).bind(columns.proposal).bind(columns.integrity.map(|value| value.as_hex())).execute(db).await?;
    tracing::info!(company_id=%scope.company.as_uuid(), run_id=%scope.run.as_uuid(),
        execution_id=%scope.execution.as_uuid(), intent_id=%columns.intent,
        disposition=columns.disposition.as_str(), proposal_id=?columns.proposal,
        "Final response disposition staged");
    Ok(())
}
