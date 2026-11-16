//! A parked decision copies identities from the admitted run, never current department roles.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::reviewer_assignment::ReviewerSelection;
use crate::application::workflow::reviewers::ReviewerSelector;
use crate::entities::department::{RoleAdmissionRefusal, RoleRefusalKind};
use crate::entities::transport::PrincipalId;

#[derive(sqlx::FromRow)]
struct ReviewStep {
    version_id: Uuid,
    step_id: String,
}
#[derive(sqlx::FromRow)]
struct CapturedReviewer {
    role: String,
    principal_id: Uuid,
}

pub(super) async fn resolve_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    selectors: &[ReviewerSelector],
) -> AppResult<Vec<Uuid>> {
    let ReviewerSelection {
        mut principals,
        roles,
    } = ReviewerSelection::from_selectors(selectors);
    if principals.len() > registry::MAX_REVIEWERS {
        return Err(invalid());
    }
    if roles.is_empty() {
        return Ok(principals.into_iter().map(|id| id.as_uuid()).collect());
    }
    let step: ReviewStep = sqlx::query_as(
        "SELECT run.version_id,execution.step_id FROM workflow_runs AS run \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    let names: Vec<_> = roles.iter().map(|role| role.as_str().to_owned()).collect();
    let captured: Vec<CapturedReviewer> = sqlx::query_as(
        "SELECT captured.role,captured.principal_id FROM run_dependency_manifests AS ownership \
         JOIN workflow_run_role_bindings AS captured ON captured.company_id=ownership.company_id \
           AND captured.manifest_id=ownership.manifest_id AND captured.run_id=ownership.root_run_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND captured.version_id=$3 \
           AND captured.step=$4 AND captured.role=ANY($5) AND captured.resolution_revision=0 \
           AND captured.agent_id IS NULL AND captured.agent_revision_id IS NULL \
         ORDER BY captured.role,captured.principal_id LIMIT 33")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(step.version_id).bind(&step.step_id)
        .bind(names).fetch_all(&mut *db).await?;
    let step_id = StepId::parse(&step.step_id).map_err(|_| invalid())?;
    for role in roles {
        let refused =
            |kind| AppError::WorkflowRole(RoleAdmissionRefusal::new(step_id.clone(), role, kind));
        let members: Vec<_> = captured
            .iter()
            .filter(|row| row.role == role.as_str())
            .collect();
        if members.is_empty() {
            return Err(refused(RoleRefusalKind::NoHolder));
        }
        for member in members {
            let id = PrincipalId::new(member.principal_id);
            if !principals.contains(&id) {
                principals.push(id);
            }
        }
        if principals.len() > registry::MAX_REVIEWERS {
            return Err(refused(RoleRefusalKind::TooMany));
        }
    }
    Ok(principals.into_iter().map(|id| id.as_uuid()).collect())
}
