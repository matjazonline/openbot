//! Personal targets follow an account for their lifetime, rather than the email it currently owns.
use sqlx::PgConnection;
use uuid::Uuid;

use super::{account_email_identity, lock_identity_key};
use crate::{
    app_error::{AppError, AppResult},
    entities::transport::{PrincipalId, QualifiedIdentity},
};

#[derive(sqlx::FromRow)]
struct Holder {
    id: Uuid,
    kind: String,
    user_id: Option<Uuid>,
    historical_person_user_id: Option<Uuid>,
}

async fn holder_on(
    db: &mut PgConnection,
    company: Uuid,
    identity: &QualifiedIdentity,
) -> AppResult<Option<Holder>> {
    Ok(sqlx::query_as(
        "SELECT principal.id, principal.kind, principal.user_id, principal.historical_person_user_id \
         FROM participant_identities AS identity \
         JOIN principals AS principal ON principal.company_id = identity.company_id \
           AND principal.id = identity.principal_id \
         WHERE identity.company_id = $1 AND identity.transport = $2 \
           AND identity.namespace = $3 AND identity.subject = $4",
    )
    .bind(company)
    .bind(identity.transport().as_str())
    .bind(identity.namespace().as_str())
    .bind(identity.subject().as_str())
    .fetch_optional(db)
    .await?)
}

async fn lifetime_on(db: &mut PgConnection, company: Uuid, user: Uuid) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND historical_person_user_id = $2",
    )
    .bind(company)
    .bind(user)
    .fetch_optional(db)
    .await?)
}

/// An absent account may have changed email without visiting its retired personal principal.
/// Acquire every old/current key before any principal lock, just as account rekey does.
async fn lock_restoration_identities_on(
    db: &mut PgConnection,
    company: Uuid,
    lifetime: Option<Uuid>,
    identity: &QualifiedIdentity,
) -> AppResult<Vec<QualifiedIdentity>> {
    let subjects: Vec<String> = sqlx::query_scalar(
        "SELECT subject FROM participant_identities WHERE company_id = $1 AND principal_id = $2 \
         AND transport = $3 AND namespace = $4 AND subject <> $5 \
         AND status = 'verified' AND provenance = 'account' ORDER BY subject",
    )
    .bind(company)
    .bind(lifetime)
    .bind(identity.transport().as_str())
    .bind(identity.namespace().as_str())
    .bind(identity.subject().as_str())
    .fetch_all(&mut *db)
    .await?;
    let obsolete: Vec<QualifiedIdentity> = subjects
        .iter()
        .map(|subject| account_email_identity(subject))
        .collect::<AppResult<_>>()?;
    let mut keys: Vec<&QualifiedIdentity> = obsolete.iter().chain([identity]).collect();
    keys.sort_by(|left, right| left.subject().as_str().cmp(right.subject().as_str()));
    for key in keys {
        lock_identity_key(db, company, key).await?;
    }
    Ok(obsolete)
}

/// Caller holds the existing account/company ownership prefix; this takes all identity keys
/// before the complete sorted principal lock set.
pub(super) async fn resolve_on(
    db: &mut PgConnection,
    company: Uuid,
    user: Uuid,
    label: &str,
    identity: &QualifiedIdentity,
) -> AppResult<PrincipalId> {
    let lifetime = lifetime_on(db, company, user).await?;
    let obsolete = lock_restoration_identities_on(db, company, lifetime, identity).await?;
    let holder = holder_on(db, company, identity).await?;
    let ids: Vec<Uuid> = lifetime
        .into_iter()
        .chain(holder.as_ref().map(|row| row.id))
        .collect();
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM principals WHERE company_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE",
    )
    .bind(company)
    .bind(ids)
    .fetch_all(&mut *db)
    .await?;

    if let Some(lifetime) = lifetime {
        for old in obsolete {
            super::super::account_identity::detach_old_address(
                db,
                company,
                PrincipalId::new(lifetime),
                &old,
            )
            .await?;
        }
    }

    // Re-read after the complete sorted lock set: a concurrent removal may have demoted it.
    let holder = holder_on(db, company, identity).await?;
    if let Some(row) = &holder
        && row.kind != "external"
        && !(row.kind == "person" && row.user_id == Some(user))
    {
        return Err(AppError::Conflict(format!(
            "Email identity is already attached to a {} principal.",
            row.kind,
        )));
    }
    let reusable = holder.filter(|row| {
        row.historical_person_user_id
            .is_none_or(|owner| owner == user)
    });
    let id = lifetime.or(reusable.map(|row| row.id));
    let id = match id {
        Some(id) => {
            sqlx::query_scalar(
                "UPDATE principals SET kind = 'person', user_id = $3, display_label = $4, \
                   updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND id = $2 RETURNING id",
            )
            .bind(company)
            .bind(id)
            .bind(user)
            .bind(label)
            .fetch_one(&mut *db)
            .await?
        }
        None => {
            sqlx::query_scalar(
                "INSERT INTO principals (id, company_id, kind, user_id, display_label) \
                 VALUES ($1, $2, 'person', $3, $4) RETURNING id",
            )
            .bind(Uuid::new_v4())
            .bind(company)
            .bind(user)
            .bind(label)
            .fetch_one(&mut *db)
            .await?
        }
    };
    Ok(PrincipalId::new(id))
}
