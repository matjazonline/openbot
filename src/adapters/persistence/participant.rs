use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapters::protocols::email::EmailIdentity;
use crate::entities::value_objects::EmailAddress;
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        company_member::CompanyMembership,
        participant::{
            IdentityClaimMetadata, IdentityProvenance, IdentityStatus, ParticipantIdentity,
            Principal, PrincipalAccessContext, PrincipalKind,
        },
        transport::{
            IdentityNamespace, IdentitySubject, ParticipantIdentityId, PrincipalId,
            QualifiedIdentity, TransportKind,
        },
    },
    use_cases::participant::{
        IdentityDirectory, IdentityObservation, IdentityResolution, PrincipalAccessPersistence,
    },
};

#[derive(sqlx::FromRow)]
struct PrincipalDb {
    id: Uuid,
    company_id: Uuid,
    kind: String,
    user_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    display_label: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PrincipalDb> for Principal {
    type Error = AppError;

    fn try_from(row: PrincipalDb) -> AppResult<Self> {
        Ok(Self {
            id: PrincipalId::new(row.id),
            company_id: row.company_id,
            kind: PrincipalKind::from_str(&row.kind).map_err(|error| {
                AppError::Internal(format!("Invalid principals.kind for {}: {error}", row.id))
            })?,
            user_id: row.user_id,
            agent_id: row.agent_id,
            display_label: row.display_label,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct IdentityDb {
    id: Uuid,
    company_id: Uuid,
    principal_id: Uuid,
    transport: String,
    namespace: String,
    subject: String,
    display_label: Option<String>,
    status: String,
    claim_metadata: serde_json::Value,
    provenance: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<IdentityDb> for ParticipantIdentity {
    type Error = AppError;

    fn try_from(row: IdentityDb) -> AppResult<Self> {
        let claim_metadata: IdentityClaimMetadata = serde_json::from_value(row.claim_metadata)
            .map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.claim_metadata for {}: {error}",
                    row.id
                ))
            })?;
        if claim_metadata.version() != 1 {
            return Err(AppError::Internal(format!(
                "Unsupported participant_identities.claim_metadata version for {}",
                row.id
            )));
        }

        Ok(Self {
            id: ParticipantIdentityId::new(row.id),
            company_id: row.company_id,
            principal_id: PrincipalId::new(row.principal_id),
            transport: TransportKind::from_str(&row.transport).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.transport for {}: {error}",
                    row.id
                ))
            })?,
            namespace: IdentityNamespace::parse(row.namespace).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.namespace for {}: {error}",
                    row.id
                ))
            })?,
            subject: IdentitySubject::parse(row.subject).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.subject for {}: {error}",
                    row.id
                ))
            })?,
            display_label: row.display_label,
            status: IdentityStatus::from_str(&row.status).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.status for {}: {error}",
                    row.id
                ))
            })?,
            claim_metadata,
            provenance: IdentityProvenance::from_str(&row.provenance).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid participant_identities.provenance for {}: {error}",
                    row.id
                ))
            })?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

const PRINCIPAL_SELECT: &str = r#"
    SELECT id, company_id, kind, user_id, agent_id, display_label, created_at, updated_at
    FROM principals
"#;

const IDENTITY_SELECT: &str = r#"
    SELECT id, company_id, principal_id, transport, namespace, subject, display_label, status,
           claim_metadata, provenance, created_at, updated_at
    FROM participant_identities
"#;

fn checked_label(label: Option<String>, fallback: &str) -> AppResult<String> {
    let label = label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string());
    if label.len() > 255 {
        return Err(AppError::BadRequest(
            "Identity display label exceeds 255 bytes.".into(),
        ));
    }
    Ok(label)
}

