use super::*;

pub(in crate::adapters::persistence::workflow) async fn persist_on(
    db: &mut PgConnection,
    manifest: &DependencyManifest,
) -> AppResult<DependencyManifestId> {
    let id = DependencyManifestId::new(Uuid::new_v4());
    let company = manifest.company_id.as_uuid();
    let root = manifest.root_run_id.as_uuid();
    sqlx::query("INSERT INTO agent_dependency_manifests(company_id,id,root_run_id,content,digest) VALUES($1,$2,$3,$4,$5)")
        .bind(company).bind(id.as_uuid()).bind(root)
        .bind(String::from_utf8(manifest.canonical_content()?).map_err(|_| invalid())?)
        .bind(manifest.content_hash()?.as_slice()).execute(&mut *db).await?;
    for agent in &manifest.agents {
        sqlx::query("INSERT INTO agent_manifest_revisions(company_id,manifest_id,agent_id,revision_id) VALUES($1,$2,$3,$4)")
            .bind(company).bind(id.as_uuid()).bind(agent.agent_id.as_uuid()).bind(agent.revision_id.as_uuid())
            .execute(&mut *db).await?;
    }
    for workflow in &manifest.workflows {
        sqlx::query("INSERT INTO agent_manifest_workflows(company_id,manifest_id,workflow_id,version_id) VALUES($1,$2,$3,$4)")
            .bind(company).bind(id.as_uuid()).bind(workflow.workflow_id.as_uuid()).bind(workflow.version_id.as_uuid())
            .execute(&mut *db).await?;
    }
    sqlx::query("INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id) VALUES($1,$2,$2,$3)")
        .bind(company).bind(root).bind(id.as_uuid()).execute(db).await?;
    Ok(id)
}

/// Read from the actual parent's stored root selection, never from a supplied manifest ID.
pub(in crate::adapters::persistence::workflow) async fn attach_child_on(
    db: &mut PgConnection,
    company: CompanyId,
    parent: RunId,
    child: RunId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<()> {
    let selected: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT ownership.root_run_id,ownership.manifest_id FROM run_dependency_manifests AS ownership \
         JOIN agent_manifest_workflows AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND selection.workflow_id=$3 AND selection.version_id=$4 FOR SHARE OF ownership,selection"
    ).bind(company.as_uuid()).bind(parent.as_uuid()).bind(workflow.as_uuid()).bind(version.as_uuid())
        .fetch_optional(&mut *db).await?;
    let (root, manifest) = selected.ok_or_else(refused)?;
    sqlx::query("INSERT INTO run_dependency_manifests(company_id,run_id,root_run_id,manifest_id) VALUES($1,$2,$3,$4)")
        .bind(company.as_uuid()).bind(child.as_uuid()).bind(root).bind(manifest).execute(db).await?;
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn selected_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    actor: WorkflowActor,
    agent: AgentId,
) -> AppResult<ResolvedAgent> {
    let selected: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT ownership.manifest_id,selection.revision_id FROM run_dependency_manifests AS ownership \
         JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND selection.agent_id=$3 FOR SHARE OF ownership,selection"
    ).bind(company.as_uuid()).bind(run.as_uuid()).bind(agent.as_uuid()).fetch_optional(&mut *db).await?;
    let (manifest, revision) = selected.ok_or_else(refused)?;
    let scope = AgentPublicationScope {
        company,
        actor,
        agent,
    };
    let revision = crate::adapters::persistence::agent_publication::storage::revision_on(
        db,
        scope,
        AgentRevisionId::new(revision),
    )
    .await?
    .ok_or_else(invalid)?;
    ResolvedAgent::from_revision(DependencyManifestId::new(manifest), revision)
}

pub(in crate::adapters::persistence::workflow) async fn workflow_bundle_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    workflow: WorkflowId,
    version: VersionId,
) -> AppResult<Arc<PublishedBundle>> {
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT version.bundle FROM run_dependency_manifests AS ownership \
         JOIN agent_manifest_workflows AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
         JOIN workflow_versions AS version ON version.company_id=selection.company_id AND version.workflow_id=selection.workflow_id AND version.id=selection.version_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND selection.workflow_id=$3 AND selection.version_id=$4 AND octet_length(version.bundle)<=$5"
    ).bind(company.as_uuid()).bind(run.as_uuid()).bind(workflow.as_uuid()).bind(version.as_uuid())
        .bind(MAX_BUNDLE_BYTES as i32).fetch_optional(db).await?.ok_or_else(missing)?;
    let bundle = restore_bundle(
        &bytes,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let definition = bundle.compiled().graph().definition();
    if bundle.company_id() != company
        || definition.workflow_id != workflow
        || definition.version_id != version
    {
        return Err(invalid());
    }
    Ok(bundle)
}

/// The authored use controls projection; callers cannot downgrade an agent.run checkpoint.
pub(in crate::adapters::persistence::workflow) async fn execution_selection_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::actions::ActionScope,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
) -> AppResult<ResolvedAgent> {
    let (step, inputs): (String, serde_json::Value) = sqlx::query_as(
        "SELECT step_id,frozen_inputs FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3"
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let definition = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(missing)?;
    let kind = crate::application::workflow::registry::ModelStep::of(definition.step_type.as_str())
        .ok_or_else(missing)?;
    let mode = if kind.uses_capabilities() {
        AgentUseMode::Tools
    } else {
        AgentUseMode::ModelOnly
    };
    let key = inputs["agent"]
        .as_str()
        .and_then(crate::application::agent::AgentKey::parse)
        .ok_or_else(missing)?;
    let reference = bundle.agent_reference(scope.company, &key)?;
    if !reference
        .uses
        .iter()
        .any(|usage| usage.step == step && usage.mode == mode)
    {
        return Err(missing());
    }
    let selection = selected_on(db, scope.company, scope.run, actor, reference.agent_id).await?;
    Ok(match mode {
        AgentUseMode::Tools => selection,
        AgentUseMode::ModelOnly => selection.model_only(),
    })
}
