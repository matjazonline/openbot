use super::PostgresPersistence;
use crate::application::{
    attribution::*,
    session::{SessionId, VerifiedSession},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Row, Transaction};
use subtle::ConstantTimeEq;
use uuid::Uuid;
mod outbox;
fn database(_: sqlx::Error) -> IdentityError {
    IdentityError::Unavailable
}
struct LiveSession {
    revision: i64,
    expires_at: DateTime<Utc>,
}
async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    actor: VerifiedSession,
) -> IdentityResult<LiveSession> {
    let revision: i64 =
        sqlx::query_scalar("SELECT auth_revision FROM users WHERE id = $1 FOR UPDATE")
            .bind(actor.user_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(database)?
            .ok_or(IdentityError::Unauthorized)?;
    let row = sqlx::query("SELECT auth_revision, expires_at, revoked_at FROM user_sessions WHERE id = $1 AND user_id = $2 FOR UPDATE")
        .bind(actor.session_id.0).bind(actor.user_id).fetch_optional(&mut **tx).await.map_err(database)?.ok_or(IdentityError::Unauthorized)?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
        .map_err(database)?;
    let expires_at = row.try_get("expires_at").map_err(database)?;
    if row.try_get::<i64, _>("auth_revision").map_err(database)? != revision
        || row
            .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
            .map_err(database)?
            .is_some()
        || expires_at <= now
    {
        return Err(IdentityError::Unauthorized);
    }
    Ok(LiveSession {
        revision,
        expires_at,
    })
}
const LIVE_GRANT: &str = "grant_row.verifier = $1 AND grant_row.client_id = 'attribution' AND grant_row.revoked_at IS NULL AND grant_row.expires_at > clock_timestamp() AND session.revoked_at IS NULL AND session.expires_at > clock_timestamp() AND account.auth_revision = grant_row.auth_revision";
const GRANT_JOIN: &str = "attribution_identity_grants AS grant_row JOIN user_sessions AS session ON (session.id, session.user_id, session.auth_revision) = (grant_row.session_id, grant_row.user_id, grant_row.auth_revision) JOIN users AS account ON account.id = grant_row.user_id";
const COMPANY_COLUMNS: &str = "company.id AS company_id, left(company.name,120) AS name, member.role, lifecycle.version, lifecycle.state, lifecycle.changed_at";
const COMPANY_JOIN: &str = "JOIN principals AS principal ON principal.user_id = grant_row.user_id \
    JOIN companies AS company ON company.id = principal.company_id \
    JOIN LATERAL membership_of(grant_row.user_id, company.id) AS member ON true \
    JOIN attribution_company_lifecycles AS lifecycle ON lifecycle.company_id = company.id";
fn status(row: &sqlx::postgres::PgRow) -> IdentityResult<CompanyStatus> {
    Ok(CompanyStatus {
        company_id: row.try_get("company_id").map_err(database)?,
        version: row.try_get("version").map_err(database)?,
        state: CompanyState::parse(&row.try_get::<String, _>("state").map_err(database)?)?,
        changed_at: row.try_get("changed_at").map_err(database)?,
    })
}
fn access(row: &sqlx::postgres::PgRow) -> IdentityResult<CompanyAccess> {
    let role: String = row.try_get("role").map_err(database)?;
    let permissions = match role.as_str() {
        "owner" | "admin" => vec!["read", "links:manage", "integrations:manage"],
        "member" => vec!["read"],
        _ => return Err(IdentityError::Unavailable),
    };
    Ok(CompanyAccess {
        company_id: row.try_get("company_id").map_err(database)?,
        name: row.try_get("name").map_err(database)?,
        permissions,
        lifecycle: status(row)?,
    })
}
#[async_trait]
impl AttributionIdentity for PostgresPersistence {
    async fn begin_continuation(
        &self,
        verifier: &SecretHash,
        binding: &AuthorizationBinding,
    ) -> IdentityResult<()> {
        sqlx::query("INSERT INTO attribution_login_continuations (verifier, state, challenge, callback) VALUES ($1, $2, $3, $4)")
            .bind(&verifier.0).bind(&binding.state.0).bind(&binding.challenge.0).bind(&binding.callback.0).execute(&self.pool).await.map_err(database)?;
        Ok(())
    }
    async fn issue_code(&self, issue: IssueCode) -> IdentityResult<AuthorizationBinding> {
        let mut tx = self.pool.begin().await.map_err(database)?;
        let live = lock_session(&mut tx, issue.actor).await?;
        let binding = match issue.source {
            AuthorizationSource::Direct(binding) => binding,
            AuthorizationSource::Continuation(verifier) => {
                let row = sqlx::query("DELETE FROM attribution_login_continuations WHERE verifier = $1 AND expires_at > clock_timestamp() RETURNING state, challenge, callback")
                    .bind(&verifier.0).fetch_optional(&mut *tx).await.map_err(database)?.ok_or(IdentityError::Unauthorized)?;
                AuthorizationBinding {
                    state: RequestState(row.try_get("state").map_err(database)?),
                    challenge: Challenge(row.try_get("challenge").map_err(database)?),
                    callback: Callback(row.try_get("callback").map_err(database)?),
                }
            }
        };
        sqlx::query("INSERT INTO attribution_identity_codes (verifier, user_id, session_id, auth_revision, client_id, callback, challenge, expires_at) VALUES ($1, $2, $3, $4, 'attribution', $5, $6, LEAST($7, clock_timestamp() + interval '60 seconds'))")
            .bind(&issue.code.0).bind(issue.actor.user_id).bind(issue.actor.session_id.0).bind(live.revision).bind(&binding.callback.0).bind(&binding.challenge.0).bind(live.expires_at)
            .execute(&mut *tx).await.map_err(database)?;
        tx.commit().await.map_err(database)?;
        Ok(binding)
    }
    async fn redeem(&self, redemption: RedeemCode) -> IdentityResult<DateTime<Utc>> {
        let candidate = sqlx::query(
            "SELECT user_id, session_id FROM attribution_identity_codes WHERE verifier = $1",
        )
        .bind(&redemption.code.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(database)?
        .ok_or(IdentityError::Unauthorized)?;
        let actor = VerifiedSession {
            user_id: candidate.try_get("user_id").map_err(database)?,
            session_id: SessionId(candidate.try_get("session_id").map_err(database)?),
        };
        let mut tx = self.pool.begin().await.map_err(database)?;
        let live = lock_session(&mut tx, actor).await?;
        let row = sqlx::query("SELECT callback, challenge, auth_revision FROM attribution_identity_codes WHERE verifier = $1 FOR UPDATE")
            .bind(&redemption.code.0).fetch_optional(&mut *tx).await.map_err(database)?.ok_or(IdentityError::Unauthorized)?;
        let callback: String = row.try_get("callback").map_err(database)?;
        let challenge: String = row.try_get("challenge").map_err(database)?;
        if callback != redemption.callback.0
            || !bool::from(
                challenge
                    .as_bytes()
                    .ct_eq(redemption.challenge.0.as_bytes()),
            )
            || row.try_get::<i64, _>("auth_revision").map_err(database)? != live.revision
        {
            return Err(IdentityError::Unauthorized);
        }
        let consumed = sqlx::query("UPDATE attribution_identity_codes SET consumed_at = clock_timestamp() WHERE verifier = $1 AND client_id = 'attribution' AND consumed_at IS NULL AND expires_at > clock_timestamp() AND $2 > clock_timestamp()")
            .bind(&redemption.code.0).bind(live.expires_at).execute(&mut *tx).await.map_err(database)?.rows_affected();
        if consumed != 1 {
            return Err(IdentityError::Unauthorized);
        }
        let expiry: DateTime<Utc> = sqlx::query_scalar("INSERT INTO attribution_identity_grants (verifier, user_id, session_id, auth_revision, client_id, expires_at) VALUES ($1, $2, $3, $4, 'attribution', LEAST($5, clock_timestamp() + interval '8 hours')) RETURNING expires_at")
            .bind(&redemption.grant.0).bind(actor.user_id).bind(actor.session_id.0).bind(live.revision).bind(live.expires_at).fetch_one(&mut *tx).await.map_err(database)?;
        tx.commit().await.map_err(database)?;
        Ok(expiry)
    }
    async fn identity(&self, grant: &SecretHash) -> IdentityResult<Identity> {
        let row = sqlx::query(&format!("SELECT account.id, left(account.username::text,120) AS display_name, LEAST(grant_row.expires_at, session.expires_at) AS expires_at FROM {GRANT_JOIN} WHERE {LIVE_GRANT}"))
            .bind(&grant.0).fetch_optional(&self.pool).await.map_err(database)?.ok_or(IdentityError::Unauthorized)?;
        Ok(Identity {
            user_id: row.try_get("id").map_err(database)?,
            display_name: row.try_get("display_name").map_err(database)?,
            expires_at: row.try_get("expires_at").map_err(database)?,
        })
    }
    async fn companies(&self, page: CompanyPageRequest) -> IdentityResult<CompanyList> {
        if !(1..=100).contains(&page.limit) {
            return Err(IdentityError::Invalid);
        }
        self.identity(&page.grant).await?;
        let rows = sqlx::query(&format!("SELECT {COMPANY_COLUMNS} FROM {GRANT_JOIN} {COMPANY_JOIN} WHERE {LIVE_GRANT} AND ($2::uuid IS NULL OR company.id > $2) ORDER BY company.id LIMIT $3"))
            .bind(&page.grant.0).bind(page.after).bind(i64::from(page.limit)+1).fetch_all(&self.pool).await.map_err(database)?;
        let more = rows.len() > usize::from(page.limit);
        let items = rows
            .iter()
            .take(usize::from(page.limit))
            .map(access)
            .collect::<IdentityResult<_>>()?;
        Ok(CompanyList { items, more })
    }
    async fn access(&self, grant: &SecretHash, company: Uuid) -> IdentityResult<CompanyAccess> {
        self.identity(grant).await?;
        let row = sqlx::query(&format!("SELECT {COMPANY_COLUMNS} FROM {GRANT_JOIN} {COMPANY_JOIN} WHERE {LIVE_GRANT} AND company.id = $2"))
            .bind(&grant.0).bind(company).fetch_optional(&self.pool).await.map_err(database)?.ok_or(IdentityError::NotFound)?;
        access(&row)
    }
    async fn status(&self, company: Uuid) -> IdentityResult<CompanyStatus> {
        let row = sqlx::query("SELECT company_id, version, state, changed_at FROM attribution_company_lifecycles WHERE company_id = $1")
            .bind(company).fetch_optional(&self.pool).await.map_err(database)?.ok_or(IdentityError::NotFound)?;
        status(&row)
    }
    async fn set_status(&self, change: StatusChange) -> IdentityResult<CompanyStatus> {
        if change.state == CompanyState::Deleted || change.expected_version < 1 {
            return Err(IdentityError::Invalid);
        }
        let mut tx = self.pool.begin().await.map_err(database)?;
        lock_session(&mut tx, change.actor).await?;
        let company: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
                .bind(change.company_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database)?;
        if company.is_none() {
            return Err(IdentityError::NotFound);
        }
        let role: String = sqlx::query_scalar(
            "SELECT membership.role FROM department_members AS member \
             CROSS JOIN LATERAL membership_of($2, $1) AS membership \
             WHERE member.company_id = $1 AND member.root_user_id = $2 \
             FOR UPDATE OF member",
        )
        .bind(change.company_id)
        .bind(change.actor.user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database)?
        .ok_or(IdentityError::NotFound)?;
        if role != "owner" && role != "admin" {
            return Err(IdentityError::Forbidden);
        }
        let row = sqlx::query("UPDATE attribution_company_lifecycles SET state = $3 WHERE company_id = $1 AND version = $2 RETURNING company_id, version, state, changed_at")
            .bind(change.company_id).bind(change.expected_version).bind(change.state.as_str()).fetch_optional(&mut *tx).await.map_err(database)?.ok_or(IdentityError::Conflict)?;
        let result = status(&row)?;
        tx.commit().await.map_err(database)?;
        Ok(result)
    }
    async fn revoke(&self, grant: &SecretHash) -> IdentityResult<()> {
        sqlx::query("UPDATE attribution_identity_grants SET revoked_at = clock_timestamp() WHERE verifier = $1 AND revoked_at IS NULL")
            .bind(&grant.0).execute(&self.pool).await.map_err(database)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
