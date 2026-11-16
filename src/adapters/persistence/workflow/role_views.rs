use super::*;
use crate::application::workflow::reads::{RunRoleHolder, RunSummary};
use crate::application::workflow::run_views::MAX_STEP_ITEMS;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::transport::PrincipalId;

#[derive(sqlx::FromRow)]
struct HolderRow {
    run: Uuid,
    version_id: Uuid,
    step: String,
    role: String,
    department_id: Uuid,
    principal_id: Uuid,
    display_label: String,
}

impl HolderRow {
    fn holder(self) -> AppResult<RunRoleHolder> {
        Ok(RunRoleHolder {
            version: VersionId::new(self.version_id),
            step: StepId::parse(self.step).map_err(|_| invalid())?,
            role: self.role.parse::<DepartmentRole>().map_err(|_| invalid())?,
            department: DepartmentId::new(self.department_id),
            principal: PrincipalId::new(self.principal_id),
            label: self.display_label,
        })
    }
}

/// A bounded batch, one extra row per run to expose truncation. No per-run query loop.
pub(super) async fn populate(
    persistence: &PostgresPersistence,
    company: CompanyId,
    runs: &mut [RunSummary],
) -> AppResult<()> {
    if runs.is_empty() {
        return Ok(());
    }
    let ids: Vec<_> = runs.iter().map(|run| run.run.as_uuid()).collect();
    let positions: std::collections::HashMap<_, _> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect();
    let rows: Vec<HolderRow> = tokio::time::timeout(std::time::Duration::from_secs(5), sqlx::query_as(
        "SELECT requested.id AS run,captured.version_id,captured.step,captured.role,captured.department_id, \
                captured.principal_id,principal.display_label FROM unnest($2::uuid[]) AS requested(id) \
         CROSS JOIN LATERAL (SELECT role_binding.version_id,role_binding.step,role_binding.role, \
                role_binding.department_id,role_binding.principal_id FROM run_dependency_manifests AS ownership \
             JOIN workflow_run_role_bindings AS role_binding ON role_binding.company_id=ownership.company_id \
               AND role_binding.run_id=ownership.root_run_id AND role_binding.manifest_id=ownership.manifest_id \
             WHERE ownership.company_id=$1 AND ownership.run_id=requested.id AND role_binding.resolution_revision=0 \
             ORDER BY role_binding.version_id,role_binding.step,role_binding.role,role_binding.principal_id LIMIT $3) AS captured \
         JOIN principals AS principal ON principal.company_id=$1 AND principal.id=captured.principal_id \
         ORDER BY requested.id,captured.version_id,captured.step,captured.role,captured.principal_id",
    ).bind(company.as_uuid()).bind(ids).bind(MAX_STEP_ITEMS+1).fetch_all(persistence.pool()))
        .await.map_err(|_| AppError::Timeout("Role holder inspection exceeded five seconds".into()))??;
    for row in rows {
        let index = *positions.get(&row.run).ok_or_else(invalid)?;
        if runs[index].role_holders.len() == MAX_STEP_ITEMS as usize {
            runs[index].role_holders_truncated = true;
        } else {
            runs[index].role_holders.push(row.holder()?);
        }
    }
    Ok(())
}

pub(super) async fn for_step(
    persistence: &PostgresPersistence,
    company: CompanyId,
    run: RunId,
    version: VersionId,
    step: &StepId,
) -> AppResult<Option<RunRoleHolder>> {
    let row: Option<HolderRow> = tokio::time::timeout(std::time::Duration::from_secs(5), sqlx::query_as(
        "SELECT ownership.run_id AS run,role_binding.version_id,role_binding.step,role_binding.role, \
             role_binding.department_id,role_binding.principal_id,principal.display_label \
         FROM run_dependency_manifests AS ownership \
         JOIN workflow_run_role_bindings AS role_binding ON role_binding.company_id=ownership.company_id \
           AND role_binding.run_id=ownership.root_run_id AND role_binding.manifest_id=ownership.manifest_id \
         JOIN principals AS principal ON principal.company_id=role_binding.company_id AND principal.id=role_binding.principal_id \
         WHERE ownership.company_id=$1 AND ownership.run_id=$2 AND role_binding.version_id=$3 \
           AND role_binding.step=$4 AND role_binding.resolution_revision=0 AND role_binding.agent_id IS NOT NULL",
    ).bind(company.as_uuid()).bind(run.as_uuid()).bind(version.as_uuid()).bind(step.as_str()).fetch_optional(persistence.pool()))
        .await.map_err(|_| AppError::Timeout("Role holder inspection exceeded five seconds".into()))??;
    row.map(HolderRow::holder).transpose()
}
