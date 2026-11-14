//! An account's address change, as every company it belongs to sees it.
//!
//! A person principal is reached through its verified account identity -- the mailbox the account
//! registered -- so an address change that left that identity alone would keep trusting the old
//! mailbox as the person and treat the new one as a stranger. The re-key here moves the trust in the
//! same transaction as the address change itself.

use uuid::Uuid;

use crate::{
    adapters::persistence::participant::{
        account_email_identity, lock_identity_key, upsert_account_identity_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        participant::IdentityClaimMetadata,
        transport::{PrincipalId, QualifiedIdentity},
    },
};

/// One company in which the account has a person principal.
#[derive(sqlx::FromRow)]
struct PersonPrincipal {
    company_id: Uuid,
    id: Uuid,
}

/// Who currently holds an email identity in one company.
#[derive(sqlx::FromRow)]
struct IdentityHolder {
    identity_id: Uuid,
    principal_id: Uuid,
    kind: String,
}

/// Move an account's trust from `old_email` to `new_email` in every company where it has a person
/// principal.
///
/// Call this inside the transaction that changes `users.email`, after the `UPDATE`: that row lock is
/// what company creation and invite acceptance take before they register the account's address, so
/// either they committed first (and their principal is re-keyed here) or they run afterwards and
/// read the new address.
///
/// In each company:
///
/// - the old address is detached to a new outsider principal, observed exactly as an unknown
///   correspondent would be, and the messages it sent go with it. It stops granting anything, and a
///   later account that registers it adopts it like any other outsider.
/// - the new address becomes the person's verified account identity. If it was already an
///   outsider's, that outsider's messages, channel grants and thread places become the person's --
///   what adopting the outsider would have kept with the address.
pub(crate) async fn rekey_account_identity_on(
    connection: &mut sqlx::PgConnection,
    user_id: Uuid,
    old_email: &str,
    new_email: &str,
) -> AppResult<()> {
    let companies = super::workflow::prelock_account_companies_on(connection, user_id).await?;
    let persons: Vec<PersonPrincipal> = sqlx::query_as(
        r#"SELECT company_id, id FROM principals
           WHERE user_id = $1 AND kind = 'person'
           ORDER BY company_id"#,
    )
    .bind(user_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(AppError::from)?;
    if persons.is_empty() {
        return Ok(());
    }
    // Registration accepts any plausible address, so the old one may be one no identity was ever
    // written for; there is then nothing of it to detach. The new one becomes an identity, so it
    // must parse, exactly as it must when a company is created.
    let old = account_email_identity(old_email).ok();
    let new = account_email_identity(new_email)?;
    if old.as_ref() == Some(&new) {
        // A confirmed same-address change can install previously missing proof.
        return super::workflow::refresh_account_dependents_on(connection, &companies).await;
    }
    let username: String = sqlx::query_scalar("SELECT username::text FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(AppError::from)?;

    // Both identity locks are taken in one order everywhere, so two re-keys that touch the same
    // pair of addresses cannot wait on each other crosswise.
    let mut keys: Vec<&QualifiedIdentity> = old.iter().chain([&new]).collect();
    keys.sort_by(|left, right| left.subject().as_str().cmp(right.subject().as_str()));

    for person in persons {
        for key in &keys {
            lock_identity_key(connection, person.company_id, key).await?;
        }
        let principal = PrincipalId::new(person.id);
        if let Some(old) = &old {
            detach_old_address(connection, person.company_id, principal, old).await?;
        }
        adopt_new_address(connection, person.company_id, principal, &new, &username).await?;
    }
    super::workflow::refresh_account_dependents_on(connection, &companies).await
}

async fn identity_holder(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    identity: &QualifiedIdentity,
) -> AppResult<Option<IdentityHolder>> {
    sqlx::query_as(
        r#"SELECT identity.id AS identity_id, principal.id AS principal_id, principal.kind
           FROM participant_identities AS identity
           JOIN principals AS principal
             ON (principal.company_id, principal.id) =
                (identity.company_id, identity.principal_id)
           WHERE identity.company_id = $1 AND identity.transport = $2
             AND identity.namespace = $3 AND identity.subject = $4
           FOR UPDATE OF principal, identity"#,
    )
    .bind(company_id)
    .bind(identity.transport().as_str())
    .bind(identity.namespace().as_str())
    .bind(identity.subject().as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)
}

/// Hand the old address to a new outsider principal, with the messages it sent.
///
/// The identity and its messages move in one statement: `messages_authored_identity_author_fk`
/// requires a message's author to hold its authored identity, and it is checked at the end of the
/// statement, so two separate updates would each violate it.
async fn detach_old_address(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    person: PrincipalId,
    old: &QualifiedIdentity,
) -> AppResult<()> {
    let Some(holder) = identity_holder(connection, company_id, old).await? else {
        return Ok(());
    };
    if holder.principal_id != person.as_uuid() {
        // Already somebody else's (an earlier change, or an outsider): nothing of ours to detach.
        return Ok(());
    }
    let outsider = PrincipalId::random();
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, display_label)
           VALUES ($1, $2, 'external', $3)"#,
    )
    .bind(outsider.as_uuid())
    .bind(company_id)
    .bind(old.subject().as_str())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    let observation = serde_json::to_value(IdentityClaimMetadata::observation())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        r#"WITH detached AS (
               UPDATE participant_identities AS identity
               SET principal_id = $3, status = 'observed', provenance = 'transport_ingress',
                   claim_metadata = $4, display_label = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE identity.company_id = $1 AND identity.id = $2
               RETURNING identity.id
           )
           UPDATE messages AS message
           SET author_principal_id = $3
           FROM detached
           WHERE message.company_id = $1 AND message.authored_identity_id = detached.id"#,
    )
    .bind(company_id)
    .bind(holder.identity_id)
    .bind(outsider.as_uuid())
    .bind(observation)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Make the new address the person's account identity, adopting what an outsider held with it.
