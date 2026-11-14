//! Synchronous, company-serialized current assignments. Notification work stays durable.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::decisions::DecisionId;
use crate::application::workflow::people_resolution::{PeopleResolution, PeopleResolutionFormat};
use crate::application::workflow::reviewer_assignment::*;
use crate::application::workflow::reviewers::ReviewerSelector;
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::department::{DepartmentId, RoleRefusalKind};
use crate::entities::notification::NotificationEventId;
use crate::entities::transport::PrincipalId;
use serde_json::Value;
use std::collections::BTreeSet;

mod roles;
mod storage;
pub(super) use storage::withdraw_on;

#[derive(sqlx::FromRow)]
struct FollowDecision {
    company_id: Uuid,
    decision_id: Uuid,
    run_id: Uuid,
    root_run_id: Uuid,
    manifest_id: Uuid,
    version_id: Uuid,
    step: String,
    context_department_id: Uuid,
    selectors: Value,
    assignment_revision: i64,
}

impl FollowDecision {
    fn company(&self) -> CompanyId {
        CompanyId::new(self.company_id)
    }
    fn selection(&self) -> AppResult<ReviewerSelection> {
        let selectors = crate::application::workflow::reviewers::parse_reviewers(&self.selectors)
            .map_err(|_| invalid())?;
        Ok(ReviewerSelection::from_selectors(&selectors))
    }
    fn revision(&self) -> AppResult<ReviewerAssignmentRevision> {
        ReviewerAssignmentRevision::try_from(
            u64::try_from(self.assignment_revision).map_err(|_| limit())?,
        )
        .map_err(|_| limit())
    }
}

pub(super) fn enabled(follow: bool, selectors: &[ReviewerSelector]) -> bool {
    follow
        && !ReviewerSelection::from_selectors(selectors)
            .roles
            .is_empty()
}

/// Called after wait/decision creation, with the company and run already owned.
pub(super) async fn park_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    decision: Uuid,
) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_pending_follow_decisions WHERE company_id=$1",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    if count >= crate::application::workflow::escalation::MAX_PARKED_FOLLOW_DECISIONS as i64 {
        return Err(limit());
    }
    sqlx::query(
        "INSERT INTO workflow_follow_decisions(company_id,decision_id,run_id,execution_id,root_run_id,manifest_id, \
         version_id,step,context_department_id,selectors,selection,assignment_revision) \
         SELECT run.company_id,$4,run.id,execution.id,ownership.root_run_id,ownership.manifest_id,run.version_id,execution.step_id, \
         manifest.role_context_department_id,execution.frozen_inputs->'reviewers', \
         workflow_follow_selection(execution.frozen_inputs->'reviewers'),1 \
         FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id AND execution.id=$3 \
         JOIN run_dependency_manifests AS ownership ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
         JOIN agent_dependency_manifests AS manifest ON manifest.company_id=ownership.company_id AND manifest.id=ownership.manifest_id \
         WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(decision).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO workflow_pending_follow_decisions(company_id,decision_id) VALUES($1,$2)",
    )
    .bind(scope.company.as_uuid())
    .bind(decision)
    .execute(&mut *db)
    .await?;
    let current = storage::load_on(db, scope.company, decision).await?;
    replace_on(db, &current, None).await
}

/// Every placement/topology/lifecycle producer invokes this before committing final state.
/// The indexed pending set, enforced at park, bounds this to 256 decisions (never history).
pub(crate) async fn refresh_on(db: &mut PgConnection, company: CompanyId) -> AppResult<()> {
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(company.as_uuid())
        .execute(&mut *db)
        .await?;
    let decisions:Vec<Uuid>=sqlx::query_scalar(
        "SELECT decision_id FROM workflow_pending_follow_decisions WHERE company_id=$1 ORDER BY decision_id LIMIT 257")
        .bind(company.as_uuid()).fetch_all(&mut *db).await?;
    if decisions.len() > crate::application::workflow::escalation::MAX_PARKED_FOLLOW_DECISIONS {
        return Err(limit());
    }
    for decision in decisions {
        let current = storage::load_on(db, company, decision).await?;
        sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(company.as_uuid())
            .bind(current.run_id)
            .execute(&mut *db)
            .await?;
        let open: bool = sqlx::query_scalar("SELECT workflow_follow_is_open($1,$2)")
            .bind(company.as_uuid())
            .bind(decision)
            .fetch_one(&mut *db)
            .await?;
        if !open {
            continue;
        }
        sqlx::query("SELECT id FROM workflow_decisions WHERE company_id=$1 AND id=$2 FOR UPDATE")
            .bind(company.as_uuid())
            .bind(decision)
            .execute(&mut *db)
            .await?;
        let previous = storage::assignment_on(db, &current).await?;
        replace_on(db, &current, Some(previous)).await?;
    }
    Ok(())
}

