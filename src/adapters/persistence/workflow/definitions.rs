use super::{authority::*, rows::*, *};

async fn read_draft(
    db: &mut PgConnection,
    company: CompanyId,
    workflow: WorkflowId,
) -> AppResult<Option<DraftState>> {
    sqlx::query_as::<_, DraftRow>(&format!(
        "SELECT {DRAFT_COLUMNS} FROM workflow_definitions \
        WHERE company_id = $1 AND id = $2"
    ))
    .bind(company.as_uuid())
    .bind(workflow.as_uuid())
    .fetch_optional(db)
    .await?
    .map(DraftRow::restore)
    .transpose()
}
async fn read_publication(
    db: &mut PgConnection,
    company: CompanyId,
    key: &IdempotencyKey,
) -> AppResult<Option<PublishedCommand>> {
    sqlx::query_as::<_, VersionRow>(&format!(
        "SELECT {VERSION_COLUMNS} FROM workflow_versions \
        WHERE company_id = $1 AND command_key = $2"
    ))
    .bind(company.as_uuid())
    .bind(key.as_str())
    .fetch_optional(db)
    .await?
    .map(VersionRow::restore)
    .transpose()
}

async fn event(
    tx: &mut Transaction<'_, Postgres>,
    draft: &CompanyWorkflowDraft,
    actor: WorkflowActor,
    kind: &str,
    version: Option<VersionId>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_definition_events \
        (company_id, workflow_id, id, revision, event_kind, actor_id, version_id) \
        VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(draft.company().as_uuid())
    .bind(draft.workflow().as_uuid())
    .bind(Uuid::new_v4())
    .bind(revision(draft.revision().get())?)
    .bind(kind)
    .bind(actor.user_id())
    .bind(version.map(VersionId::as_uuid))
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn insert_draft(
    tx: &mut Transaction<'_, Postgres>,
    draft: &CompanyWorkflowDraft,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_definitions \
        (company_id, id, revision, title, description, source, template_origin) \
        VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(draft.company().as_uuid())
    .bind(draft.workflow().as_uuid())
    .bind(revision(draft.revision().get())?)
    .bind(draft.title())
    .bind(draft.description())
    .bind(draft.source())
    .bind(origin_json(draft)?)
    .execute(&mut **tx)
    .await
    .map_err(write_error)?;
    Ok(())
}

#[async_trait]
impl WorkflowDraftCopies for PostgresPersistence {
    async fn insert_copy(&self, copy: PreparedTemplateCopy) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let draft = copy.draft();
        authorize_company(&mut tx, draft.company(), copy.actor()).await?;
        insert_draft(&mut tx, draft).await?;
        event(&mut tx, draft, copy.actor(), "copied", None).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl WorkflowDefinitions for PostgresPersistence {
    async fn draft(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DraftState>> {
        read_draft(&mut *self.pool.acquire().await?, company, workflow).await
    }
    async fn publication(
        &self,
        company: CompanyId,
        key: &IdempotencyKey,
    ) -> AppResult<Option<PublishedCommand>> {
        read_publication(&mut *self.pool.acquire().await?, company, key).await
    }
    async fn selectable_version(
        &self,
        company: CompanyId,
        version: VersionId,
    ) -> AppResult<Option<Arc<PublishedBundle>>> {
        let query = format!(
            "SELECT {VERSION_COLUMNS} FROM workflow_versions \
            WHERE company_id = $1 AND id = $2 AND EXISTS \
            (SELECT 1 FROM workflow_definitions AS definition \
             WHERE definition.company_id = workflow_versions.company_id \
               AND definition.id = workflow_versions.workflow_id AND NOT definition.archived)"
        );
        sqlx::query_as::<_, VersionRow>(&query)
            .bind(company.as_uuid())
            .bind(version.as_uuid())
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.restore().map(|command| command.bundle))
            .transpose()
    }
    async fn save_draft(&self, command: PreparedDraft) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let state = command.state();
        let draft = &state.draft;
        authorize_company(&mut tx, draft.company(), command.actor()).await?;
        match command.expected() {
            None => insert_draft(&mut tx, draft).await?,
            Some(expected) => {
                let count = sqlx::query(
                    "UPDATE workflow_definitions SET revision = $3, title = $4, \
                    description = $5, source = $6, archived = $7 \
                    WHERE company_id = $1 AND id = $2 AND revision = $8 AND NOT archived",
                )
                .bind(draft.company().as_uuid())
                .bind(draft.workflow().as_uuid())
                .bind(revision(draft.revision().get())?)
                .bind(draft.title())
                .bind(draft.description())
                .bind(draft.source())
                .bind(state.archived)
                .bind(revision(expected.get())?)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if count != 1 {
                    return Err(conflict());
                }
            }
        }
        event(
            &mut tx,
            draft,
            command.actor(),
            if state.archived { "archived" } else { "saved" },
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    async fn publish(&self, prepared: PreparedPublication) -> AppResult<Arc<PublishedBundle>> {
        let request = &prepared.command().request;
        let mut tx = self.pool.begin().await?;
        authorize_company(&mut tx, request.target.company, request.target.actor).await?;
        let bundle = publish_on(&mut tx, &prepared).await?;
        tx.commit().await?;
        Ok(bundle)
    }
}

/// Publish under the company lock the caller holds: replay by key, the draft comparison, the
/// publication checks (`supported_publication` at Publish, `verify_on`, `verify_children`), the
/// version and its event. `publish` and assistant provisioning (p08 D2.3) both use it.
pub(super) async fn publish_on(
    tx: &mut Transaction<'_, Postgres>,
    prepared: &PreparedPublication,
) -> AppResult<Arc<PublishedBundle>> {
    let command = prepared.command();
    let request = &command.request;
    if let Some(saved) = read_publication(tx, request.target.company, &request.key).await? {
        if !saved.request.equivalent(request) {
            return Err(conflict());
        }
        return Ok(saved.bundle);
    }
    let current = read_draft(tx, request.target.company, request.target.workflow)
        .await?
        .ok_or_else(missing)?;
    if current.archived
        || current.draft != command.draft
        || current.draft.revision() != request.expected
    {
        return Err(conflict());
    }
    supported_publication(
        tx,
        &command.bundle,
        super::workflow_tools::ToolCheck::Publish,
    )
    .await?;
    // Agent/skill snapshots are compared with their durable rows at publish only (06-D5).
    super::agent_publication::verify_on(tx, &command.bundle).await?;
    verify_children(tx, &command.bundle).await?;
    insert_version(tx, command).await?;
    event(
        tx,
        &command.draft,
        request.target.actor,
        "published",
        Some(request.version),
    )
    .await?;
    Ok(command.bundle.clone())
}

/// The draft write a template copy makes, with its event, under the caller's company lock.
pub(super) async fn insert_draft_on(
    tx: &mut Transaction<'_, Postgres>,
    draft: &CompanyWorkflowDraft,
    actor: WorkflowActor,
    kind: &str,
) -> AppResult<()> {
    insert_draft(tx, draft).await?;
    event(tx, draft, actor, kind, None).await
}

async fn verify_children(
    tx: &mut Transaction<'_, Postgres>,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    // The bounded immutable closure was checked by the bundle codec. Each direct
    // child must already be a real selectable publication, not caller-created facts.
    for child in bundle.children().values() {
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT version.content_hash FROM workflow_versions AS version \
            JOIN workflow_definitions AS definition ON definition.company_id = version.company_id \
                AND definition.id = version.workflow_id \
            WHERE version.company_id = $1 AND version.id = $2 AND NOT definition.archived",
        )
        .bind(bundle.company_id().as_uuid())
        .bind(child.compiled().graph().definition().version_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;
        if hash.as_deref() != Some(child.content_hash().as_str()) {
            return Err(conflict());
        }
    }
    Ok(())
}
async fn insert_version(
    tx: &mut Transaction<'_, Postgres>,
    command: &PublishedCommand,
) -> AppResult<()> {
    let request = &command.request;
    let draft = &command.draft;
    sqlx::query(
        "INSERT INTO workflow_versions (company_id, workflow_id, id, draft_revision, command_key, \
        actor_id, title, description, source, template_origin, bundle, content_hash, agent_ids) \
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(draft.company().as_uuid())
    .bind(draft.workflow().as_uuid())
    .bind(request.version.as_uuid())
    .bind(revision(draft.revision().get())?)
    .bind(request.key.as_str())
    .bind(request.target.actor.user_id())
    .bind(draft.title())
    .bind(draft.description())
    .bind(draft.source())
    .bind(origin_json(draft)?)
    .bind(store_bundle(&command.bundle)?)
    .bind(command.bundle.content_hash().as_str())
    .bind(
        command
            .bundle
            .frozen_agent_ids()
            .into_iter()
            .collect::<Vec<Uuid>>(),
    )
    .execute(&mut **tx)
    .await
    .map_err(write_error)?;
    Ok(())
}
