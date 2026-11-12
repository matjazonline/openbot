//! Live effect authorization. Historical origin restoration never calls this module.
use super::super::row_locks::RowLocks;
use super::*;
use crate::application::workflow::actions::ActionRequest;
use crate::domain::local_agents::{LocalAgentSession, SessionAssociation};

#[derive(sqlx::FromRow)]
struct Origin {
    operation_id: Uuid,
    machine_id: Uuid,
    session_id: Uuid,
    actor_id: Uuid,
    origin: String,
}

pub(in crate::adapters::persistence::workflow) async fn authorize_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionRequest,
) -> AppResult<()> {
    match Box::pin(authorize_inner(tx, request)).await {
        Err(AppError::DatabaseTimeout(_)) => Err(AppError::ServiceUnavailable(
            "Native MCP authority is busy; no new dispatch was authorized".into(),
        )),
        result => result,
    }
}

async fn authorize_inner(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionRequest,
) -> AppResult<()> {
    let company = request.scope.company;
    let (call,private):(Option<Uuid>,bool)=sqlx::query_as("SELECT run.native_mcp_call_id,
        EXISTS(SELECT 1 FROM native_mcp_brokers AS broker WHERE broker.company_id=run.company_id AND broker.version_id=run.version_id)
        FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(company.as_uuid()).bind(request.scope.run.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let Some(call) = call else {
        if private {
            return Err(missing());
        }
        return Ok(());
    };
    if !private {
        return Err(missing());
    }
    let (actor,bytes):(Uuid,Vec<u8>)=sqlx::query_as("SELECT run.actor_id,blob.bundle FROM workflow_runs AS run
        JOIN workflow_bundles AS blob ON blob.company_id=run.company_id AND blob.digest=run.bundle_digest
        WHERE run.company_id=$1 AND run.id=$2 AND run.native_mcp_call_id=$3 FOR SHARE OF run NOWAIT")
        .bind(company.as_uuid()).bind(request.scope.run.as_uuid()).bind(call).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let actor = WorkflowActor::authenticated(actor)?;
    let bundle = restore_bundle(
        &bytes,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    provenance::restore_on(tx, request, actor, &bundle).await?;
    eligible(tx, request).await?;
    // Every added owner acquisition, including nested source helpers, uses NOWAIT.
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE NOWAIT")
        .bind(actor.user_id())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(missing)?;
    authority::authorize_company_with_locks(tx, company, actor, RowLocks::NoWait).await?;
    let source=sqlx::query_as::<_,Origin>("SELECT call.operation_id,call.machine_id,call.session_id,call.actor_id,operation.origin
        FROM native_mcp_calls AS call JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
        WHERE call.company_id=$1 AND call.id=$2 AND call.run_id=$3 AND call.retired_at IS NULL")
        .bind(company.as_uuid()).bind(call).bind(request.scope.run.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    if source.actor_id != actor.user_id() {
        return Err(missing());
    }
    let session = crate::adapters::persistence::local_agents::shared::session_on(
        tx,
        company,
        SessionId::new(source.session_id).map_err(|_| invalid())?,
    )
    .await?
    .ok_or_else(missing)?;
    source_on(tx, company, actor, &source, &session).await?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM local_agent_operations WHERE company_id=$1 AND id=$2 AND machine_id=$3
        AND actor_id=$4 AND outcome IS NULL AND deadline>clock_timestamp() FOR SHARE NOWAIT",
    )
    .bind(company.as_uuid())
    .bind(source.operation_id)
    .bind(source.machine_id)
    .bind(actor.user_id())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM native_mcp_calls WHERE company_id=$1 AND id=$2 AND NOT cancel_requested
        AND deadline>clock_timestamp() AND retired_at IS NULL FOR SHARE NOWAIT",
    )
    .bind(company.as_uuid())
    .bind(call)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    eligible(tx, request).await
}

async fn eligible(db: &mut PgConnection, request: &ActionRequest) -> AppResult<()> {
    let live: bool = sqlx::query_scalar("SELECT COALESCE(public.native_mcp_run_live($1,$2),false)")
        .bind(request.scope.company.as_uuid())
        .bind(request.scope.run.as_uuid())
        .fetch_one(db)
        .await?;
    if !live {
        return Err(AppError::Forbidden(
            "Originating native MCP authority has ended".into(),
        ));
    }
    Ok(())
}

async fn source_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    source: &Origin,
    session: &LocalAgentSession,
) -> AppResult<()> {
    match session.association {
        SessionAssociation::Workflow { run_id } => {
            super::super::local_agents::authorize_run_with_locks_on(
                tx,
                company,
                run_id,
                actor,
                RowLocks::NoWait,
            )
            .await?;
        }
        SessionAssociation::Conversation { conversation_id } => {
            let channel: Uuid =
                sqlx::query_scalar("SELECT channel_id FROM threads WHERE company_id=$1 AND id=$2")
                    .bind(company.as_uuid())
                    .bind(conversation_id)
                    .fetch_optional(&mut **tx)
                    .await?
                    .ok_or_else(missing)?;
            association::authorize_association_with_locks(
                tx,
                company,
                actor,
                RelatedAssociation::Thread {
                    channel_id: RelatedChannelId::new(channel),
                    thread_id: RelatedThreadId::new(conversation_id),
                },
                RowLocks::NoWait,
            )
            .await?;
        }
    }
    if source.origin == "workflow" {
        super::super::local_agents::authorize_operation_with_locks_on(
            tx,
            company,
            OperationId::new(source.operation_id).map_err(|_| invalid())?,
            actor,
            RowLocks::NoWait,
        )
        .await?;
    } else if source.origin != "direct" {
        return Err(invalid());
    }
    if session.selection.machine_id.as_uuid() != source.machine_id {
        return Err(missing());
    }
    crate::adapters::persistence::local_agents::shared::validate_frozen_with_locks_on(
        tx,
        company,
        actor.user_id(),
        &session.selection,
        &session.harness,
        RowLocks::NoWait,
    )
    .await
}
