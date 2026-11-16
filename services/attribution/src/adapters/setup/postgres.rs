use super::dns::DomainDns;
use crate::adapters::management::{Actor, Receipt};
pub use crate::adapters::postgres::Tx;
pub(super) use crate::adapters::postgres::failure as db;
use crate::{
    adapters::identity::{
        crypto::{self, Cipher},
        tenants::lock_tenant,
    },
    application::{freshness::Ticket, identity::*, setup::*},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Clone)]
pub struct SetupPostgres {
    pub pool: PgPool,
    pub cipher: std::sync::Arc<Cipher>,
}
pub struct MutationResult {
    pub created: bool,
    pub body: Value,
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    company: CompanyId,
    resource: Resource,
    store: Option<StoreId>,
    after: Uuid,
}

const DOMAIN_FIELDS: &str = "id, store_id, host, CASE WHEN revoked_at IS NOT NULL THEN 'revoked' WHEN verified_until IS NULL THEN 'pending' WHEN verified_until <= clock_timestamp() THEN 'expired' ELSE 'verified' END AS status, verified_until";
const CREDENTIAL_FIELDS: &str =
    "id, store_id, label, scopes, expires_at, created_at, revoked_at, last_used_at";
fn resource_sql(resource: Resource) -> (&'static str, &'static str) {
    match resource {
        Resource::Stores => (
            "attribution_stores",
            "id, company_id, name, timezone, enabled",
        ),
        Resource::Domains => ("attribution_domains", DOMAIN_FIELDS),
        Resource::Installations => (
            "attribution_installations",
            "id, store_id, domain_id, name, enabled",
        ),
        Resource::Credentials => ("attribution_credentials", CREDENTIAL_FIELDS),
    }
}
pub(super) async fn now(tx: &mut Tx) -> Result<DateTime<Utc>> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
        .map_err(db)
}
pub async fn enabled_store(tx: &mut Tx, company: CompanyId, store: StoreId) -> Result<()> {
    let enabled: bool =
        sqlx::query_scalar("SELECT enabled FROM attribution_stores WHERE company_id=$1 AND id=$2")
            .bind(company.0)
            .bind(store.0)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db)?
            .ok_or(Failure::NotFound)?;
    if !enabled {
        return Err(Failure::Forbidden);
    }
    Ok(())
}
pub(super) async fn value(
    tx: &mut Tx,
    company: CompanyId,
    resource: Resource,
    id: Uuid,
) -> Result<Value> {
    let (table, fields) = resource_sql(resource);
    sqlx::query_scalar(&format!(
        "SELECT to_jsonb(r) FROM (SELECT {fields} FROM {table} WHERE company_id=$1 AND id=$2) r"
    ))
    .bind(company.0)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?
    .ok_or(Failure::NotFound)
}
impl SetupPostgres {
    pub async fn user_transaction(&self, p: &TenantPrincipal) -> Result<Tx> {
        let mut tx = self.pool.begin().await.map_err(db)?;
        lock_tenant(&mut tx, p.access.company_id, &p.ticket).await?;
        // Acquire before testing time: a waiting writer must not use a session
        // whose expiry/revocation completed while it waited.
        let row=sqlx::query("SELECT user_id, revoked_at, absolute_expires_at, idle_expires_at FROM attribution_sessions WHERE id=$1 FOR UPDATE")
            .bind(p.principal.session.id.0).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::Unauthorized)?;
        let time = now(&mut tx).await?;
        if row.try_get::<Uuid, _>("user_id").map_err(db)? != p.principal.identity.user_id.0
            || row
                .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
                .map_err(db)?
                .is_some()
            || row
                .try_get::<DateTime<Utc>, _>("absolute_expires_at")
                .map_err(db)?
                <= time
            || row
                .try_get::<DateTime<Utc>, _>("idle_expires_at")
                .map_err(db)?
                <= time
        {
            return Err(Failure::Unauthorized);
        }
        Ok(tx)
    }
    pub async fn read(&self, p: &TenantPrincipal, r: Resource, id: Uuid) -> Result<Value> {
        let mut tx = self.user_transaction(p).await?;
        let result = value(&mut tx, p.access.company_id, r, id).await?;
        tx.commit().await.map_err(db)?;
        Ok(result)
    }
    pub async fn list(&self, p: &TenantPrincipal, r: Resource, q: ListQuery) -> Result<Value> {
        let limit = q.limit()?;
        let after = match &q.cursor {
            Some(encoded) => {
                let bytes = URL_SAFE_NO_PAD
                    .decode(encoded)
                    .map_err(|_| Failure::Invalid)?;
                let bytes = self
                    .cipher
                    .open("setup-cursor", Uuid::nil(), &bytes)
                    .map_err(|_| Failure::Invalid)?;
                let cursor: Cursor =
                    serde_json::from_slice(&bytes).map_err(|_| Failure::Invalid)?;
                if cursor.company != p.access.company_id
                    || cursor.resource != r
                    || cursor.store != q.store_id
                {
                    return Err(Failure::Invalid);
                }
                cursor.after
            }
            None => Uuid::nil(),
        };
        let mut tx = self.user_transaction(p).await?;
        if let Some(store) = q.store_id {
            value(&mut tx, p.access.company_id, Resource::Stores, store.0).await?;
        }
        let (table, fields) = resource_sql(r);
        let filter = if r == Resource::Stores {
            "id"
        } else {
            "store_id"
        };
        let mut items:Vec<Value>=sqlx::query_scalar(&format!("SELECT to_jsonb(r) FROM (SELECT {fields} FROM {table} WHERE company_id=$1 AND id>$2 AND ($3::uuid IS NULL OR {filter}=$3) ORDER BY id LIMIT $4) r"))
            .bind(p.access.company_id.0).bind(after).bind(q.store_id.map(|id|id.0)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db)?;
        let next = if items.len() > usize::from(limit) {
            items.pop();
            let after =
                serde_json::from_value(items.last().ok_or(Failure::Unavailable)?["id"].clone())
                    .map_err(|_| Failure::Unavailable)?;
            let cursor = Cursor {
                company: p.access.company_id,
                resource: r,
                store: q.store_id,
                after,
            };
            Some(URL_SAFE_NO_PAD.encode(self.cipher.seal(
                "setup-cursor",
                Uuid::nil(),
                &serde_json::to_vec(&cursor).map_err(|_| Failure::Unavailable)?,
            )?))
        } else {
            None
        };
        tx.commit().await.map_err(db)?;
        Ok(json!({"items":items,"next_cursor":next}))
    }
    pub async fn mutate(
        &self,
        p: &TenantPrincipal,
        m: Mutation,
        retry: Retry,
        dns: &DomainDns,
    ) -> Result<MutationResult> {
        if !p
            .access
            .permissions
            .contains(&Permission::ManageIntegrations)
        {
            return Err(Failure::Forbidden);
        }
        let m = m.normalize()?;
        let receipt = Receipt::new(Actor::user(p), m.operation(), retry, &m)?;
        let mut tx = self.user_transaction(p).await?;
        if let Some(body) = receipt.replay(&mut tx).await? {
            tx.commit().await.map_err(db)?;
            return Ok(MutationResult {
                created: false,
                body,
            });
        }
        if let Mutation::VerifyDomain(id) = &m {
            let proof = domain_proof(&mut tx, p.access.company_id, *id).await?;
            tx.rollback().await.map_err(db)?;
            dns.verify(&proof.0, &proof.1).await?;
            tx = self.user_transaction(p).await?;
            if let Some(body) = receipt.replay(&mut tx).await? {
                tx.commit().await.map_err(db)?;
                return Ok(MutationResult {
                    created: false,
                    body,
                });
            }
            if domain_proof(&mut tx, p.access.company_id, *id).await? != proof {
                return Err(Failure::Conflict);
            }
        }
        let (resource, created, body) = apply(&mut tx, p.access.company_id, &m).await?;
        let metadata = if body.get("secret").is_some() {
            json!({"credential":body["credential"],"secret_available":false,"recovery":"revoke_then_reissue_with_new_idempotency_key"})
        } else {
            body.clone()
        };
        receipt
            .record(&mut tx, metadata, m.action(), resource)
            .await?;
        tx.commit().await.map_err(db)?;
        Ok(MutationResult { created, body })
    }
}
async fn domain_proof(tx: &mut Tx, company: CompanyId, id: DomainId) -> Result<(String, String)> {
    let row=sqlx::query("SELECT store_id, host, challenge, revoked_at FROM attribution_domains WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
    if row
        .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
        .map_err(db)?
        .is_some()
    {
        return Err(Failure::Gone);
    }
    enabled_store(tx, company, StoreId(row.try_get("store_id").map_err(db)?)).await?;
    Ok((
        row.try_get("host").map_err(db)?,
        row.try_get("challenge").map_err(db)?,
    ))
}
async fn apply(tx: &mut Tx, company: CompanyId, m: &Mutation) -> Result<(Uuid, bool, Value)> {
    let generated = Uuid::new_v4();
    let (id, created, r) = match m {
        Mutation::CreateStore(w) => {
            sqlx::query(
                "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,$3,$4)",
            )
            .bind(generated)
            .bind(company.0)
            .bind(&w.name)
            .bind(&w.timezone)
            .execute(&mut **tx)
            .await
            .map_err(db)?;
            (generated, true, Resource::Stores)
        }
        Mutation::UpdateStore(id, w) => {
            value(tx, company, Resource::Stores, id.0).await?;
            sqlx::query("UPDATE attribution_stores SET name=COALESCE($3,name), timezone=COALESCE($4,timezone), enabled=COALESCE($5,enabled) WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).bind(&w.name).bind(&w.timezone).bind(w.enabled).execute(&mut **tx).await.map_err(db)?;
            (id.0, false, Resource::Stores)
        }
        Mutation::CreateDomain(w) => {
            enabled_store(tx, company, w.store_id).await?;
            let challenge = crypto::random();
            sqlx::query("INSERT INTO attribution_domains(id,company_id,store_id,host,challenge) VALUES($1,$2,$3,$4,$5)").bind(generated).bind(company.0).bind(w.store_id.0).bind(&w.host).bind(&challenge).execute(&mut **tx).await.map_err(db)?;
            let domain = value(tx, company, Resource::Domains, generated).await?;
            return Ok((
                generated,
                true,
                json!({"domain":domain,"record_name":format!("_attribution.{}",w.host),"record_value":challenge}),
            ));
        }
        Mutation::VerifyDomain(id) => {
            sqlx::query("UPDATE attribution_domains SET verified_until=clock_timestamp()+interval '30 days' WHERE company_id=$1 AND id=$2 AND revoked_at IS NULL").bind(company.0).bind(id.0).execute(&mut **tx).await.map_err(db)?;
            (id.0, false, Resource::Domains)
        }
        Mutation::RevokeDomain(id) => {
            value(tx, company, Resource::Domains, id.0).await?;
            sqlx::query("UPDATE attribution_domains SET revoked_at=COALESCE(revoked_at,clock_timestamp()) WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).execute(&mut **tx).await.map_err(db)?;
            (id.0, false, Resource::Domains)
        }
        Mutation::CreateInstallation(w) => {
            enabled_store(tx, company, w.store_id).await?;
            approved_domain(tx, company, w.store_id, w.domain_id).await?;
            sqlx::query("INSERT INTO attribution_installations(id,company_id,store_id,domain_id,name) VALUES($1,$2,$3,$4,$5)").bind(generated).bind(company.0).bind(w.store_id.0).bind(w.domain_id.0).bind(&w.name).execute(&mut **tx).await.map_err(db)?;
            (generated, true, Resource::Installations)
        }
        Mutation::UpdateInstallation(id, w) => {
            let installation = value(tx, company, Resource::Installations, id.0).await?;
            if w.enabled {
                let store: StoreId = serde_json::from_value(installation["store_id"].clone())
                    .map_err(|_| Failure::Unavailable)?;
                let domain: DomainId = serde_json::from_value(installation["domain_id"].clone())
                    .map_err(|_| Failure::Unavailable)?;
                enabled_store(tx, company, store).await?;
                approved_domain(tx, company, store, domain).await?;
            }
            sqlx::query(
                "UPDATE attribution_installations SET enabled=$3 WHERE company_id=$1 AND id=$2",
            )
            .bind(company.0)
            .bind(id.0)
            .bind(w.enabled)
            .execute(&mut **tx)
            .await
            .map_err(db)?;
            (id.0, false, Resource::Installations)
        }
        Mutation::CreateCredential(w) => {
            enabled_store(tx, company, w.store_id).await?;
            let time = now(tx).await?;
            if w.expires_at.is_some_and(|expiry| expiry <= time) {
                return Err(Failure::Invalid);
            }
            let scopes: Vec<String> = w.scopes.iter().map(|s| s.as_str().to_owned()).collect();
            return issue(tx, company, w.store_id, &w.label, &scopes, w.expires_at).await;
        }
        Mutation::RevokeCredential(id) => {
            value(tx, company, Resource::Credentials, id.0).await?;
            sqlx::query("UPDATE attribution_credentials SET revoked_at=COALESCE(revoked_at,clock_timestamp()) WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).execute(&mut **tx).await.map_err(db)?;
            (id.0, false, Resource::Credentials)
        }
        Mutation::RotateCredential(id, w) => {
            let row=sqlx::query("SELECT store_id,label,scopes,expires_at,revoked_at,rotated_to FROM attribution_credentials WHERE company_id=$1 AND id=$2 FOR UPDATE").bind(company.0).bind(id.0).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
            let time = now(tx).await?;
            let expiry: Option<DateTime<Utc>> = row.try_get("expires_at").map_err(db)?;
            if row
                .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
                .map_err(db)?
                .is_some()
                || row
                    .try_get::<Option<Uuid>, _>("rotated_to")
                    .map_err(db)?
                    .is_some()
                || expiry.is_some_and(|e| e <= time)
            {
                return Err(Failure::Conflict);
            }
            let store = StoreId(row.try_get("store_id").map_err(db)?);
            enabled_store(tx, company, store).await?;
            let issued = issue(
                tx,
                company,
                store,
                &row.try_get::<String, _>("label").map_err(db)?,
                &row.try_get::<Vec<String>, _>("scopes").map_err(db)?,
                expiry,
            )
            .await?;
            let end = time + chrono::Duration::seconds(i64::from(w.overlap_seconds));
            sqlx::query("UPDATE attribution_credentials SET expires_at=$3, rotated_to=$4 WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).bind(expiry.map_or(end,|old|old.min(end))).bind(issued.0).execute(&mut **tx).await.map_err(db)?;
            return Ok(issued);
        }
    };
    Ok((id, created, value(tx, company, r, id).await?))
}
async fn issue(
    tx: &mut Tx,
    company: CompanyId,
    store: StoreId,
    label: &str,
    scopes: &[String],
    expiry: Option<DateTime<Utc>>,
) -> Result<(Uuid, bool, Value)> {
    let id = Uuid::new_v4();
    let secret = format!("ak_{}", crypto::random());
    sqlx::query("INSERT INTO attribution_credentials(id,company_id,store_id,label,verifier,scopes,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(company.0).bind(store.0).bind(label).bind(crypto::verifier(&secret)).bind(scopes).bind(expiry).execute(&mut **tx).await.map_err(db)?;
    Ok((
        id,
        true,
        json!({"credential":value(tx,company,Resource::Credentials,id).await?,"secret":secret}),
    ))
}
pub async fn approved_domain(
    tx: &mut Tx,
    company: CompanyId,
    store: StoreId,
    id: DomainId,
) -> Result<String> {
    let row=sqlx::query("SELECT host, revoked_at, verified_until FROM attribution_domains WHERE company_id=$1 AND store_id=$2 AND id=$3").bind(company.0).bind(store.0).bind(id.0).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
    let time = now(tx).await?;
    if row
        .try_get::<Option<DateTime<Utc>>, _>("revoked_at")
        .map_err(db)?
        .is_some()
        || row
            .try_get::<Option<DateTime<Utc>>, _>("verified_until")
            .map_err(db)?
            .is_none_or(|expiry| expiry <= time)
    {
        return Err(Failure::Gone);
    }
    row.try_get("host").map_err(db)
}
/// Caller holds the shared tenant transaction for the complete link/click effect.
pub async fn approved_destination(
    tx: &mut Tx,
    company: CompanyId,
    store: StoreId,
    input: &str,
) -> Result<url::Url> {
    enabled_store(tx, company, store).await?;
    let url = destination(input)?;
    let id:Uuid=sqlx::query_scalar("SELECT id FROM attribution_domains WHERE company_id=$1 AND store_id=$2 AND host=$3 AND revoked_at IS NULL")
        .bind(company.0).bind(store.0).bind(url.host_str()).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
    approved_domain(tx, company, store, DomainId(id)).await?;
    Ok(url)
}
pub struct PublicInstallation {
    pub company: CompanyId,
    pub store: StoreId,
    pub host: String,
    pub allowed_observations: [&'static str; 3],
}
impl SetupPostgres {
    /// The preliminary lookup supplies scope only. This locked check is required
    /// before an observation write; the public ID never authenticates a merchant.
    pub async fn installation_scope(&self, id: InstallationId) -> Result<CompanyId> {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT company_id FROM attribution_installations WHERE id=$1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(db)?
        .map(CompanyId)
        .ok_or(Failure::NotFound)
    }
    pub async fn public_installation(
        &self,
        tx: &mut Tx,
        id: InstallationId,
        company: CompanyId,
        ticket: &Ticket,
    ) -> Result<PublicInstallation> {
        lock_tenant(tx, company, ticket).await?;
        let row=sqlx::query("SELECT store_id,domain_id,enabled FROM attribution_installations WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
        if !row.try_get::<bool, _>("enabled").map_err(db)? {
            return Err(Failure::Gone);
        }
        let store = StoreId(row.try_get("store_id").map_err(db)?);
        enabled_store(tx, company, store).await?;
        let host = approved_domain(
            tx,
            company,
            store,
            DomainId(row.try_get("domain_id").map_err(db)?),
        )
        .await?;
        Ok(PublicInstallation {
            company,
            store,
            host,
            allowed_observations: ["image_load", "landing_page_load", "visible_placement"],
        })
    }
}