async fn adopt_new_address(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    person: PrincipalId,
    new: &QualifiedIdentity,
    display_label: &str,
) -> AppResult<()> {
    let holder = identity_holder(connection, company_id, new).await?;
    let adopted_outsider = match &holder {
        None => None,
        Some(holder) if holder.principal_id == person.as_uuid() => None,
        Some(holder) if holder.kind == "external" => Some(holder.principal_id),
        Some(holder) => {
            return Err(AppError::Conflict(format!(
                "Email identity is already attached to a {} principal.",
                holder.kind
            )));
        }
    };
    upsert_account_identity_on(connection, company_id, person, new, display_label).await?;
    if let Some(outsider) = adopted_outsider {
        adopt_outsider_places(connection, company_id, person, outsider).await?;
    }
    Ok(())
}

/// Give the person the channel grants and thread places an outsider held for the address it just
/// took, when that address was all the outsider was.
///
/// An outsider that still has another identity keeps its rows, because they serve that identity
/// too. Otherwise the outsider is left with no identity, grant or thread place -- unreachable, but
/// not deleted, since other history (a workflow destination, a notification actor) may name it.
async fn adopt_outsider_places(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    person: PrincipalId,
    outsider: Uuid,
) -> AppResult<()> {
    let still_reachable: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM participant_identities
               WHERE company_id = $1 AND principal_id = $2
           )"#,
    )
    .bind(company_id)
    .bind(outsider)
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    if still_reachable {
        return Ok(());
    }
    // A grant the person already holds for the same channel, capability and provenance keeps the
    // person's row.
    sqlx::query(
        r#"INSERT INTO channel_principal_grants
               (company_id, channel_id, principal_id, capability, provenance, created_at)
           SELECT company_id, channel_id, $3, capability, provenance, created_at
           FROM channel_principal_grants
           WHERE company_id = $1 AND principal_id = $2
           ON CONFLICT (company_id, channel_id, principal_id, capability, provenance) DO NOTHING"#,
    )
    .bind(company_id)
    .bind(outsider)
    .bind(person.as_uuid())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    sqlx::query("DELETE FROM channel_principal_grants WHERE company_id = $1 AND principal_id = $2")
        .bind(company_id)
        .bind(outsider)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    sqlx::query(
        r#"INSERT INTO thread_principals
               (company_id, channel_id, thread_id, principal_id, role, created_at)
           SELECT company_id, channel_id, thread_id, $3, role, created_at
           FROM thread_principals
           WHERE company_id = $1 AND principal_id = $2
           ON CONFLICT (company_id, channel_id, thread_id, principal_id, role) DO NOTHING"#,
    )
    .bind(company_id)
    .bind(outsider)
    .bind(person.as_uuid())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    sqlx::query("DELETE FROM thread_principals WHERE company_id = $1 AND principal_id = $2")
        .bind(company_id)
        .bind(outsider)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

#[cfg(test)]
#[path = "account_identity_tests.rs"]
mod tests;
