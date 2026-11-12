//! Department eligibility for immutable role requirements. Callers hold company then channel.
use super::binding_rows::read_version;
use super::*;

fn needs_department(
    reference: &crate::application::workflow::publication::RoleReference,
) -> AppError {
    AppError::BadRequest(format!(
        "binding.role_needs_department: step {step} requires role {role} on a department-owned desk",
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
    let error = || match reference {
        _ if configuration.escalation().is_some() => escalation_needs_department(),
        Some(reference) => needs_department(reference),
        None => invalid(),
    };
    if reference.is_none() && configuration.escalation().is_none() {
        return Ok(());
    }
    let channel = match association {
        RelatedAssociation::Company => return Err(error()),
        RelatedAssociation::Channel(channel)
        | RelatedAssociation::Thread {
            channel_id: channel,
            ..
        } => channel,
    };
    let department: Option<Uuid> = sqlx::query_scalar(
        "SELECT department_id FROM channels WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(configuration.company_id().as_uuid())
    .bind(channel.as_uuid())
    .fetch_optional(tx)
    .await?
    .ok_or_else(missing)?;
    if department.is_none() {
        return Err(error());
    }
    Ok(())
}

/// Both ownership writers call this only when removing an existing department, with the
/// company write lock and channel UPDATE lock held. Concurrent activation shares these locks.
pub(crate) async fn check_detach_on(
    tx: &mut PgConnection,
    company: CompanyId,
    channel: Uuid,
) -> AppResult<()> {
    let escalating: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id=binding.company_id \
           AND configuration.binding_id=binding.id AND configuration.revision=binding.configuration_revision \
         WHERE binding.company_id=$1 AND binding.channel_id=$2 AND binding.active \
           AND configuration.escalation IS NOT NULL)",
    )
    .bind(company.as_uuid())
    .bind(channel)
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
         WHERE binding.company_id = $1 AND binding.channel_id = $2 AND binding.active \
         ORDER BY configuration.version_id LIMIT $3",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .bind(MAX_VERSIONS + 1)
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
        if let Some(reference) = bundle.first_role_reference() {
            return Err(needs_department(reference));
        }
    }
    Ok(())
}

fn escalation_needs_department() -> AppError {
    AppError::BadRequest(
        "binding.escalation_needs_department: escalation requires a department-owned desk".into(),
    )
}
