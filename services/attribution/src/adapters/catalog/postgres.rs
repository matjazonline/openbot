use crate::{
    adapters::{
        identity::tenants::lock_tenant,
        management::{Authorized, Receipt},
        postgres::{Tx, failure as db},
        setup::postgres::{SetupPostgres, approved_destination, enabled_store},
    },
    application::{
        catalog::*,
        freshness::Ticket,
        identity::*,
        setup::{ListQuery, Retry, StoreId},
    },
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

impl Authorized {
    fn authorize(&self, resource: Resource, write: bool) -> Result<()> {
        if matches!(self, Self::Key(_)) && resource != Resource::Links {
            return Err(Failure::Forbidden);
        }
        self.require(
            if write {
                Permission::ManageLinks
            } else {
                Permission::Read
            },
            if write {
                crate::application::setup::KeyScope::LinksWrite
            } else {
                crate::application::setup::KeyScope::LinksRead
            },
        )
    }
}
fn table(resource: Resource) -> (&'static str, &'static str) {
    match resource {
        Resource::Campaigns => ("attribution_campaigns", "id,store_id,name,enabled"),
        Resource::Ads => (
            "attribution_ads",
            "id,store_id,campaign_id,name,advertised_product_id,enabled",
        ),
        Resource::Links => (
            "attribution_links",
            "id,store_id,ad_id,code,destination,placement,revision,enabled",
        ),
    }
}
async fn value(
    tx: &mut Tx,
    company: CompanyId,
    filter: Option<StoreId>,
    resource: Resource,
    id: Uuid,
) -> Result<Value> {
    let (table, fields) = table(resource);
    sqlx::query_scalar(&format!("SELECT to_jsonb(r) FROM (SELECT {fields} FROM {table} WHERE company_id=$1 AND id=$2 AND ($3::uuid IS NULL OR store_id=$3)) r"))
        .bind(company.0).bind(id).bind(filter.map(|s|s.0)).fetch_optional(&mut **tx).await.map_err(db)?.ok_or(Failure::NotFound)
}
pub async fn read(
    store: &SetupPostgres,
    p: &Authorized,
    resource: Resource,
    id: Uuid,
) -> Result<Value> {
    p.authorize(resource, false)?;
    let mut tx = p.begin(store).await?;
    let result = value(&mut tx, p.company(), p.filter(None)?, resource, id).await?;
    tx.commit().await.map_err(db)?;
    Ok(result)
}
#[derive(Serialize, Deserialize)]
struct Cursor {
    company: CompanyId,
    resource: Resource,
    store: Option<StoreId>,
    after: Uuid,
}
pub async fn list(
    store: &SetupPostgres,
    p: &Authorized,
    resource: Resource,
    q: ListQuery,
) -> Result<Value> {
    p.authorize(resource, false)?;
    let limit = q.limit()?;
    let filter = p.filter(q.store_id)?;
    let after = if let Some(cursor) = q.cursor {
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| Failure::Invalid)?;
        let bytes = store
            .cipher
            .open("catalog-cursor", Uuid::nil(), &bytes)
            .map_err(|_| Failure::Invalid)?;
        let cursor: Cursor = serde_json::from_slice(&bytes).map_err(|_| Failure::Invalid)?;
        if cursor.company != p.company() || cursor.resource != resource || cursor.store != filter {
            return Err(Failure::Invalid);
        }
        cursor.after
    } else {
        Uuid::nil()
    };
    let mut tx = p.begin(store).await?;
    if let Some(filter) = filter {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM attribution_stores WHERE company_id=$1 AND id=$2)",
        )
        .bind(p.company().0)
        .bind(filter.0)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !exists {
            return Err(Failure::NotFound);
        }
    }
    let (table, fields) = table(resource);
    let mut items:Vec<Value>=sqlx::query_scalar(&format!("SELECT to_jsonb(r) FROM (SELECT {fields} FROM {table} WHERE company_id=$1 AND id>$2 AND ($3::uuid IS NULL OR store_id=$3) ORDER BY id LIMIT $4) r"))
        .bind(p.company().0).bind(after).bind(filter.map(|s|s.0)).bind(i64::from(limit)+1).fetch_all(&mut *tx).await.map_err(db)?;
    let next = if items.len() > usize::from(limit) {
        items.pop();
        let after = serde_json::from_value(items.last().ok_or(Failure::Unavailable)?["id"].clone())
            .map_err(|_| Failure::Unavailable)?;
        let bytes = serde_json::to_vec(&Cursor {
            company: p.company(),
            resource,
            store: filter,
            after,
        })
        .map_err(|_| Failure::Unavailable)?;
        Some(URL_SAFE_NO_PAD.encode(store.cipher.seal("catalog-cursor", Uuid::nil(), &bytes)?))
    } else {
        None
    };
    tx.commit().await.map_err(db)?;
    Ok(json!({"items":items,"next_cursor":next}))
}
pub async fn mutate(
    store: &SetupPostgres,
    p: &Authorized,
    mutation: Mutation,
    retry: Retry,
) -> Result<(bool, Value)> {
    let resource = match mutation {
        Mutation::CreateCampaign(_) | Mutation::UpdateCampaign(..) => Resource::Campaigns,
        Mutation::CreateAd(_) | Mutation::UpdateAd(..) => Resource::Ads,
        _ => Resource::Links,
    };
    p.authorize(resource, true)?;
    p.filter(mutation.supplied_store())?;
    let mutation = mutation.normalize()?;
    let receipt = Receipt::new(p.actor(), mutation.operation(), retry, &mutation)?;
    let mut tx = p.begin(store).await?;
    if let Some(result) = receipt.replay(&mut tx).await? {
        tx.commit().await.map_err(db)?;
        return Ok((false, result));
    }
    let (id, created) = apply(&mut tx, p, &mutation).await?;
    let result = value(&mut tx, p.company(), p.filter(None)?, resource, id).await?;
    receipt
        .record(&mut tx, result.clone(), mutation.action(), id)
        .await?;
    tx.commit().await.map_err(db)?;
    Ok((created, result))
}
async fn parent(
    tx: &mut Tx,
    company: CompanyId,
    store: StoreId,
    resource: Resource,
    id: Uuid,
) -> Result<Value> {
    let parent = value(tx, company, Some(store), resource, id).await?;
    if parent["enabled"] != true {
        return Err(Failure::Gone);
    }
    Ok(parent)
}
async fn apply(tx: &mut Tx, p: &Authorized, m: &Mutation) -> Result<(Uuid, bool)> {
    let id = Uuid::new_v4();
    let company = p.company();
    let filter = p.filter(None)?;
    match m {
        Mutation::CreateCampaign(w) => {
            enabled_store(tx, company, w.store_id).await?;
            sqlx::query("INSERT INTO attribution_campaigns(id,company_id,store_id,name) VALUES($1,$2,$3,$4)").bind(id).bind(company.0).bind(w.store_id.0).bind(&w.name).execute(&mut **tx).await.map_err(db)?;
            Ok((id, true))
        }
        Mutation::UpdateCampaign(id, w) => {
            value(tx, company, filter, Resource::Campaigns, id.0).await?;
            sqlx::query("UPDATE attribution_campaigns SET name=COALESCE($3,name),enabled=COALESCE($4,enabled) WHERE company_id=$1 AND id=$2").bind(company.0).bind(id.0).bind(&w.name).bind(w.enabled).execute(&mut **tx).await.map_err(db)?;
            Ok((id.0, false))
        }
        Mutation::CreateAd(w) => {
            enabled_store(tx, company, w.store_id).await?;
            parent(
                tx,
                company,
                w.store_id,
                Resource::Campaigns,
                w.campaign_id.0,
            )
            .await?;
            sqlx::query("INSERT INTO attribution_ads(id,company_id,store_id,campaign_id,name,advertised_product_id) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(company.0).bind(w.store_id.0).bind(w.campaign_id.0).bind(&w.name).bind(&w.advertised_product_id).execute(&mut **tx).await.map_err(db)?;
            Ok((id, true))
        }
        Mutation::UpdateAd(id, w) => {
            value(tx, company, filter, Resource::Ads, id.0).await?;
            sqlx::query("UPDATE attribution_ads SET name=COALESCE($3,name),enabled=COALESCE($4,enabled),advertised_product_id=CASE WHEN $5 THEN $6 ELSE advertised_product_id END WHERE company_id=$1 AND id=$2")
                .bind(company.0).bind(id.0).bind(&w.name).bind(w.enabled).bind(w.advertised_product_id.is_some()).bind(w.advertised_product_id.as_ref().and_then(|s|s.as_deref())).execute(&mut **tx).await.map_err(db)?;
            Ok((id.0, false))
        }
        Mutation::CreateLink(w) => {
            enabled_store(tx, company, w.store_id).await?;
            let ad = parent(tx, company, w.store_id, Resource::Ads, w.ad_id.0).await?;
            let campaign: CampaignId = serde_json::from_value(ad["campaign_id"].clone())
                .map_err(|_| Failure::Unavailable)?;
            parent(tx, company, w.store_id, Resource::Campaigns, campaign.0).await?;
            approved_destination(tx, company, w.store_id, &w.destination).await?;
            let code = oauth2::CsrfToken::new_random_len(18).secret().to_owned();
            sqlx::query("INSERT INTO attribution_links(id,company_id,store_id,ad_id,code,destination,placement) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(company.0).bind(w.store_id.0).bind(w.ad_id.0).bind(code).bind(&w.destination).bind(&w.placement).execute(&mut **tx).await.map_err(db)?;
            Ok((id, true))
        }
        Mutation::UpdateLink(id, w) => {
            let old = value(tx, company, filter, Resource::Links, id.0).await?;
            let store: StoreId = serde_json::from_value(old["store_id"].clone())
                .map_err(|_| Failure::Unavailable)?;
            // Disabling must remain possible after domain revocation. New or
            // re-enabled destinations always need a current exact approval.
            if w.destination.is_some() || w.enabled != Some(false) {
                approved_destination(
                    tx,
                    company,
                    store,
                    w.destination
                        .as_deref()
                        .unwrap_or(old["destination"].as_str().ok_or(Failure::Unavailable)?),
                )
                .await?;
            }
            let changed=sqlx::query("UPDATE attribution_links SET destination=COALESCE($3,destination),placement=COALESCE($4,placement),enabled=COALESCE($5,enabled),revision=revision+1 WHERE company_id=$1 AND id=$2 AND revision<9223372036854775807")
                .bind(company.0).bind(id.0).bind(&w.destination).bind(&w.placement).bind(w.enabled).execute(&mut **tx).await.map_err(db)?;
            if changed.rows_affected() != 1 {
                return Err(Failure::Conflict);
            }
            Ok((id.0, false))
        }
    }
}

pub struct RedirectScope {
    pub company: CompanyId,
    pub store: StoreId,
}
pub async fn redirect_scope(store: &SetupPostgres, code: &str) -> Result<RedirectScope> {
    let row = sqlx::query("SELECT company_id,store_id FROM attribution_links WHERE code=$1")
        .bind(code)
        .fetch_optional(&store.pool)
        .await
        .map_err(db)?
        .ok_or(Failure::NotFound)?;
    Ok(RedirectScope {
        company: CompanyId(row.try_get("company_id").map_err(db)?),
        store: StoreId(row.try_get("store_id").map_err(db)?),
    })
}
/// Only Some(Location) can produce302, and only after PostgreSQL confirms COMMIT.
pub async fn redirect(
    store: &SetupPostgres,
    scope: &RedirectScope,
    ticket: &Ticket,
    code: &str,
    classification: Classification,
    head: bool,
) -> Result<Option<String>> {
    let mut tx = store.pool.begin().await.map_err(db)?;
    lock_tenant(&mut tx, scope.company, ticket).await?;
    let row = sqlx::query(&format!(
        "{LINK_PROJECTION} WHERE l.company_id=$1 AND l.store_id=$2 AND l.code=$3"
    ))
    .bind(scope.company.0)
    .bind(scope.store.0)
    .bind(code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    .ok_or(Failure::NotFound)?;
    for flag in ["link_enabled", "ad_enabled", "campaign_enabled"] {
        if !row.try_get::<bool, _>(flag).map_err(db)? {
            return Err(Failure::Gone);
        }
    }
    let destination: String = row.try_get("destination").map_err(db)?;
    let mut url = approved_destination(&mut tx, scope.company, scope.store, &destination).await?;
    link_destination(url.as_str())?;
    if head {
        tx.commit().await.map_err(db)?;
        return Ok(None);
    }
    let click = ClickId(Uuid::new_v4());
    url.query_pairs_mut()
        .append_pair("ob_click", &click.0.to_string());
    let location = url.to_string();
    if !location.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return Err(Failure::Invalid);
    }
    let link: Uuid = row.try_get("id").map_err(db)?;
    let dimensions = snapshot(&row, scope.company, scope.store)?;
    sqlx::query("INSERT INTO attribution_clicks(id,company_id,store_id,link_id,dimensions,is_bot,is_prefetch,classifier_version) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(click.0).bind(scope.company.0).bind(scope.store.0).bind(link).bind(dimensions).bind(classification.is_bot).bind(classification.is_prefetch).bind(classification.version).execute(&mut *tx).await.map_err(|_| {
            tracing::warn!(operation="redirect_storage",outcome="insert_failed"); Failure::Unavailable
        })?;
    tx.commit().await.map_err(|_| {
        tracing::warn!(operation = "redirect_storage", outcome = "commit_uncertain");
        Failure::Unavailable
    })?;
    tracing::info!(operation = "redirect_storage", outcome = "committed");
    Ok(Some(location))
}

const LINK_PROJECTION: &str = "SELECT l.id,l.destination,l.placement,l.revision,l.enabled AS link_enabled,a.id AS ad_id,a.name AS ad_name,a.advertised_product_id,a.enabled AS ad_enabled,c.id AS campaign_id,c.name AS campaign_name,c.enabled AS campaign_enabled FROM attribution_links l JOIN attribution_ads a ON (a.company_id,a.store_id,a.id)=(l.company_id,l.store_id,l.ad_id) JOIN attribution_campaigns c ON (c.company_id,c.store_id,c.id)=(a.company_id,a.store_id,a.campaign_id)";

fn snapshot(row: &sqlx::postgres::PgRow, company: CompanyId, store: StoreId) -> Result<Value> {
    Ok(
        json!({"company_id":company,"store_id":store,"campaign_id":row.try_get::<Uuid,_>("campaign_id").map_err(db)?,"campaign_name":row.try_get::<String,_>("campaign_name").map_err(db)?,"ad_id":row.try_get::<Uuid,_>("ad_id").map_err(db)?,"ad_name":row.try_get::<String,_>("ad_name").map_err(db)?,"advertised_product_id":row.try_get::<Option<String>,_>("advertised_product_id").map_err(db)?,"link_id":row.try_get::<Uuid,_>("id").map_err(db)?,"link_revision":row.try_get::<i64,_>("revision").map_err(db)?,"placement":row.try_get::<String,_>("placement").map_err(db)?,"destination":row.try_get::<String,_>("destination").map_err(db)?}),
    )
}
/// Caller holds the tenant lock. Disabled link history remains observable through
/// an active installation; only new redirect clicks require enabled parents.
pub async fn link_dimensions(
    tx: &mut Tx,
    company: CompanyId,
    store: StoreId,
    id: LinkId,
) -> Result<Value> {
    let row = sqlx::query(&format!(
        "{LINK_PROJECTION} WHERE l.company_id=$1 AND l.store_id=$2 AND l.id=$3"
    ))
    .bind(company.0)
    .bind(store.0)
    .bind(id.0)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?
    .ok_or(Failure::NotFound)?;
    snapshot(&row, company, store)
}
