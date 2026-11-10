use super::*;

/// The call and shared action intent commit together, before mutable profile defaults can
/// be consulted by another claim. A replay always uses the saved selection and deadline.
pub(super) async fn prepare_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    policy: LeasePolicy,
) -> AppResult<SavedCall> {
    let scope = action_scope(claim.fence.scope);
    let facts = http::runtime::facts(tx, scope).await?;
    if !lease::lock_fence(tx, claim.fence, policy.attempt_bound()).await? {
        return Err(conflict());
    }
    let inputs =
        http::runtime::frozen_inputs(tx, &facts, claim, &WorkflowStepKind(contract::KIND.into()))
            .await?;
    if let Some(call) = saved_on(tx, scope).await? {
        let action = call.action()?;
        if action.scope() != scope {
            return Err(invalid());
        }
        return Ok(call);
    }
    let inputs: LocalRunInputs = serde_json::from_value(inputs).map_err(|_| contract::invalid())?;
    authorize_run_on(tx, scope.company, scope.run.as_uuid(), facts.actor).await?;
    // Resolve the original association before taking any machine lock.
    if let Some(id) = inputs.session_id {
        let session = shared::session_on(tx, scope.company, id)
            .await?
            .ok_or_else(missing)?;
        validate_continuation_on(tx, scope, facts.actor, inputs.profile_id, &session).await?;
    }
    let locked = shared::lock_profile_on(tx, scope.company, inputs.profile_id).await?;
    if locked.machine.state != MachineState::Enabled {
        return Err(conflict());
    }
    shared::validate_selection_on(
        tx,
        scope.company,
        locked.machine.id,
        &locked.profile.selection,
    )
    .await?;
    let deadline: DateTime<Utc> = sqlx::query_scalar(
        "SELECT LEAST(deadline,clock_timestamp()+interval '30 minutes') FROM workflow_runs WHERE company_id=$1 AND id=$2",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    let frozen = FrozenLocalRun {
        profile_id: inputs.profile_id,
        prompt: inputs.prompt,
        session_id: inputs.session_id,
        approval: inputs.approval,
        selection: locked.profile.selection,
        deadline,
    };
    let tool = facts
        .bundle
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.contract.name.as_str() == contract::KIND)
        .cloned()
        .ok_or_else(missing)?;
    let mut context = http::runtime::context(&facts);
    context.approval_required = frozen.approval.is_some();
    let action = FrozenAction::local_agent(ActionRequest {
        scope,
        operation_key: tool.contract.name.clone(),
        target: ActionTarget::Local {
            resource: frozen.profile_id.as_uuid(),
            resource_kind: TypeName::parse(contract::RESOURCE_KIND).map_err(|_| invalid())?,
        },
        contract: tool,
        arguments: serde_json::to_value(frozen).map_err(|_| invalid())?,
        context,
    })?;
    let intent = actions::insert_on(tx, &action, None).await?;
    sqlx::query("INSERT INTO workflow_local_agent_calls(company_id,run_id,execution_id,invocation_id,argument_digest,profile_id) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(intent.invocation.as_uuid()).bind(intent.argument_digest.as_str()).bind(inputs.profile_id.as_uuid())
        .execute(&mut **tx).await?;
    saved_on(tx, scope).await?.ok_or_else(invalid)
}

pub(super) async fn validate_continuation_on(
    db: &mut PgConnection,
    scope: ActionScope,
    actor: WorkflowActor,
    profile: ProfileId,
    session: &LocalAgentSession,
) -> AppResult<()> {
    if session.company_id != scope.company.as_uuid()
        || session.profile_id != profile
        || session.thread_id.is_none()
    {
        return Err(missing());
    }
    let SessionAssociation::Workflow { run_id: original } = session.association else {
        return Err(missing());
    };
    let current = authorize_run_on(db, scope.company, scope.run.as_uuid(), actor).await?;
    let original = authorize_run_on(db, scope.company, original, actor).await?;
    if current != original {
        return Err(missing());
    }
    Ok(())
}
