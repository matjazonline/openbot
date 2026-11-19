use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::{collections::HashSet, str::FromStr};
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::department_desks::{prelock_ownership_on, sync_desk_grants_on};
use crate::adapters::persistence::integration::email_binding::{
    CanonicalEmailBinding, write_canonical_email_binding,
};
use crate::adapters::persistence::participant::resolve_or_create_external_identity_on;
use crate::adapters::persistence::skill::copy_library_skill_on;
use crate::adapters::protocols::email::EmailIdentity;
use crate::app_error::AppError;
use crate::app_error::AppResult;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::channel::ChannelUpdate;
use crate::application::ports::channel::ChannelWrite;
use crate::application::ports::participant::IdentityObservation;
use crate::domain::workflow::CompanyId;
use crate::entities::address::{AddressTarget, ResolvedAddress};
use crate::entities::channel::Channel;
use crate::entities::channel::ChannelAccessMode;
use crate::entities::creation::CreationProvenance;
use crate::entities::department::DepartmentId;
use crate::entities::participant::IdentityClaimMetadata;
use crate::entities::participant::IdentityProvenance;
use crate::entities::participant::PrincipalCapability;
use crate::entities::transport::PrincipalId;
use crate::entities::value_objects::ChannelSlug;
use crate::entities::value_objects::CompanySlug;
use crate::entities::value_objects::EmailAddress;
use crate::use_cases::agent::AgentWrite;
use crate::use_cases::participant::IdentityResolution;

#[derive(sqlx::FromRow, Debug, Serialize)]
pub struct ChannelDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub slug: String,
    pub alias_slugs: Vec<String>,
    pub participant_emails: Option<Vec<String>>,
    pub access_mode: String,
    pub principal_grants: serde_json::Value,
    pub enabled: bool,
    pub add_3rd_party: bool,
    pub settings_revision: i64,
    pub created_by: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub department_id: Uuid,
}

impl TryFrom<ChannelDb> for Channel {
    type Error = AppError;

    fn try_from(db: ChannelDb) -> AppResult<Self> {
        Ok(Channel {
            id: db.id,
            company_id: db.company_id,
            name: db.name,
            description: db.description,
            slug: ChannelSlug::from(db.slug),
            alias_slugs: db.alias_slugs.into_iter().map(ChannelSlug::from).collect(),
            participant_emails: db
                .participant_emails
                .map(|emails| emails.into_iter().map(EmailAddress::from).collect()),
            access_mode: ChannelAccessMode::from_str(&db.access_mode)
                .map_err(|error| AppError::Internal(error.to_string()))?,
            principal_grants: serde_json::from_value(db.principal_grants).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid channel principal grants for {}: {error}",
                    db.id
                ))
            })?,
            enabled: db.enabled,
            add_3rd_party: db.add_3rd_party,
            settings_revision: db.settings_revision,
            created_by: serde_json::from_value(db.created_by).map_err(|err| {
                AppError::Internal(format!("Invalid channels.created_by provenance: {err}"))
            })?,
            created_at: db.created_at,
            department_id: DepartmentId::new(db.department_id),
        })
    }
}

const CHANNEL_SELECT: &str = r#"
    SELECT channel.id, channel.company_id, channel.name, channel.description,
           (SELECT address.slug::text FROM addresses AS address
            WHERE address.company_id = channel.company_id AND address.channel_id = channel.id
              AND address.is_primary) AS slug,
           COALESCE(
               (SELECT array_agg(address.slug::text ORDER BY address.slug::text)
                FROM addresses AS address
                WHERE address.company_id = channel.company_id AND address.channel_id = channel.id
                  AND NOT address.is_primary),
               ARRAY[]::text[]) AS alias_slugs,
           NULLIF((CASE WHEN channel.access_mode='public' THEN ARRAY['@public']::text[] ELSE ARRAY[]::text[] END)
           || COALESCE(
               (SELECT array_agg(identity.subject ORDER BY identity.subject)
                  FROM channel_principal_grants AS channel_grant
                  JOIN participant_identities AS identity
                    ON (identity.company_id, identity.principal_id) =
                       (channel_grant.company_id, channel_grant.principal_id)
                 WHERE channel_grant.company_id=channel.company_id AND channel_grant.channel_id=channel.id
                   AND channel_grant.capability='participate'
                   AND channel_grant.provenance='configured_allowlist'
                   AND identity.transport='email' AND identity.status<>'disabled'),
               ARRAY[]::text[]), ARRAY[]::text[]) AS participant_emails,
           channel.access_mode,
           COALESCE(
               (SELECT jsonb_agg(jsonb_build_object(
                           'principal_id', channel_grant.principal_id,
                           'capability', channel_grant.capability,
                           'provenance', channel_grant.provenance,
                           'created_at', channel_grant.created_at)
                       ORDER BY channel_grant.created_at, channel_grant.principal_id, channel_grant.capability)
                FROM channel_principal_grants AS channel_grant
                WHERE channel_grant.company_id = channel.company_id AND channel_grant.channel_id = channel.id),
               '[]'::jsonb) AS principal_grants,
           channel.enabled, channel.add_3rd_party, channel.settings_revision, channel.created_by, channel.created_at,
           channel.department_id
    FROM channels AS channel
