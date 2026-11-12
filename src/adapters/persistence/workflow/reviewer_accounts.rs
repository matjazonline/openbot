//! Account hints are discovery only. Sorted account locks precede company/run ownership.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::decisions::specification;
use crate::application::workflow::reviewer_assignment::ReviewerSelection;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
struct AccountMapping {
    principal_id: Uuid,
    user_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BusinessPurpose {
    Existing,
    Unavailable,
    Prepare { inputs: Value, follow: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewerAccountHints {
    actor: Uuid,
    source: Value,
    purpose: BusinessPurpose,
    mappings: Vec<AccountMapping>,
}

pub(super) async fn prelock_business_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<()> {
    let Some(hints) = business_hints_on(db, scope).await? else {
        return Ok(());
    };
    let mut accounts = vec![hints.actor];
    accounts.extend(hints.mappings.iter().filter_map(|mapping| mapping.user_id));
    accounts.sort_unstable();
    accounts.dedup();
    // One actor plus the exact effective Business reviewers. No unused policy recipients.
    if accounts.len() > registry::MAX_REVIEWERS + 1 {
        return Err(invalid());
    }
    let locked: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
            .bind(&accounts)
            .fetch_all(&mut *db)
            .await?;
    if locked != accounts {
        return Err(changed());
    }
    let company: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
            .bind(scope.company.as_uuid())
            .fetch_optional(&mut *db)
            .await?;
    company.ok_or_else(missing)?;
    if business_hints_on(db, scope).await?.as_ref() != Some(&hints) {
        return Err(changed());
    }
    Ok(())
}

async fn business_hints_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<ReviewerAccountHints>> {
    let source:Option<Value>=sqlx::query_scalar(
        "SELECT jsonb_build_object('actor',run.actor_id,'binding',run.binding_id,'revision',run.binding_revision, \
         'digest',run.bundle_digest,'version',run.version_id,'state',run.state,'step',execution.step_id, \
         'ordinal',execution.activation,'decision',decision.id,'submitted',decision.submitted_at) \
         FROM workflow_runs AS run JOIN workflow_executions AS execution \
           ON execution.company_id=run.company_id AND execution.run_id=run.id AND execution.id=$3 \
         LEFT JOIN workflow_decisions AS decision ON decision.company_id=run.company_id AND decision.execution_id=execution.id \
         WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(source) = source else {
        return Ok(None);
    };
    let actor = source["actor"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(invalid)?;
    let mut hints = ReviewerAccountHints {
        actor,
        source,
        purpose: BusinessPurpose::Unavailable,
        mappings: Vec::new(),
    };
    if !hints.source["decision"].is_null() {
        hints.purpose = BusinessPurpose::Existing;
        return Ok(Some(hints));
    }
    let activation = match activation::preview_on(db, scope).await {
        Ok(Some(activation)) => activation,
        Ok(None) | Err(AppError::BadRequest(_) | AppError::WorkflowRole(_)) => {
            return Ok(Some(hints));
        }
        Err(error) => return Err(error),
    };
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(
            Uuid::parse_str(hints.source["binding"].as_str().ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
        ),
        scope.run.as_uuid(),
    )
    .await?;
    let spec = match specification(binding.bundle(), &activation) {
        Ok(spec) => spec,
        Err(AppError::BadRequest(_) | AppError::WorkflowRole(_)) => return Ok(Some(hints)),
        Err(error) => return Err(error),
    };
    let follow = spec.reviewers_follow_role
        && !ReviewerSelection::from_selectors(&spec.reviewers)
            .roles
            .is_empty();
    hints.purpose = BusinessPurpose::Prepare {
        inputs: activation.inputs,
        follow,
    };
    if follow {
        return Ok(Some(hints));
    }
    let reviewers = match reviewer_resolution::resolve_on(db, scope, &spec.reviewers).await {
        Ok(reviewers) => reviewers,
        Err(AppError::BadRequest(_) | AppError::WorkflowRole(_)) => {
            hints.purpose = BusinessPurpose::Unavailable;
            return Ok(Some(hints));
        }
        Err(error) => return Err(error),
    };
    hints.mappings=sqlx::query_as("SELECT id AS principal_id,user_id FROM principals WHERE company_id=$1 AND id=ANY($2) ORDER BY id")
        .bind(scope.company.as_uuid()).bind(&reviewers).fetch_all(db).await?;
    Ok(Some(hints))
}

fn changed() -> AppError {
    AppError::Conflict(
        "Workflow reviewer authority changed during account locking; retry the operation".into(),
    )
}
