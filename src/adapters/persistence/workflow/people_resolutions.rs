//! Immutable later People facts. Callers retain company serialization through assignment commit.
use super::*;
use crate::application::workflow::people_resolution::{PeopleResolution, PeopleResolutionRevision};
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::department::DepartmentRole;
use serde_json::Value;

pub(super) struct ResolutionKey<'a> {
    pub company: CompanyId,
    pub root: RunId,
    pub manifest: DependencyManifestId,
    pub version: VersionId,
    pub step: &'a StepId,
    pub role: DepartmentRole,
}

pub(super) async fn next_revision_on(
    db: &mut PgConnection,
    key: &ResolutionKey<'_>,
) -> AppResult<PeopleResolutionRevision> {
    // The same serialization belongs to admission, role writers and lifecycle changes.
    let company: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
            .bind(key.company.as_uuid())
            .fetch_optional(&mut *db)
            .await?;
    company.ok_or_else(missing)?;
    let previous: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(resolution_revision),0) FROM workflow_people_role_resolutions \
         WHERE company_id=$1 AND root_run_id=$2 AND manifest_id=$3 AND version_id=$4 AND step=$5 AND role=$6",
    )
    .bind(key.company.as_uuid()).bind(key.root.as_uuid()).bind(key.manifest.as_uuid())
    .bind(key.version.as_uuid()).bind(key.step.as_str()).bind(key.role.as_str())
    .fetch_one(db).await?;
    let next = previous.checked_add(1).ok_or_else(exhausted)?;
    PeopleResolutionRevision::try_from(u64::try_from(next).map_err(|_| exhausted())?)
        .map_err(|_| exhausted())
}

pub(super) async fn persist_on(db: &mut PgConnection, value: &PeopleResolution) -> AppResult<()> {
    let content: Value =
        serde_json::from_slice(&value.canonical_content()?).map_err(|_| invalid())?;
    let refusal = content["refusal"].as_str();
    let resolution = i64::try_from(value.resolution_revision.get()).map_err(|_| invalid())?;
    sqlx::query(
        "INSERT INTO workflow_people_role_resolutions(company_id,root_run_id,manifest_id,version_id,step,role, \
         resolution_revision,context_department_id,department_id,refusal,content) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).bind(value.manifest_id.as_uuid())
    .bind(value.version_id.as_uuid()).bind(value.step.as_str()).bind(value.role.as_str())
    .bind(resolution).bind(value.context_department_id.as_uuid())
    .bind(value.department_id.map(|department| department.as_uuid())).bind(refusal).bind(&content)
    .execute(&mut *db).await?;
    if value.principals.is_empty() {
        return Ok(());
    }
    let department = value.department_id.ok_or_else(invalid)?;
    for principal in &value.principals {
        sqlx::query(
            "INSERT INTO workflow_run_role_bindings(company_id,run_id,manifest_id,version_id,step,role, \
             resolution_revision,department_id,principal_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(value.company_id.as_uuid()).bind(value.root_run_id.as_uuid()).bind(value.manifest_id.as_uuid())
        .bind(value.version_id.as_uuid()).bind(value.step.as_str()).bind(value.role.as_str())
        .bind(resolution).bind(department.as_uuid()).bind(principal.as_uuid())
        .execute(&mut *db).await?;
    }
    Ok(())
}

pub(super) async fn read_on(
    db: &mut PgConnection,
    key: &ResolutionKey<'_>,
    revision: PeopleResolutionRevision,
) -> AppResult<PeopleResolution> {
    let content: Value = sqlx::query_scalar(
        "SELECT content FROM workflow_people_role_resolutions WHERE company_id=$1 AND root_run_id=$2 \
         AND manifest_id=$3 AND version_id=$4 AND step=$5 AND role=$6 AND resolution_revision=$7",
    )
    .bind(key.company.as_uuid()).bind(key.root.as_uuid()).bind(key.manifest.as_uuid())
    .bind(key.version.as_uuid()).bind(key.step.as_str()).bind(key.role.as_str())
    .bind(i64::try_from(revision.get()).map_err(|_| invalid())?)
    .fetch_optional(db).await?.ok_or_else(missing)?;
    PeopleResolution::decode(&serde_json::to_vec(&content).map_err(|_| invalid())?)
}

fn exhausted() -> AppError {
    AppError::Conflict(
        "workflow.role_resolution_limit: human role revision space is exhausted".into(),
    )
}