"#;

async fn load_channel(persistence: &PostgresPersistence, id: Uuid) -> AppResult<Option<Channel>> {
    let query = format!("{CHANNEL_SELECT} WHERE channel.id = $1");
    let db = sqlx::query_as::<_, ChannelDb>(&query)
        .bind(id)
        .fetch_optional(&persistence.pool)
        .await
        .map_err(AppError::from)?;

    db.map(TryInto::try_into).transpose()
}

/// Write a channel's canonical slug plus its aliases into the company's address namespace, which
/// desks share with people and agents.
///
/// One statement per slug so a `(company_id, slug)` primary-key violation can name the address
/// that actually collided; the caller's transaction rolls the partial write back.
pub(super) async fn insert_desk_addresses(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    slug: &str,
    alias_slugs: &[String],
) -> AppResult<()> {
    for (candidate, is_primary) in
        std::iter::once((slug, true)).chain(alias_slugs.iter().map(|alias| (alias.as_str(), false)))
    {
        sqlx::query(
            r#"INSERT INTO addresses (company_id, channel_id, slug, is_primary, target_kind)
               VALUES ($1, $2, $3, $4, 'desk')"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(candidate)
        .bind(is_primary)
        .execute(&mut **tx)
        .await
        .map_err(|err| slug_conflict_error(err, candidate))?;
    }

    Ok(())
}

/// A taken address is routine user input, not a database fault, so it surfaces as a bad request
/// with the offending slug rather than a raw driver message.
fn slug_conflict_error(err: sqlx::Error, slug: &str) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err
        && db_err.code().as_deref() == Some("23505")
    {
        return AppError::BadRequest(format!(
            "Address '{slug}' is already in use by a desk, person or agent in this company."
        ));
    }
    AppError::from(err)
}

/// Turn the form's email allowlist into the channel's stored access mode and the addresses that
/// become principal grants. `@public` selects a mode; it is never a participant of its own.
pub(super) fn channel_access(
    participant_emails: Option<Vec<String>>,
) -> (ChannelAccessMode, Vec<String>) {
    let mut seen = HashSet::new();
    let mut participants = Vec::new();
    let mut is_public = false;

    for email in participant_emails.unwrap_or_default() {
        let normalized = email.trim().to_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if normalized == "@public" {
            is_public = true;
        } else if seen.insert(normalized.clone()) {
            participants.push(normalized);
        }
    }

    let mode = if is_public {
        ChannelAccessMode::Public
    } else {
        ChannelAccessMode::Members
    };

    (mode, participants)
}