async fn load_resolution(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    identity: &QualifiedIdentity,
) -> AppResult<Option<IdentityResolution>> {
    let identity_query = format!(
        "{IDENTITY_SELECT} WHERE company_id = $1 AND transport = $2 AND namespace = $3 AND subject = $4"
    );
    let identity_row = sqlx::query_as::<_, IdentityDb>(&identity_query)
        .bind(company_id)
        .bind(identity.transport().as_str())
        .bind(identity.namespace().as_str())
        .bind(identity.subject().as_str())
        .fetch_optional(&mut *connection)
        .await
        .map_err(AppError::from)?;
    let Some(identity_row) = identity_row else {
        return Ok(None);
    };
    let principal_query = format!("{PRINCIPAL_SELECT} WHERE company_id = $1 AND id = $2");
    let principal_row = sqlx::query_as::<_, PrincipalDb>(&principal_query)
        .bind(company_id)
        .bind(identity_row.principal_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(AppError::from)?;
    Ok(Some(IdentityResolution {
        principal: principal_row.try_into()?,
        identity: identity_row.try_into()?,
    }))
}

pub(crate) async fn resolve_or_create_external_identity_on(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    observation: IdentityObservation,
) -> AppResult<IdentityResolution> {
    sqlx::query(
        r#"SELECT pg_advisory_xact_lock(hashtextextended(
               $1::text || ':' || $2 || ':' || $3 || ':' || $4, 0))"#,
    )
    .bind(company_id)
    .bind(observation.identity.transport().as_str())
    .bind(observation.identity.namespace().as_str())
    .bind(observation.identity.subject().as_str())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;

    if let Some(existing) = load_resolution(connection, company_id, &observation.identity).await? {
        return Ok(existing);
    }

    let principal_id = PrincipalId::random();
    let identity_id = ParticipantIdentityId::random();
    let label = checked_label(
        observation.display_label.clone(),
        observation.identity.subject().as_str(),
    )?;
    let claims = serde_json::to_value(&observation.claim_metadata)
        .map_err(|error| AppError::Internal(error.to_string()))?;

    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, display_label)
           VALUES ($1, $2, 'external', $3)"#,
    )
    .bind(principal_id.as_uuid())
    .bind(company_id)
    .bind(&label)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"INSERT INTO participant_identities
               (id, company_id, principal_id, transport, namespace, subject, display_label,
                status, claim_metadata, provenance)
           VALUES ($1, $2, $3, $4, $5, $6, $7, 'observed', $8, $9)"#,
    )
    .bind(identity_id.as_uuid())
    .bind(company_id)
    .bind(principal_id.as_uuid())
    .bind(observation.identity.transport().as_str())
    .bind(observation.identity.namespace().as_str())
    .bind(observation.identity.subject().as_str())
    .bind(observation.display_label)
    .bind(claims)
    .bind(observation.provenance.as_str())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;

    load_resolution(connection, company_id, &observation.identity)
        .await?
        .ok_or_else(|| AppError::Internal("Created identity was not found.".into()))
}

