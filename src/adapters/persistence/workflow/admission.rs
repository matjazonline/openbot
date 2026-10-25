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
        if !verify_current(&mut tx, command).await? {
            // Nothing was written; the caller re-reads the binding and decides again.
            tx.rollback().await?;
            return Ok(AdmissionResult::Stale);
        }
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
    for user in token_reviewers(command) {
        let actor = WorkflowActor::authenticated(user).map_err(|_| invalid())?;
        let reviewer = authority::authorize_reviewer_on(db, command.company_id(), actor).await?;
        if !reviewer.privileged {
            return Err(missing());
        }
    }
    Ok(())
}

/// Admission creates only root runs; children inherit the root policy through their budget link.
fn token_reviewers(command: &PreparedAdmission) -> Vec<Uuid> {
    command
        .binding()
        .bundle()
        .compiled()
        .graph()
        .definition()
        .limits
        .root_budget
        .tokens()
        .map(|policy| policy.reviewers().to_vec())
        .unwrap_or_default()
}

/// `false` when the binding's current configuration is not the one this admission prepared
/// against or the caller evaluated: a concurrent reconfiguration, deactivation or re-selection
/// (p08 D1.4.5 `Stale`). A binding narrower than the run's association is a caller error.
async fn verify_current(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<bool> {
    // The company lock serializes lifecycle edits; the row lock additionally
    // closes direct head deletion/update races until this admission commits.
    let exists = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workflow_bindings WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.binding().id().as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    if exists.is_none() {
        return Ok(false);
    }
    let Some(current) =
        binding_rows::read_binding(tx, command.company_id(), command.binding().id()).await?
    else {
        return Ok(false);
    };
    let selected = command.binding();
    if !within_association(current.association, command.association()) {
        return Err(conflict());
    }
    if !current.active
        || command
            .expected_binding_revision()
            .is_some_and(|expected| expected != current.configuration.revision())
        || current.configuration.revision() != selected.revision()
        || current.configuration.params() != selected.params()
        || current.configuration.resources() != selected.resources()
        || current.configuration.run_as() != selected.run_as()
        || current.configuration.message_trigger() != selected.message_trigger()
        || store_bundle(current.configuration.bundle())? != store_bundle(selected.bundle())?
    {
        return Ok(false);
    }
    bindings::verify_selection(tx, selected).await?;
    Ok(true)
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