async fn replace_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    previous: Option<ReviewerAssignment>,
) -> AppResult<()> {
    let selection = current.selection()?;
    let resolved = roles::resolve_on(db, current, &selection, previous.as_ref()).await?;
    let refusal = assignment_refusal_on(db, current, &selection, &resolved).await?;
    let principals = if refusal.is_none() {
        effective_principals(&selection, &resolved)
    } else {
        BTreeSet::new()
    };
    if previous.as_ref().is_some_and(|old| {
        old.roles == resolved.references
            && old.refusal == refusal
            && old
                .reviewers
                .iter()
                .map(|reviewer| reviewer.principal_id)
                .collect::<BTreeSet<_>>()
                == principals
    }) {
        return Ok(());
    }
    let revision = match previous.as_ref() {
        None => current.revision()?,
        Some(_) => ReviewerAssignmentRevision::try_from(
            u64::try_from(
                current
                    .assignment_revision
                    .checked_add(1)
                    .ok_or_else(limit)?,
            )
            .map_err(|_| limit())?,
        )
        .map_err(|_| limit())?,
    };
    let reviewers =
        storage::reviewers_on(db, current, revision, &principals, previous.as_ref()).await?;
    let assignment = ReviewerAssignment {
        format: ReviewerAssignmentFormat::V1,
        company_id: current.company(),
        decision_id: DecisionId(current.decision_id),
        root_run_id: RunId::new(current.root_run_id),
        manifest_id: DependencyManifestId::new(current.manifest_id),
        version_id: VersionId::new(current.version_id),
        step: StepId::parse(&current.step).map_err(|_| invalid())?,
        context_department_id: DepartmentId::new(current.context_department_id),
        assignment_revision: revision,
        roles: resolved.references,
        reviewers,
        refusal,
    };
    storage::persist_on(db, current, &assignment, previous.as_ref()).await?;
    if previous.is_some() {
        Box::pin(escalations::assignment_changed_on(
            db,
            current.company(),
            current.decision_id,
        ))
        .await?;
    }
    Ok(())
}

fn effective_principals(
    selection: &ReviewerSelection,
    resolved: &roles::ResolvedPeople,
) -> BTreeSet<PrincipalId> {
    selection
        .principals
        .iter()
        .copied()
        .chain(resolved.principals.iter().copied())
        .collect()
}

async fn assignment_refusal_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    selection: &ReviewerSelection,
    resolved: &roles::ResolvedPeople,
) -> AppResult<Option<ReviewerAssignmentRefusal>> {
    if resolved.unavailable {
        return Ok(Some(ReviewerAssignmentRefusal::RoleUnavailable));
    }
    let principals = effective_principals(selection, resolved);
    if principals.len() > registry::MAX_REVIEWERS {
        return Ok(Some(ReviewerAssignmentRefusal::TooMany));
    }
    let ids: Vec<_> = principals.iter().map(|id| id.as_uuid()).collect();
    let eligible:i64=sqlx::query_scalar("SELECT count(*) FROM unnest($3::uuid[]) AS principal(id) WHERE workflow_follow_reviewer_eligible($1,$2,principal.id)")
        .bind(current.company_id).bind(current.run_id).bind(&ids).fetch_one(db).await?;
    if usize::try_from(eligible).map_err(|_| invalid())? != principals.len() {
        return Ok(Some(ReviewerAssignmentRefusal::ReviewerUnavailable));
    }
    Ok(None)
}

fn limit() -> AppError {
    AppError::Conflict(
        "workflow.pending_follow_limit: followed decision or assignment limit reached".into(),
    )
}
