use super::*;
use admission_binding::SourceKey;

/// An admission captures a finite deadline once; replay never renews it. Runtime
/// cancellation/deadline transitions belong to the later execution owner.
pub(super) const ADMISSION_DEADLINE_SECONDS: i32 = 24 * 60 * 60;

#[async_trait]
impl WorkflowAdmission for PostgresPersistence {
    async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult> {
        let mut tx = self.pool.begin().await?;
        authority::authorize_company(&mut tx, command.company_id(), command.actor()).await?;
        association::authorize_association(
            &mut tx,
            command.company_id(),
            command.actor(),
            command.association(),
        )
        .await?;
        admission_source::authorize(&mut tx, command).await?;
        let source = SourceKey::from_trigger(command.trigger());
        if let Some(result) = admission_replay::resolve(&mut tx, command, &source).await? {
            tx.commit().await?;
            return Ok(result);
        }
        // Source authorization already holds the parent lock. Existing children
        // replay above; cancellation must prevent only fresh child scheduling.
        if let TriggerSource::Child { parent } = command.trigger().source() {
            let cancelled: bool = sqlx::query_scalar(
                "SELECT state='cancelled' FROM workflow_runs WHERE company_id=$1 AND id=$2",
            )
            .bind(command.company_id().as_uuid())
            .bind(parent.execution().run_id().as_uuid())
            .fetch_one(&mut *tx)
            .await?;
            if cancelled {
                return Err(conflict());
            }
        }
        verify_current(&mut tx, command).await?;
        authorize_token_reviewers(&mut tx, command).await?;
        resources::check_readiness(&mut tx, command.binding()).await?;
        admission_write::create(&mut tx, command, &source).await?;
        tx.commit().await?;
        Ok(AdmissionResult::Created(command.proposed_run_id()))
    }
}

async fn authorize_token_reviewers(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<()> {
    for user in token_reviewers(db, command).await? {
        let actor = WorkflowActor::authenticated(user).map_err(|_| invalid())?;
        let reviewer = authority::authorize_reviewer_on(db, command.company_id(), actor).await?;
        if !reviewer.privileged {
            return Err(missing());
        }
    }
    Ok(())
}

async fn token_reviewers(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<Vec<Uuid>> {
    // Read immutable root policy without locking the root run from a child.
    // A child's local declaration never replaces its inherited root reviewers.
    if let TriggerSource::Child { parent } = command.trigger().source() {
        return sqlx::query_scalar(
            "SELECT ARRAY(SELECT (reviewer->>'id')::uuid FROM \
             jsonb_array_elements(COALESCE(budget.token_policy->'reviewers','[]'::jsonb)) AS reviewer) \
             FROM workflow_run_budgets AS link JOIN workflow_root_budgets AS budget \
               ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id \
             WHERE link.company_id=$1 AND link.run_id=$2",
        )
        .bind(command.company_id().as_uuid())
        .bind(parent.execution().run_id().as_uuid())
        .fetch_optional(db)
        .await?
        .ok_or_else(missing);
    }
    let limits = &command
        .binding()
        .bundle()
        .compiled()
        .graph()
        .definition()
        .limits;
    Ok(limits
        .root_budget
        .tokens()
        .map(|policy| policy.reviewers().to_vec())
        .unwrap_or_default())
}

async fn verify_current(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<()> {
    // The company lock serializes lifecycle edits; the row lock additionally
    // closes direct head deletion/update races until this admission commits.
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workflow_bindings WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.binding().id().as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(conflict)?;
    let current = binding_rows::read_binding(tx, command.company_id(), command.binding().id())
        .await?
        .ok_or_else(conflict)?;
    let selected = command.binding();
    if !current.active
        || !within_association(current.association, command.association())
        || current.configuration.revision() != selected.revision()
        || current.configuration.params() != selected.params()
        || current.configuration.resources() != selected.resources()
        || store_bundle(current.configuration.bundle())? != store_bundle(selected.bundle())?
    {
        return Err(conflict());
    }
    bindings::verify_selection(tx, selected).await
}

fn within_association(binding: RelatedAssociation, run: RelatedAssociation) -> bool {
    match (binding, run) {
        (RelatedAssociation::Company, _) => true,
        (RelatedAssociation::Channel(expected), RelatedAssociation::Channel(actual))
        | (
            RelatedAssociation::Channel(expected),
            RelatedAssociation::Thread {
                channel_id: actual, ..
            },
        ) => expected == actual,
        (expected, actual) => expected == actual,
    }
}
