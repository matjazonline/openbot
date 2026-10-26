use super::*;
use admission_binding::SourceKey;

/// An admission captures a finite deadline once; replay never renews it. Runtime
/// cancellation/deadline transitions belong to the later execution owner.
pub(super) const ADMISSION_DEADLINE_SECONDS: i32 = 24 * 60 * 60;

#[async_trait]
impl WorkflowAdmission for PostgresPersistence {
    async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult> {
        let mut tx = self.pool.begin().await?;
        if command.admission_claim().is_some() {
            sqlx::query("SET LOCAL lock_timeout = '5s'")
                .execute(&mut *tx)
                .await?;
            sqlx::query("SET LOCAL statement_timeout = '20s'")
                .execute(&mut *tx)
                .await?;
        }
        authority::authorize_company(&mut tx, command.company_id(), command.actor()).await?;
        association::authorize_association(
            &mut tx,
            command.company_id(),
            command.actor(),
            command.association(),
        )
        .await?;
        admission_source::authorize(&mut tx, command).await?;
        if !claim_holds(&mut tx, command).await? {
            tx.rollback().await?;
            return Ok(AdmissionResult::Stale);
        }
        let source = SourceKey::from_trigger(command.trigger());
        if let Some(result) = admission_replay::resolve(&mut tx, command, &source).await? {
            if let AdmissionResult::Replayed(run) = result
                && !settle_claim(&mut tx, command, run).await?
            {
                tx.rollback().await?;
                return Ok(AdmissionResult::Stale);
            }
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
        if !settle_claim(&mut tx, command, command.proposed_run_id()).await? {
            tx.rollback().await?;
            return Ok(AdmissionResult::Stale);
        }
        tx.commit().await?;
        Ok(AdmissionResult::Created(command.proposed_run_id()))
    }
}

async fn claim_holds(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
) -> AppResult<bool> {
    let Some(claim) = command.admission_claim() else {
        return Ok(true);
    };
    match claim {
        AdmissionClaim::Message(claim) => {
            if claim.company != command.company_id()
                || claim.intent != command.trigger().trigger_id().as_uuid()
            {
                return Ok(false);
            }
            let TriggerSource::Message { message_id } = command.trigger().source() else {
                return Ok(false);
            };
            let RelatedAssociation::Thread {
                channel_id,
                thread_id,
            } = command.association()
            else {
                return Ok(false);
            };
            Ok(sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM workflow_message_intents WHERE company_id=$1 AND id=$2 \
                 AND state='claimed' AND worker_id=$3 AND claim_generation=$4 \
                 AND message_id=$5 AND channel_id=$6 AND thread_id=$7 \
                 AND lease_expires_at > clock_timestamp() FOR UPDATE",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.intent)
            .bind(claim.worker.0)
            .bind(claim.generation)
            .bind(message_id.as_uuid())
            .bind(channel_id.as_uuid())
            .bind(thread_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?
            .is_some())
        }
        AdmissionClaim::Schedule(claim) => {
            if claim.company != command.company_id()
                || claim.occurrence != command.trigger().trigger_id().as_uuid()
            {
                return Ok(false);
            }
            Ok(sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM schedule_runs WHERE company_id=$1 AND id=$2 \
                 AND materialization_status='materializing' AND materialization_worker_id=$3 \
                 AND materialization_generation=$4 AND materialization_lock_expires_at > clock_timestamp() FOR UPDATE",
            ).bind(claim.company.as_uuid()).bind(claim.occurrence).bind(claim.worker.0)
                .bind(claim.generation).fetch_optional(&mut **tx).await?.is_some())
        }
    }
}

async fn settle_claim(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedAdmission,
    run: RunId,
) -> AppResult<bool> {
    if !claim_holds(tx, command).await? {
        return Ok(false);
    }
    match command.admission_claim() {
        None => Ok(true),
        Some(AdmissionClaim::Message(claim)) => {
            sqlx::query(
                "INSERT INTO workflow_message_intent_bindings \
                 (company_id,intent_id,binding_id,binding_revision,outcome,run_id) \
                 VALUES ($1,$2,$3,$4,'admitted',$5) \
                 ON CONFLICT (company_id,intent_id,binding_id) DO NOTHING",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.intent)
            .bind(command.binding().id().as_uuid())
            .bind(i64::try_from(command.binding().revision().get()).map_err(|_| invalid())?)
            .bind(run.as_uuid())
            .execute(&mut **tx)
            .await?;
            Ok(true)
        }
        Some(AdmissionClaim::Schedule(claim)) => {
            sqlx::query(
                "UPDATE schedule_runs SET run_id=$3, last_error=NULL, \
                 materialization_status='materialized', materialization_worker_id=NULL, \
                 materialization_generation=NULL, materialization_locked_at=NULL, \
                 materialization_lock_expires_at=NULL, updated_at=clock_timestamp() \
                 WHERE company_id=$1 AND id=$2",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.occurrence)
            .bind(run.as_uuid())
            .execute(&mut **tx)
            .await?;
            sqlx::query(
                "UPDATE channel_schedules SET last_error=NULL WHERE company_id=$1 \
                 AND id=(SELECT schedule_id FROM schedule_runs WHERE company_id=$1 AND id=$2) \
                 AND last_error IS NOT NULL",
            )
            .bind(claim.company.as_uuid())
            .bind(claim.occurrence)
            .execute(&mut **tx)
            .await?;
            Ok(true)
        }
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
