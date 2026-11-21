//! Receipt and audit persistence shared by all management mutations. Callers own
//! current authorization and the tenant transaction; this module never commits it.
use super::{
    identity::crypto,
    postgres::{Tx, failure as db},
    setup::{merchant::Merchant, postgres::SetupPostgres},
};
use crate::application::{
    identity::{CompanyId, Failure, Permission, Result, TenantPrincipal},
    setup::{KeyScope, Retry, StoreId},
};
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

pub enum Authorized {
    User(TenantPrincipal),
    Key(Merchant),
}
impl Authorized {
    pub fn require(&self, permission: Permission, scope: KeyScope) -> Result<()> {
        match self {
            Self::User(p) => {
                if p.access.permissions.contains(&permission) {
                    Ok(())
                } else {
                    Err(Failure::Forbidden)
                }
            }
            Self::Key(p) => p.require(scope),
        }
    }

    pub fn company(&self) -> CompanyId {
        match self {
            Self::User(p) => p.access.company_id,
            Self::Key(p) => p.company,
        }
    }
    pub fn filter(&self, supplied: Option<StoreId>) -> Result<Option<StoreId>> {
        match self {
            Self::User(_) => Ok(supplied),
            Self::Key(p) => {
                if let Some(id) = supplied {
                    p.store(id)?;
                }
                Ok(Some(p.store))
            }
        }
    }
    pub fn actor(&self) -> Actor {
        match self {
            Self::User(p) => Actor::user(p),
            Self::Key(p) => Actor {
                company: p.company,
                kind: ActorKind::Merchant,
                id: p.credential.0,
            },
        }
    }
    pub async fn begin(&self, store: &SetupPostgres) -> Result<Tx> {
        match self {
            Self::User(p) => store.user_transaction(p).await,
            Self::Key(p) => store.begin_merchant(p).await,
        }
    }
}

#[derive(Clone, Copy)]
pub enum ActorKind {
    User,
    Merchant,
}
impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Merchant => "merchant",
        }
    }
}
#[derive(Clone, Copy)]
pub struct Actor {
    pub company: CompanyId,
    pub kind: ActorKind,
    pub id: Uuid,
}
impl Actor {
    pub fn user(p: &TenantPrincipal) -> Self {
        Self {
            company: p.access.company_id,
            kind: ActorKind::User,
            id: p.principal.identity.user_id.0,
        }
    }
}
pub struct Receipt {
    pub actor: Actor,
    pub operation: String,
    pub retry: Retry,
    fingerprint: Vec<u8>,
}
impl Receipt {
    pub fn new(
        actor: Actor,
        operation: String,
        retry: Retry,
        payload: &impl Serialize,
    ) -> Result<Self> {
        let normalized = serde_json::to_string(payload).map_err(|_| Failure::Unavailable)?;
        Ok(Self {
            actor,
            operation,
            retry,
            fingerprint: crypto::verifier(&normalized),
        })
    }
    pub async fn replay(&self, tx: &mut Tx) -> Result<Option<Value>> {
        let time = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut **tx)
            .await
            .map_err(db)?;
        self.retry.validate(time)?;
        let row=sqlx::query("SELECT original_time,fingerprint,result FROM attribution_management_receipts WHERE company_id=$1 AND principal_kind=$2 AND principal_id=$3 AND operation=$4 AND key=$5")
            .bind(self.actor.company.0).bind(self.actor.kind.as_str()).bind(self.actor.id).bind(&self.operation).bind(self.retry.key).fetch_optional(&mut **tx).await.map_err(db)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.try_get::<String, _>("original_time").map_err(db)?
            != self.retry.created_at.to_rfc3339()
            || row.try_get::<Vec<u8>, _>("fingerprint").map_err(db)? != self.fingerprint
        {
            return Err(Failure::Conflict);
        }
        Ok(Some(row.try_get("result").map_err(db)?))
    }
    /// Save only the replayable metadata. In particular, callers must remove a
    /// newly issued bearer secret before recording its one-time issuance result.
    pub async fn record(
        &self,
        tx: &mut Tx,
        result: Value,
        action: &str,
        resource: Uuid,
    ) -> Result<()> {
        sqlx::query("INSERT INTO attribution_management_receipts(company_id,principal_kind,principal_id,operation,key,original_time,fingerprint,result) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(self.actor.company.0).bind(self.actor.kind.as_str()).bind(self.actor.id).bind(&self.operation).bind(self.retry.key).bind(self.retry.created_at.to_rfc3339()).bind(&self.fingerprint).bind(result).execute(&mut **tx).await.map_err(db)?;
        sqlx::query("INSERT INTO attribution_audit(id,company_id,actor_kind,actor_id,action,resource_id) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(Uuid::new_v4()).bind(self.actor.company.0).bind(self.actor.kind.as_str()).bind(self.actor.id).bind(action).bind(resource).execute(&mut **tx).await.map_err(db)?;
        Ok(())
    }
}
