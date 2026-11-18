//! Department eligibility for immutable role requirements. Callers hold company then channel.
use super::binding_rows::read_version;
use super::*;

fn needs_department(
    reference: &crate::application::workflow::publication::RoleReference,
) -> AppError {
    AppError::BadRequest(format!(
        "binding.role_needs_department: step {step} requires role {role} in an unambiguous department",
        step = reference.step,
        role = reference.role
    ))
}

pub(super) async fn check_on(
    tx: &mut PgConnection,
    configuration: &binding::ConfiguredBinding,
    association: RelatedAssociation,
) -> AppResult<()> {
    let reference = configuration.bundle().first_role_reference();
    let memory_own = crate::application::workflow::memory::needs_local_unit(configuration.bundle());
    let error = || match reference {
        _ if configuration.escalation().is_some() => escalation_needs_department(),
        Some(reference) => needs_department(reference),
        None if memory_own => memory_needs_department(),
        None => invalid(),
    };
    if reference.is_none() && configuration.escalation().is_none() && !memory_own {
        return Ok(());
    }
    if context_on(tx, configuration.company_id(), association)
        .await?
        .is_err()
    {
        return Err(error());
    }
    Ok(())
}

/// Roster mutations hold the company write lock, just like activation. Validate the final
/// placements, including the previous single-holder role occupant, before committing any edit.
pub(crate) async fn check_principals_on(
    tx: &mut PgConnection,
    company: CompanyId,
    principals: &[Uuid],
) -> AppResult<()> {
    const MAX_TARGETS: i64 = 256;
    let targets: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT principal_id FROM workflow_bindings WHERE company_id=$1 AND active \
         AND principal_id=ANY($2) ORDER BY principal_id LIMIT $3",
    )
    .bind(company.as_uuid())
    .bind(principals)
    .bind(MAX_TARGETS + 1)
    .fetch_all(&mut *tx)
    .await?;
    if targets.len() > MAX_TARGETS as usize {
        return Err(AppError::BadRequest(
            "binding.role_inspection_limit: deactivate bindings before changing these placements"
                .into(),
        ));
    }
    for principal in targets {
        let target =
            AddressTarget::Principal(crate::entities::transport::PrincipalId::new(principal));
        if context_on(tx, company, RelatedAssociation::inbox(target))
            .await?
            .is_err()
        {
            check_without_context_on(tx, company, target).await?;
        }
    }
    Ok(())
}

async fn check_without_context_on(
    tx: &mut PgConnection,
    company: CompanyId,
    target: AddressTarget,
) -> AppResult<()> {
    let escalating: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id=binding.company_id \
           AND configuration.binding_id=binding.id AND configuration.revision=binding.configuration_revision \
         WHERE binding.company_id=$1 AND binding.channel_id IS NOT DISTINCT FROM $2::uuid AND binding.principal_id IS NOT DISTINCT FROM $3::uuid AND binding.active \
           AND configuration.escalation IS NOT NULL)",
    )
    .bind(company.as_uuid())
    .bind(target.desk_id())
    .bind(target.principal_id().map(|id| id.as_uuid()))
    .fetch_one(&mut *tx)
    .await?;
    if escalating {
        return Err(escalation_needs_department());
    }
    // Limit work even for a desk with many manual bindings. Inspect one bundle at a time.
    const MAX_VERSIONS: i64 = 256;
    let versions: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT configuration.version_id FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
           AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
         WHERE binding.company_id = $1 AND binding.channel_id IS NOT DISTINCT FROM $2::uuid AND binding.principal_id IS NOT DISTINCT FROM $4::uuid AND binding.active \
         ORDER BY configuration.version_id LIMIT $3",
    )
    .bind(company.as_uuid())
    .bind(target.desk_id())
    .bind(MAX_VERSIONS + 1)
    .bind(target.principal_id().map(|id| id.as_uuid()))
    .fetch_all(&mut *tx)
    .await?;
    if versions.len() > MAX_VERSIONS as usize {
        return Err(AppError::BadRequest(
            "binding.role_inspection_limit: deactivate bindings before removing this department"
                .into(),
        ));
    }
    for version in versions {
        let bundle = read_version(&mut *tx, company, VersionId::new(version)).await?;
        if crate::application::workflow::memory::needs_local_unit(&bundle) {
            return Err(memory_needs_department());
        }
        if let Some(reference) = bundle.first_role_reference() {
            return Err(needs_department(reference));
        }
    }
    Ok(())
}

pub(super) fn memory_needs_department() -> AppError {
    AppError::BadRequest(
        "binding.memory_scope_needs_desk: own memory requires an unambiguous department".into(),
    )
}

fn escalation_needs_department() -> AppError {
    AppError::BadRequest(
        "binding.escalation_needs_department: escalation requires an unambiguous department".into(),
    )
}

/// Company serialization protects both the desk owner and the agent's current placements.
/// Two distinct departments are enough to prove ambiguity; role assignments also place an agent.
pub(super) async fn context_on(
    tx: &mut PgConnection,
    company: CompanyId,
    association: RelatedAssociation,
) -> AppResult<
    Result<crate::entities::department::DepartmentId, crate::entities::department::RoleRefusalKind>,
> {
    use crate::entities::department::{DepartmentId, RoleRefusalKind};
    let Some(target) = association.target() else {
        return Ok(Err(RoleRefusalKind::MissingDepartment));
    };
    let departments: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department FROM workflow_target_departments($1,$2,$3) AS department",
    )
    .bind(company.as_uuid())
    .bind(target.desk_id())
    .bind(target.principal_id().map(|id| id.as_uuid()))
    .fetch_all(tx)
    .await?;
    Ok(match departments.as_slice() {
        [] => Err(RoleRefusalKind::MissingDepartment),
        [department] => Ok(DepartmentId::new(*department)),
        _ => Err(RoleRefusalKind::Ambiguous),
    })
}
