use super::*;
use crate::adapters::persistence::workflow::native_mcp::{self, NativeMcpOwner};
use crate::application::local_agents::harness::NativeBundleRequest;
use crate::application::local_agents::mcp::{
    NativeMcpAction, NativeMcpBroker, NativeMcpRequest, NativeMcpResponse,
};

#[async_trait]
impl NativeMcpBroker for PostgresPersistence {
    async fn authorize_native_mcp(&self, request: &NativeBundleRequest) -> AppResult<()> {
        let mut tx = begin(&self.pool).await?;
        mcp_authority_on(&mut tx, request).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn sweep_native_mcp(&self, limit: u16) -> AppResult<u16> {
        native_mcp::sweep(self, limit).await
    }
    async fn native_mcp(&self, request: &NativeMcpRequest) -> AppResult<NativeMcpResponse> {
        let mut tx = begin(&self.pool).await?;
        let authority = mcp_authority_on(
            &mut tx,
            &NativeBundleRequest {
                verifier: request.verifier,
                identity: request.identity,
                operation: request.operation,
            },
        )
        .await?;
        let owner = owner_on(&mut tx, &authority, request).await?;
        let response = match &request.body.action {
            NativeMcpAction::Call { alias, arguments } => {
                let tool = authority
                    .bundle
                    .tools
                    .iter()
                    .find(|tool| &tool.alias == alias)
                    .ok_or_else(missing)?
                    .contract()?;
                if let Some(saved) =
                    native_mcp::recover_on(&mut tx, &owner, &tool, arguments).await?
                {
                    saved
                } else {
                    active_on(&mut tx, &authority, request).await?;
                    native_mcp::admit_on(&mut tx, &owner, &tool, arguments).await?
                }
            }
            NativeMcpAction::Status { call_id } => {
                native_mcp::response_on(&mut tx, owner.company, *call_id, Some(&owner)).await?
            }
            NativeMcpAction::Cancel { call_id } => cancel_on(&mut tx, &owner, *call_id).await?,
            NativeMcpAction::CancelInvocation => {
                let call_id = native_mcp::invocation_id_on(&mut tx, &owner).await?;
                cancel_on(&mut tx, &owner, call_id).await?
            }
        };
        tx.commit().await?;
        Ok(response)
    }
}

async fn cancel_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    call: NativeMcpCallId,
) -> AppResult<NativeMcpResponse> {
    let status = native_mcp::response_on(db, owner.company, call, Some(owner)).await?;
    // Desired cancellation takes only the call row; no broker run lock from native ingress.
    sqlx::query("UPDATE native_mcp_calls SET cancel_requested=true WHERE company_id=$1 AND id=$2 AND retired_at IS NULL")
        .bind(owner.company.as_uuid()).bind(call.as_uuid()).execute(db).await?;
    Ok(status)
}

async fn owner_on(
    db: &mut PgConnection,
    authority: &bundles::NativeAuthority,
    request: &NativeMcpRequest,
) -> AppResult<NativeMcpOwner> {
    let invocation = &request.body.invocation;
    match authority.operation.dispatch() {
        DispatchEvidence::NativeIdentity {
            thread_id,
            turn_id: Some(turn_id),
            ..
        } if thread_id == &invocation.thread_id && turn_id == &invocation.turn_id => {}
        _ => {
            return Err(AppError::Conflict(
                "Native invocation has no matching acknowledged owner".into(),
            ));
        }
    }
    if authority.session.thread_id.as_ref() != Some(&invocation.thread_id) {
        return Err(missing());
    }
    let company = CompanyId::new(authority.company);
    let actor = WorkflowActor::authenticated(authority.actor)?;
    let (association, ceiling) =
        native_mcp::source_facts_on(db, company, actor, &authority.session.association).await?;
    let deadline: DateTime<Utc> =
        sqlx::query_scalar("SELECT LEAST($1::timestamptz,clock_timestamp()+interval '15 minutes')")
            .bind(authority.operation.deadline())
            .fetch_one(db)
            .await?;
    Ok(NativeMcpOwner {
        company,
        actor,
        operation: authority.operation.id(),
        machine: authority.machine,
        installation: authority.session.installation_id,
        session: authority.session.id,
        harness: authority.session.harness.clone(),
        invocation: invocation.clone(),
        association,
        ceiling,
        deadline,
    })
}

async fn active_on(
    db: &mut PgConnection,
    authority: &bundles::NativeAuthority,
    request: &NativeMcpRequest,
) -> AppResult<()> {
    // Expiry can own only an operation. Read it again under its lock after all source/machine locks.
    let active:Option<Uuid> = sqlx::query_scalar("SELECT id FROM local_agent_operations WHERE company_id=$1 AND id=$2
        AND actor_id=$3 AND outcome IS NULL AND deadline>clock_timestamp()
        AND NOT public.native_mcp_stop_requested(company_id,id)
        AND dispatch->>'kind'='native_identity' AND dispatch->>'thread_id'=$4 AND dispatch->>'turn_id'=$5
        AND (observed_turn_state IS NULL OR observed_turn_state IN ('queued','running','waiting_for_input')) FOR UPDATE")
        .bind(authority.company).bind(authority.operation.id().as_uuid()).bind(authority.actor)
        .bind(request.body.invocation.thread_id.as_str()).bind(request.body.invocation.turn_id.as_str()).fetch_optional(&mut *db).await?;
    if active.is_none() {
        return Err(AppError::Conflict(
            "Originating native operation has ended".into(),
        ));
    }
    if let SessionAssociation::Workflow { run_id } = authority.session.association {
        let live:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$2 AND state IN ('queued','running','waiting') AND deadline>clock_timestamp())")
            .bind(authority.company).bind(run_id).fetch_one(db).await?;
        if !live {
            return Err(AppError::Conflict("Originating workflow has ended".into()));
        }
    }
    Ok(())
}

async fn mcp_authority_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &NativeBundleRequest,
) -> AppResult<bundles::NativeAuthority> {
    match Box::pin(bundles::authorize_with_locks_on(
        tx,
        request,
        crate::adapters::persistence::workflow::row_locks::RowLocks::NoWait,
    ))
    .await
    {
        Err(AppError::DatabaseTimeout(_)) => Err(AppError::ServiceUnavailable(
            "Native MCP source is busy; retry the same invocation".into(),
        )),
        result => result,
    }
}
