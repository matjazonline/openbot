//! Authenticates headers before body consumption, then rechecks under the same
//! transaction lock as revocation and lifecycle changes at the actual write.
use super::{
    admission::{Admission, Permit},
    postgres::{SetupPostgres, Tx, db, enabled_store, now},
};
use crate::{
    adapters::identity::{crypto, tenants::lock_tenant},
    application::{freshness::Ticket, identity::*, setup::*},
};
use axum::http::{HeaderMap, header};
use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

pub struct Merchant {
    pub company: CompanyId,
    pub store: StoreId,
    pub credential: CredentialId,
    required: KeyScope,
    ticket: Ticket,
    verifier: Vec<u8>,
    _permit: Permit,
}
impl Merchant {
    pub fn require(&self, scope: KeyScope) -> Result<()> {
        if self.required == scope {
            Ok(())
        } else {
            Err(Failure::Forbidden)
        }
    }
    /// Scope from the key is authoritative; a submitted ID never changes it.
    pub fn store(&self, submitted: StoreId) -> Result<()> {
        if submitted == self.store {
            Ok(())
        } else {
            Err(Failure::NotFound)
        }
    }
}
impl SetupPostgres {
    pub async fn merchant(
        &self,
        identity: &IdentityService,
        admission: &Admission,
        headers: &HeaderMap,
        scope: KeyScope,
    ) -> Result<Merchant> {
        if headers.contains_key(header::COOKIE)
            || headers.get_all(header::AUTHORIZATION).iter().count() != 1
        {
            return Err(Failure::Unauthorized);
        }
        let secret = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|s| {
                s.len() == 46
                    && s.starts_with("ak_")
                    && s[3..]
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
            .ok_or(Failure::Unauthorized)?;
        let verifier = crypto::verifier(secret);
        let row=sqlx::query("SELECT id,company_id,store_id,scopes FROM attribution_credentials WHERE verifier=$1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at>clock_timestamp())")
            .bind(&verifier).fetch_optional(&self.pool).await.map_err(db)?.ok_or(Failure::Unauthorized)?;
        if !row
            .try_get::<Vec<String>, _>("scopes")
            .map_err(db)?
            .iter()
            .any(|s| s == scope.as_str())
        {
            return Err(Failure::Forbidden);
        }
        let company = CompanyId(row.try_get("company_id").map_err(db)?);
        if headers.get_all("x-company-id").iter().count() > 1 {
            return Err(Failure::Invalid);
        }
        if let Some(value) = headers.get("x-company-id") {
            let supplied = value
                .to_str()
                .ok()
                .and_then(|v| Uuid::parse_str(v).ok())
                .ok_or(Failure::Invalid)?;
            if supplied != company.0 {
                return Err(Failure::NotFound);
            }
        }
        let permit = admission.enter(company)?;
        let ticket = identity.tenant(company).await?;
        let principal = Merchant {
            company,
            store: StoreId(row.try_get("store_id").map_err(db)?),
            credential: CredentialId(row.try_get("id").map_err(db)?),
            required: scope,
            ticket,
            verifier,
            _permit: permit,
        };
        // This pre-body check is not the write authorization: begin_merchant must
        // be used again for the body-dependent mutation, covering revoke races.
        let tx = self.begin_merchant(&principal).await?;
        tx.rollback().await.map_err(db)?;
        Ok(principal)
    }
    pub async fn begin_merchant(&self, p: &Merchant) -> Result<Tx> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        lock_tenant(&mut tx, p.company, &p.ticket).await?;
        let row=sqlx::query("SELECT scopes,expires_at,revoked_at FROM attribution_credentials WHERE company_id=$1 AND store_id=$2 AND id=$3 AND verifier=$4 FOR UPDATE")
            .bind(p.company.0).bind(p.store.0).bind(p.credential.0).bind(&p.verifier).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::Unauthorized)?;
        let time = now(&mut tx).await?;
        if row
            .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
            .map_err(db)?
            .is_some()
            || row
                .try_get::<Option<DateTime<Utc>>, _>("expires_at")
                .map_err(db)?
                .is_some_and(|expiry| expiry <= time)
        {
            return Err(Failure::Unauthorized);
        }
        if !row
            .try_get::<Vec<String>, _>("scopes")
            .map_err(db)?
            .iter()
            .any(|s| s == p.required.as_str())
        {
            return Err(Failure::Forbidden);
        }
        enabled_store(&mut tx, p.company, p.store).await?;
        sqlx::query("UPDATE attribution_credentials SET last_used_at=$4 WHERE company_id=$1 AND store_id=$2 AND id=$3").bind(p.company.0).bind(p.store.0).bind(p.credential.0).bind(time).execute(&mut *tx).await.map_err(db)?;
        Ok(tx)
    }
}