/// Give an account its company principal, with its mailbox as a verified email identity.
///
/// If that mailbox was already seen as an outsider, the existing external principal is *promoted*
/// rather than duplicated, so the threads it is already a party to stay attached to the person who
/// just joined. The promotion is keyed on the account's own registered address and nothing else --
/// never a display name, and never an email a provider merely claimed on somebody's behalf.
pub(crate) async fn create_person_principal_on(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    user_id: Uuid,
    display_label: &str,
    email: &str,
) -> AppResult<PrincipalId> {
    let qualified = EmailIdentity::parse(EmailAddress::from(email))
        .map(EmailIdentity::qualify_default)
        .map_err(|error| AppError::BadRequest(format!("Invalid account email: {error}")))?;
    sqlx::query(
        r#"SELECT pg_advisory_xact_lock(hashtextextended(
               $1::text || ':' || $2 || ':' || $3 || ':' || $4, 0))"#,
    )
    .bind(company_id)
    .bind(qualified.transport().as_str())
    .bind(qualified.namespace().as_str())
    .bind(qualified.subject().as_str())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;

    let existing: Option<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT principal.id, principal.kind, principal.user_id
           FROM participant_identities AS identity
           JOIN principals AS principal
             ON (principal.company_id, principal.id) =
                (identity.company_id, identity.principal_id)
           WHERE identity.company_id = $1 AND identity.transport = 'email'
             AND identity.namespace = $2 AND identity.subject = $3
           FOR UPDATE OF principal, identity"#,
    )
    .bind(company_id)
    .bind(qualified.namespace().as_str())
    .bind(qualified.subject().as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;

    let principal_id = match existing {
        Some((id, kind, _)) if kind == "external" => {
            sqlx::query(
                r#"UPDATE principals
                   SET kind = 'person', user_id = $3, display_label = $4,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE company_id = $1 AND id = $2"#,
            )
            .bind(company_id)
            .bind(id)
            .bind(user_id)
            .bind(display_label)
            .execute(&mut *connection)
            .await
            .map_err(AppError::from)?;
            PrincipalId::new(id)
        }
        Some((id, kind, Some(existing_user_id)))
            if kind == "person" && existing_user_id == user_id =>
        {
            PrincipalId::new(id)
        }
        Some((_, kind, existing_user_id)) => {
            return Err(AppError::Conflict(format!(
                "Email identity is already attached to a {kind} principal ({existing_user_id:?})."
            )));
        }
        None => {
            let id = PrincipalId::random();
            sqlx::query(
                r#"INSERT INTO principals
                       (id, company_id, kind, user_id, display_label)
                   VALUES ($1, $2, 'person', $3, $4)"#,
            )
            .bind(id.as_uuid())
            .bind(company_id)
            .bind(user_id)
            .bind(display_label)
            .execute(&mut *connection)
            .await
            .map_err(AppError::from)?;
            id
        }
    };

    let claims = serde_json::to_value(IdentityClaimMetadata::account())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        r#"INSERT INTO participant_identities
               (id, company_id, principal_id, transport, namespace, subject, display_label,
                status, claim_metadata, provenance)
           VALUES ($1, $2, $3, 'email', $4, $5, $6, 'verified', $7, 'account')
           ON CONFLICT (company_id, transport, namespace, subject) DO UPDATE SET
               principal_id = EXCLUDED.principal_id,
               display_label = EXCLUDED.display_label,
               status = 'verified', claim_metadata = EXCLUDED.claim_metadata,
               provenance = 'account', updated_at = CURRENT_TIMESTAMP"#,
    )
    .bind(ParticipantIdentityId::random().as_uuid())
    .bind(company_id)
    .bind(principal_id.as_uuid())
    .bind(qualified.namespace().as_str())
    .bind(qualified.subject().as_str())
    .bind(display_label)
    .bind(claims)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(principal_id)
}