pub(crate) async fn insert_email_allowlist_grants(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    participants: Vec<String>,
) -> AppResult<()> {
    for email in participants {
        let identity = EmailIdentity::parse(EmailAddress::from(email))
            .map(EmailIdentity::qualify_default)
            .map_err(|error| AppError::BadRequest(format!("Invalid participant email: {error}")))?;
        let IdentityResolution { principal, .. } = resolve_or_create_external_identity_on(
            transaction,
            company_id,
            IdentityObservation {
                identity,
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance: IdentityProvenance::ChannelAllowlist,
            },
        )
        .await?;

        // Each provenance owns its own row, so this allowlist entry and a grant the principal holds
        // from elsewhere (a department, a manager) never delete or relabel one another.
        for capability in [PrincipalCapability::Participate, PrincipalCapability::View] {
            sqlx::query(
                r#"INSERT INTO channel_principal_grants
                       (company_id, channel_id, principal_id, capability, provenance)
                   VALUES ($1, $2, $3, $4, 'configured_allowlist')
                   ON CONFLICT (company_id, channel_id, principal_id, capability, provenance)
                   DO NOTHING"#,
            )
            .bind(company_id)
            .bind(channel_id)
            .bind(principal.id.as_uuid())
            .bind(capability.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(AppError::from)?;
        }
    }
    Ok(())
}

/// Every channel owns one canonical email interface, written in the same transaction as the
/// channel itself so the two can never disagree.
///
/// The three creation paths and the update path all route through here rather than each building
/// their own binding: the endpoint key, access policy and delivery policy are one decision.
pub(super) async fn write_channel_email_binding(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    write: &ChannelWrite,
) -> AppResult<()> {
    let channel_slug = ChannelSlug::new(write.slug.clone());
    let created_by = write
        .created_by
        .clone()
        .unwrap_or_else(CreationProvenance::system);
    write_canonical_email_binding(
        transaction,
        CanonicalEmailBinding {
            company_id,
            target: crate::entities::address::AddressTarget::Desk(channel_id),
            slug: &channel_slug,
            name: &write.name,
            created_by: &created_by,
        },
    )
    .await
}

/// The `channels` row of a new channel, with its stored access mode; every creation path writes
/// it here, then its slugs, email binding and allowlist grants through their own helpers. Returns
/// the participants that become allowlist grants.
pub(crate) async fn insert_channel_row_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    write: &ChannelWrite,
) -> AppResult<Vec<String>> {
    let (access_mode, participants) = channel_access(write.participant_emails.clone());
    let department =
        prelock_ownership_on(tx, CompanyId::new(company_id), None, write.department_id).await?;
    let created_by = serde_json::to_value(
        write
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system),
    )
    .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        r#"INSERT INTO channels (
                id, company_id, name, description, access_mode, enabled, add_3rd_party,
                created_by, department_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
    )
    .bind(channel_id)
    .bind(company_id)
    .bind(&write.name)
    .bind(&write.description)
    .bind(access_mode.as_str())
    .bind(write.enabled)
    .bind(write.add_3rd_party)
    .bind(created_by)
    .bind(department.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sync_desk_grants_on(tx, CompanyId::new(company_id), channel_id).await?;
    Ok(participants)
}

#[async_trait]
impl ChannelPersistence for PostgresPersistence {
    async fn create(
        &self,
        actor: crate::application::workflow::WorkflowActor,
        company_id: Uuid,
        write: ChannelWrite,
    ) -> AppResult<Channel> {
        let uuid = Uuid::new_v4();
        let mut tx = super::workflow::management::begin(
            self,
            crate::domain::workflow::CompanyId::new(company_id),
            actor,
        )
        .await?;
        use crate::application::ports::administration::ManagementTarget;
        super::workflow::management::authorize_target_on(
            &mut tx,
            crate::domain::workflow::CompanyId::new(company_id),
            actor,
            write
                .department_id
                .map_or(ManagementTarget::Root, ManagementTarget::Unit),
        )
        .await?;
        let participants = insert_channel_row_on(&mut tx, company_id, uuid, &write).await?;

        insert_desk_addresses(&mut tx, company_id, uuid, &write.slug, &write.alias_slugs).await?;

        write_channel_email_binding(&mut tx, company_id, uuid, &write).await?;

        insert_email_allowlist_grants(&mut tx, company_id, uuid, participants).await?;

        tx.commit().await.map_err(AppError::from)?;
        load_channel(self, uuid)
            .await?
            .ok_or_else(|| AppError::Internal("Created channel was not found".into()))
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        load_channel(self, id).await
    }

