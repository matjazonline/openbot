use super::*;
use binding::{BindingConfiguration, ConfiguredBinding};
use rows::{VERSION_COLUMNS, VersionRow};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct BindingRow {
    company_id: Uuid,
    id: Uuid,
    state_revision: i64,
    configuration_revision: i64,
    active: bool,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    version_id: Uuid,
    params: Value,
    resources: Value,
}

pub(super) async fn read_binding(
    db: &mut PgConnection,
    company: CompanyId,
    id: WorkflowBindingId,
) -> AppResult<Option<BindingState>> {
    let row = sqlx::query_as::<_, BindingRow>(
        "SELECT binding.company_id, binding.id, binding.state_revision, \
         binding.configuration_revision, binding.active, binding.channel_id, binding.thread_id, \
         configuration.version_id, configuration.params, configuration.resources \
         FROM workflow_bindings AS binding JOIN workflow_binding_revisions AS configuration \
           ON configuration.company_id = binding.company_id AND configuration.binding_id = binding.id \
           AND configuration.revision = binding.configuration_revision \
         WHERE binding.company_id = $1 AND binding.id = $2",
    ).bind(company.as_uuid()).bind(id.as_uuid()).fetch_optional(&mut *db).await?;
    let Some(row) = row else { return Ok(None) };
    let bundle = read_version(db, company, VersionId::new(row.version_id)).await?;
    let association = match (row.channel_id, row.thread_id) {
        (None, None) => RelatedAssociation::Company,
        (Some(channel), None) => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
        (Some(channel), Some(thread)) => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel),
            thread_id: RelatedThreadId::new(thread),
        },
        _ => return Err(invalid()),
    };
    let configuration = ConfiguredBinding::new(
        BindingConfiguration {
            company_id: CompanyId::new(row.company_id),
            id: WorkflowBindingId::new(row.id),
            revision: BindingRevision::new(
                u64::try_from(row.configuration_revision).map_err(|_| invalid())?,
            )
            .ok_or_else(invalid)?,
            params: row.params,
            resources: serde_json::from_value(row.resources).map_err(|_| invalid())?,
        },
        bundle,
    )
    .map_err(|_| invalid())?;
    Ok(Some(BindingState {
        configuration: Arc::new(configuration),
        association,
        active: row.active,
        revision: BindingStateRevision::new(
            u64::try_from(row.state_revision).map_err(|_| invalid())?,
        )
        .ok_or_else(invalid)?,
    }))
}

pub(super) async fn read_version(
    db: &mut PgConnection,
    company: CompanyId,
    version: VersionId,
) -> AppResult<Arc<PublishedBundle>> {
    sqlx::query_as::<_, VersionRow>(&format!(
        "SELECT {VERSION_COLUMNS} FROM workflow_versions WHERE company_id = $1 AND id = $2"
    ))
    .bind(company.as_uuid())
    .bind(version.as_uuid())
    .fetch_optional(db)
    .await?
    .ok_or_else(missing)?
    .restore()
    .map(|command| command.bundle)
}

pub(super) fn association_columns(association: RelatedAssociation) -> (Option<Uuid>, Option<Uuid>) {
    match association {
        RelatedAssociation::Company => (None, None),
        RelatedAssociation::Channel(channel) => (Some(channel.as_uuid()), None),
        RelatedAssociation::Thread {
            channel_id,
            thread_id,
        } => (Some(channel_id.as_uuid()), Some(thread_id.as_uuid())),
    }
}