/// The company's one system principal: the platform itself, as an author.
///
/// A schedule that runs for nobody in particular and an approval note are said by the platform,
/// not by a mailbox, so they are attributed here rather than to a synthesized channel address.
/// Idempotent by `principals_company_system_key`, so every such message across the company's whole
/// history is attributed to the same actor.
pub(crate) async fn ensure_system_principal_on(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
) -> AppResult<PrincipalId> {
    let id = PrincipalId::random();
    let stored: Uuid = sqlx::query_scalar(
        r#"INSERT INTO principals (id, company_id, kind, display_label)
           VALUES ($1, $2, 'system', 'System')
           ON CONFLICT (company_id) WHERE kind = 'system'
           DO UPDATE SET updated_at = CURRENT_TIMESTAMP
           RETURNING id"#,
    )
    .bind(id.as_uuid())
    .bind(company_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(PrincipalId::new(stored))
}

pub(crate) async fn create_agent_principal_on(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    agent_id: Uuid,
    display_label: &str,
) -> AppResult<PrincipalId> {
    let id = PrincipalId::random();
    let stored: Uuid = sqlx::query_scalar(
        r#"INSERT INTO principals (id, company_id, kind, agent_id, display_label)
           VALUES ($1, $2, 'agent', $3, $4)
           ON CONFLICT (company_id, agent_id) WHERE agent_id IS NOT NULL
           DO UPDATE SET display_label = EXCLUDED.display_label,
                         updated_at = CURRENT_TIMESTAMP
           RETURNING id"#,
    )
    .bind(id.as_uuid())
    .bind(company_id)
    .bind(agent_id)
    .bind(display_label)
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(PrincipalId::new(stored))
}

#[async_trait]
impl IdentityDirectory for PostgresPersistence {
    async fn resolve_or_create_external_identity(
        &self,
        company_id: Uuid,
        observation: IdentityObservation,
    ) -> AppResult<IdentityResolution> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        // Serialize only contenders for this tenant-qualified identity. This prevents the usual
        // insert-principal/lose-identity-upsert race from leaking an orphan external principal.
        let created =
            resolve_or_create_external_identity_on(&mut transaction, company_id, observation)
                .await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(created)
    }

    async fn identities_for_principals(
        &self,
        company_id: Uuid,
        principal_ids: &[PrincipalId],
        transport: TransportKind,
    ) -> AppResult<Vec<ParticipantIdentity>> {
        if principal_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = principal_ids.iter().map(|id| id.as_uuid()).collect();
        let query = format!(
            "{IDENTITY_SELECT} WHERE company_id = $1 AND principal_id = ANY($2) AND transport = $3 AND status <> 'disabled' ORDER BY created_at, id"
        );
        sqlx::query_as::<_, IdentityDb>(&query)
            .bind(company_id)
            .bind(ids)
            .bind(transport.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }
}

fn membership_from_db(value: Option<&str>) -> AppResult<CompanyMembership> {
    match value {
        Some("owner") => Ok(CompanyMembership::Owner),
        Some("admin") => Ok(CompanyMembership::Admin),
        Some("member") => Ok(CompanyMembership::Member),
        None => Ok(CompanyMembership::None),
        Some(other) => Err(AppError::Internal(format!(
            "Invalid company membership role '{other}'"
        ))),
    }
}

#[async_trait]
impl PrincipalAccessPersistence for PostgresPersistence {
    async fn access_context_for_identity(
        &self,
        company_id: Uuid,
        identity: &QualifiedIdentity,
    ) -> AppResult<PrincipalAccessContext> {
        let row: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
            r#"SELECT principal.id, identity.status,
                      CASE
                          WHEN company.user_id = principal.user_id THEN 'owner'
                          ELSE member.role
                      END AS membership
               FROM participant_identities AS identity
               JOIN principals AS principal
                 ON (principal.company_id, principal.id) =
                    (identity.company_id, identity.principal_id)
               JOIN companies AS company ON company.id = principal.company_id
               LEFT JOIN company_members AS member
                 ON (member.company_id, member.user_id) =
                    (principal.company_id, principal.user_id)
               WHERE identity.company_id = $1 AND identity.transport = $2
                 AND identity.namespace = $3 AND identity.subject = $4"#,
        )
        .bind(company_id)
        .bind(identity.transport().as_str())
        .bind(identity.namespace().as_str())
        .bind(identity.subject().as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        let Some((principal_id, status, membership)) = row else {
            return Ok(PrincipalAccessContext {
                principal_id: None,
                membership: CompanyMembership::None,
            });
        };
        if status == "disabled" {
            return Ok(PrincipalAccessContext {
                principal_id: None,
                membership: CompanyMembership::None,
            });
        }
        Ok(PrincipalAccessContext {
            principal_id: Some(PrincipalId::new(principal_id)),
            membership: membership_from_db(membership.as_deref())?,
        })
    }

    async fn access_context_for_user(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<PrincipalAccessContext>> {
        let row: Option<(Uuid, String)> = sqlx::query_as(
            r#"SELECT principal.id,
                      CASE WHEN company.user_id = principal.user_id
                           THEN 'owner' ELSE member.role END AS membership
               FROM principals AS principal
               JOIN companies AS company ON company.id = principal.company_id
               JOIN company_members AS member
                 ON (member.company_id, member.user_id) =
                    (principal.company_id, principal.user_id)
               WHERE principal.company_id = $1 AND principal.user_id = $2"#,
        )
        .bind(company_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|(principal_id, membership)| {
            Ok(PrincipalAccessContext {
                principal_id: Some(PrincipalId::new(principal_id)),
                membership: membership_from_db(Some(&membership))?,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::test_support::test_pool;
    use crate::entities::company::CompanyAccess;
    use crate::use_cases::company::{CompanyPersistence, CompanyWrite};
    use crate::use_cases::participant::IdentityObservation;
    use crate::use_cases::user::UserPersistence;
    use std::sync::Arc;

    /// One company with a fresh owner, named uniquely because the whole suite shares a database.
    async fn company(persistence: &PostgresPersistence, label: &str) -> (Uuid, Uuid) {
        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("{label}_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = persistence.get_by_email(&email).await.unwrap().unwrap();
        let company = CompanyPersistence::create(
            persistence,
            owner.id,
            CompanyWrite {
                name: format!("{label} Corp"),
                slug: format!("{label}-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        (company.id, owner.id)
    }

    fn email_identity(address: &str) -> QualifiedIdentity {
        EmailIdentity::parse(EmailAddress::from(address))
            .unwrap()
            .qualify_default()
    }

    fn observation(identity: QualifiedIdentity) -> IdentityObservation {
        IdentityObservation {
            identity,
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: IdentityProvenance::EmailIngress,
        }
    }

    /// Every table in the model is keyed by `(company_id, ...)` and references its parent the same
    /// way, so a row that names another tenant's principal, channel or thread cannot be written at
    /// all -- the check is a foreign key, not application code that could be bypassed.
    #[tokio::test]
    async fn the_database_rejects_every_cross_tenant_participant_row() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let (company_a, owner_a) = company(&persistence, "tenanta").await;
        let (company_b, _) = company(&persistence, "tenantb").await;

        let outsider = persistence
            .resolve_or_create_external_identity(
                company_a,
                observation(email_identity("outsider@partner.test")),
            )
            .await
            .unwrap();

        // A person principal in company B cannot name company A's owner: the composite key points
        // at `company_members`, where that pair does not exist.
        let cross_tenant_person = sqlx::query(
            r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
               VALUES ($1, $2, 'person', $3, 'Impostor')"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_b)
        .bind(owner_a)
        .execute(&pool)
        .await;
        assert!(cross_tenant_person.is_err());

        // An identity in company B cannot attach itself to company A's principal.
        let cross_tenant_identity = sqlx::query(
            r#"INSERT INTO participant_identities
                   (id, company_id, principal_id, transport, namespace, subject, status,
                    claim_metadata, provenance)
               VALUES ($1, $2, $3, 'email', 'email', 'stolen@partner.test', 'observed',
                       '{"kind":"observation","version":1}'::jsonb, 'email_ingress')"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_b)
        .bind(outsider.principal.id.as_uuid())
        .execute(&pool)
        .await;
        assert!(cross_tenant_identity.is_err());

        // Neither can a channel grant nor a thread participant reach across the boundary.
        let cross_tenant_grant = sqlx::query(
            r#"INSERT INTO channel_principal_grants
                   (company_id, channel_id, principal_id, capability, provenance)
               VALUES ($1, $2, $3, 'participate', 'email_allowlist')"#,
        )
        .bind(company_b)
        .bind(Uuid::new_v4())
        .bind(outsider.principal.id.as_uuid())
        .execute(&pool)
        .await;
        assert!(cross_tenant_grant.is_err());

        let cross_tenant_thread_principal = sqlx::query(
            r#"INSERT INTO thread_principals
                   (company_id, channel_id, thread_id, principal_id, role)
               VALUES ($1, $2, $3, $4, 'participant')"#,
        )
        .bind(company_b)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(outsider.principal.id.as_uuid())
        .execute(&pool)
        .await;
        assert!(cross_tenant_thread_principal.is_err());
    }

    /// The resolver is the only writer of external identities precisely so that a burst of mail
    /// from one new correspondent cannot leave duplicate principals behind.
    #[tokio::test]
    async fn concurrent_observation_of_one_identity_creates_one_principal() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let (company_id, _) = company(&persistence, "racers").await;
        let identity = email_identity("first-time@partner.test");

        let mut racers = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let persistence = persistence.clone();
            let identity = identity.clone();
            racers.spawn(async move {
                persistence
                    .resolve_or_create_external_identity(company_id, observation(identity))
                    .await
            });
        }

        let mut resolved = Vec::new();
        while let Some(joined) = racers.join_next().await {
            resolved.push(joined.unwrap().unwrap());
        }
        assert_eq!(resolved.len(), 8);
        let first = resolved[0].principal.id;
        assert!(
            resolved
                .iter()
                .all(|resolution| resolution.principal.id == first),
            "every contender resolves to the same principal"
        );

        let principals: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM principals WHERE company_id = $1 AND kind = 'external'",
        )
        .bind(company_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(principals, 1);
        let identities: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM participant_identities WHERE company_id = $1")
                .bind(company_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        // The owner's own account identity is the other row.
        assert_eq!(identities, 2);
    }

    /// A subject is only unique inside its namespace. Two Slack workspaces can hand out the same
    /// user id, and those are two people.
    #[tokio::test]
    async fn the_same_subject_in_two_namespaces_does_not_collide() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company_id, _) = company(&persistence, "namespaces").await;

        let in_workspace = |workspace: &str| {
            QualifiedIdentity::new(
                TransportKind::Slack,
                IdentityNamespace::parse(workspace).unwrap(),
                IdentitySubject::parse("U012ABC").unwrap(),
            )
        };
        let one = persistence
            .resolve_or_create_external_identity(company_id, observation(in_workspace("T-one")))
            .await
            .unwrap();
        let other = persistence
            .resolve_or_create_external_identity(company_id, observation(in_workspace("T-other")))
            .await
            .unwrap();

        assert_ne!(one.principal.id, other.principal.id);
        assert_ne!(one.identity.id, other.identity.id);
    }

    /// A provider's claim about somebody's email address is recorded and then ignored: it does not
    /// find the account principal that owns that address, and it confers none of its membership.
    #[tokio::test]
    async fn a_profile_email_claim_neither_merges_a_principal_nor_grants_access() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let (company_id, owner_id) = company(&persistence, "claims").await;

        let owner_context = persistence
            .access_context_for_user(company_id, owner_id)
            .await
            .unwrap()
            .expect("company creation gives its owner a principal");
        assert_eq!(owner_context.membership, CompanyMembership::Owner);
        let owner_email: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id = $1")
            .bind(owner_id)
            .fetch_one(&persistence.pool)
            .await
            .unwrap();

        let claimant = persistence
            .resolve_or_create_external_identity(
                company_id,
                IdentityObservation {
                    identity: QualifiedIdentity::new(
                        TransportKind::Slack,
                        IdentityNamespace::parse("T-claims").unwrap(),
                        IdentitySubject::parse("U-claimant").unwrap(),
                    ),
                    display_label: Some("Definitely The Owner".into()),
                    claim_metadata: IdentityClaimMetadata::slack_profile(Some(EmailAddress::from(
                        owner_email.clone(),
                    ))),
                    provenance: IdentityProvenance::SlackProfileClaim,
                },
            )
            .await
            .unwrap();

        assert_ne!(
            claimant.principal.id,
            owner_context.principal_id.unwrap(),
            "a claimed profile email must not resolve onto the account principal"
        );
        assert_eq!(claimant.principal.kind, PrincipalKind::External);

        let claimed_context = persistence
            .access_context_for_identity(
                company_id,
                &QualifiedIdentity::new(
                    TransportKind::Slack,
                    IdentityNamespace::parse("T-claims").unwrap(),
                    IdentitySubject::parse("U-claimant").unwrap(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(claimed_context.membership, CompanyMembership::None);

        // The real mailbox still belongs to the owner, so email keeps working as one identity.
        let by_email = persistence
            .access_context_for_identity(company_id, &email_identity(&owner_email))
            .await
            .unwrap();
        assert_eq!(by_email.membership, CompanyMembership::Owner);
        assert_eq!(by_email.principal_id, owner_context.principal_id);

        // And company access itself is unchanged by the claim.
        let access: Option<CompanyAccess> = persistence
            .company_access(owner_id, company_id)
            .await
            .unwrap();
        assert!(access.is_some_and(|access| access.membership.is_owner()));
    }
}