    async fn get_by_company_slug_and_channel_slug(
        &self,
        company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        let query = format!(
            "{CHANNEL_SELECT} JOIN companies c ON c.id = channel.company_id \
             JOIN addresses AS lookup ON lookup.company_id = channel.company_id \
               AND lookup.channel_id = channel.id \
             WHERE c.slug = $1 AND lookup.slug = $2"
        );
        let db = sqlx::query_as::<_, ChannelDb>(&query)
            .bind(company_slug.as_str())
            .bind(channel_slug.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_departments(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::department::Department>> {
        crate::adapters::persistence::department::departments_of_company(
            &self.pool,
            crate::domain::workflow::CompanyId::new(company_id),
        )
        .await
    }

    async fn address_for_target(
        &self,
        company_id: Uuid,
        target: AddressTarget,
    ) -> AppResult<Option<ResolvedAddress>> {
        let slug: Option<String> = sqlx::query_scalar(
            "SELECT slug::text FROM addresses WHERE company_id=$1 AND is_primary \
             AND channel_id IS NOT DISTINCT FROM $2 AND principal_id IS NOT DISTINCT FROM $3",
        )
        .bind(company_id)
        .bind(target.desk_id())
        .bind(target.principal_id().map(PrincipalId::as_uuid))
        .fetch_optional(&self.pool)
        .await?;
        Ok(slug.map(|slug| ResolvedAddress {
            slug: slug.into(),
            target,
        }))
    }

    async fn address_targets(
        &self,
        company_id: Uuid,
        slugs: &[ChannelSlug],
    ) -> AppResult<Vec<ResolvedAddress>> {
        let slugs: Vec<&str> = slugs.iter().map(|slug| slug.as_str()).collect();
        let rows: Vec<(String, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
            "SELECT address.slug::text, address.channel_id, address.principal_id \
               FROM addresses AS address \
              WHERE address.company_id = $1 AND address.slug = ANY($2::citext[])",
        )
        .bind(company_id)
        .bind(&slugs)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        rows.into_iter()
            .map(|(slug, desk, principal)| {
                let target = match (desk, principal) {
                    (Some(desk), None) => AddressTarget::Desk(desk),
                    (None, Some(principal)) => {
                        AddressTarget::Principal(PrincipalId::new(principal))
                    }
                    _ => {
                        return Err(AppError::Internal(format!(
                            "Address '{slug}' names neither exactly one desk nor one principal"
                        )));
                    }
                };
                Ok(ResolvedAddress {
                    slug: ChannelSlug::from(slug),
                    target,
                })
            })
            .collect()
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        let query = format!(
            "{CHANNEL_SELECT} WHERE channel.company_id = $1 ORDER BY channel.created_at DESC, channel.id DESC"
        );
        let db_list = sqlx::query_as::<_, ChannelDb>(&query)
            .bind(company_id)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update(&self, request: ChannelUpdate) -> AppResult<Channel> {
        let channel_id = request.channel_id;
        crate::adapters::persistence::channel_edit::update_channel_within(
            &self.pool,
            request,
            crate::adapters::persistence::channel_edit::EditBudget::DEFAULT,
        )
        .await?;
        load_channel(self, channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Updated channel was not found".into()))
    }

    async fn delete(
        &self,
        actor: crate::application::workflow::WorkflowActor,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<()> {
        let mut tx = super::workflow::management::begin(
            self,
            crate::domain::workflow::CompanyId::new(company_id),
            actor,
        )
        .await?;
        super::workflow::management::authorize_association_on(
            &mut tx,
            crate::domain::workflow::CompanyId::new(company_id),
            actor,
            crate::application::workflow::RelatedAssociation::Channel(
                crate::application::workflow::RelatedChannelId::new(id),
            ),
        )
        .await?;
        let channel: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM channels WHERE id=$1 AND company_id=$2 FOR UPDATE")
                .bind(id)
                .bind(company_id)
                .fetch_optional(&mut *tx)
                .await?;
        channel.ok_or_else(crate::use_cases::channel::channel_not_found)?;
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM workflow_bindings WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM channel_schedules WHERE channel_id=$1) \
             OR EXISTS(SELECT 1 FROM workflow_runs WHERE channel_id=$1)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if retained {
            return Err(AppError::Conflict(
                "This channel has retained conversations or workflow configuration and cannot be deleted"
                    .into(),
            ));
        }
        sqlx::query("DELETE FROM channels WHERE id=$1 AND company_id=$2")
            .bind(id)
            .bind(company_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if error
                    .as_database_error()
                    .is_some_and(|error| error.is_foreign_key_violation())
                {
                    AppError::Conflict(
                        "This channel has retained conversations or workflow configuration and cannot be deleted"
                            .into(),
                    )
                } else {
                    AppError::from(error)
                }
            })?;
        tx.commit().await?;
        Ok(())
    }
}

/// Every setting one channel edit writes, on the caller's transaction.
///
/// The caller holds the channel's gate and row lock and has checked its tenant
/// (`channel_edit`).
pub(super) async fn write_channel_settings_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    id: Uuid,
    write: &ChannelWrite,
    department: DepartmentId,
) -> AppResult<()> {
    let (access_mode, participants) = channel_access(write.participant_emails.clone());
    sqlx::query(
        r#"UPDATE channels
               SET name = $1, description = $2, access_mode = $3, enabled = $4,
                   add_3rd_party = $5, department_id = $8
               WHERE id = $6 AND company_id = $7"#,
    )
    .bind(&write.name)
    .bind(&write.description)
    .bind(access_mode.as_str())
    .bind(write.enabled)
    .bind(write.add_3rd_party)
    .bind(id)
    .bind(company_id)
    .bind(department.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    // Only the grants this form owns: a grant written by another provenance is not the email
    // allowlist's to replace.
    sqlx::query(
        r#"DELETE FROM channel_principal_grants
               WHERE channel_id = $1 AND provenance = 'configured_allowlist'"#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query("DELETE FROM addresses WHERE company_id = $1 AND channel_id = $2")
        .bind(company_id)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    insert_desk_addresses(tx, company_id, id, &write.slug, &write.alias_slugs).await?;

    // A renamed address moves the channel's canonical email interface rather than replacing
    // it, so the binding's audit history survives the rename.
    write_channel_email_binding(tx, company_id, id, write).await?;

    insert_email_allowlist_grants(tx, company_id, id, participants).await?;
    Ok(())
}

/// Copy a library definition into the company inside `tx`: its global skills become tenant-owned
/// copies the agent is attached to, and its portable builtin grants come with it. Its model is
/// resolved by company-agent creation, like every company agent's (01DEP-P03-D3).
pub(crate) async fn prepare_library_agent_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    library_agent_id: Uuid,
    agent: &mut AgentWrite,
) -> AppResult<()> {
    let source_exists = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM agents \
             WHERE id = $1 AND company_id IS NULL FOR KEY SHARE",
    )
    .bind(library_agent_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .is_some();
    if !source_exists {
        return Err(AppError::NotFound("Library agent not found.".into()));
    }

    let source_skill_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT skill_id FROM agent_skills \
             WHERE agent_id = $1 ORDER BY position",
    )
    .bind(library_agent_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let created_by = agent
        .created_by
        .clone()
        .unwrap_or_else(CreationProvenance::system);
    let mut copied_skill_ids = Vec::with_capacity(source_skill_ids.len());
    for source_skill_id in source_skill_ids {
        let copied = copy_library_skill_on(tx, company_id, source_skill_id, &created_by).await?;
        copied_skill_ids.push(copied.id);
    }
    agent.skill_ids = copied_skill_ids;

    // Library bindings are builtin-only; native scope constraints forbid tenant targets.
    let mut bindings = super::agent_capability::bindings_on(tx, None, library_agent_id).await?;
    if !bindings.is_empty() {
        for binding in &mut bindings {
            binding.id = crate::entities::agent_capability::AgentBindingId::new(Uuid::new_v4());
        }
        agent.tool_bindings = Some(bindings);
    }
    Ok(())
}

/// Visibility inputs for a requested run set; never loads unrelated company channels.
pub(super) async fn channels_by_ids(
    p: &PostgresPersistence,
    company: Uuid,
    ids: &[Uuid],
) -> AppResult<Vec<Channel>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let query = format!("{CHANNEL_SELECT} WHERE channel.company_id=$1 AND channel.id=ANY($2)");
    let rows = sqlx::query_as::<_, ChannelDb>(&query)
        .bind(company)
        .bind(ids)
        .fetch_all(p.pool())
        .await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "channel_allowlist_tests.rs"]
mod allowlist_tests;
