use super::query;
use crate::{
    adapters::{
        identity::crypto,
        management::Authorized,
        postgres::{Tx, failure as db},
        setup::postgres::SetupPostgres,
    },
    application::{
        analytics::*,
        identity::{Failure, Permission, Result},
        setup::KeyScope,
    },
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Cursor {
    filter: String,
    after: Option<String>,
}
async fn validate_objects(tx: &mut Tx, p: &Authorized, f: &Filter) -> Result<()> {
    for (table, id) in [
        ("attribution_stores", f.store_id.map(|i| i.0)),
        ("attribution_ads", f.ad_id.map(|i| i.0)),
        ("attribution_campaigns", f.campaign_id.map(|i| i.0)),
    ] {
        if let Some(id) = id {
            let scope = if table == "attribution_stores" {
                "id"
            } else {
                "store_id"
            };
            let exists:bool=sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE company_id=$1 AND id=$2 AND ($3::uuid IS NULL OR {scope}=$3))"))
                .bind(p.company().0).bind(id).bind(f.store_id.map(|i|i.0)).fetch_one(&mut **tx).await.map_err(db)?;
            if !exists {
                return Err(Failure::NotFound);
            }
        }
    }
    if let (Some(ad), Some(campaign)) = (f.ad_id, f.campaign_id) {
        let matches:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attribution_ads WHERE company_id=$1 AND id=$2 AND campaign_id=$3)")
            .bind(p.company().0).bind(ad.0).bind(campaign.0).fetch_one(&mut **tx).await.map_err(db)?;
        if !matches {
            return Err(Failure::NotFound);
        }
    }
    Ok(())
}
pub async fn report(
    store: &SetupPostgres,
    p: &Authorized,
    mut f: Filter,
    export: bool,
) -> Result<Analytics> {
    p.require(Permission::Read, KeyScope::AnalyticsRead)?;
    f.store_id = p.filter(f.store_id)?;
    if export && f.cursor.is_some() {
        return Err(Failure::Invalid);
    }
    let normalized = serde_json::to_string(&(p.company(), &f)).map_err(|_| Failure::Unavailable)?;
    let hash = URL_SAFE_NO_PAD.encode(crypto::verifier(&normalized));
    let after = match &f.cursor {
        None => None,
        Some(raw) => {
            let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| Failure::Invalid)?;
            let clear = store
                .cipher
                .open("analytics-cursor", Uuid::nil(), &bytes)
                .map_err(|_| Failure::Invalid)?;
            let cursor: Cursor = serde_json::from_slice(&clear).map_err(|_| Failure::Invalid)?;
            if cursor.filter != hash {
                return Err(Failure::Invalid);
            }
            Some(cursor.after)
        }
    };
    let mut tx = p.begin(store).await?;
    sqlx::query("SET LOCAL statement_timeout='2s'")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    validate_objects(&mut tx, p, &f).await?;
    let limit = if export {
        MAX_EXPORT_ROWS
    } else {
        usize::from(f.limit)
    };
    let sql = query::statement(f.group_by);
    let rows = sqlx::query(&sql)
        .bind(p.company().0)
        .bind(f.store_id.map(|i| i.0))
        .bind(f.ad_id.map(|i| i.0))
        .bind(f.campaign_id.map(|i| i.0))
        .bind(f.advertised_product_id.as_ref().map(|p| p.0.as_str()))
        .bind(f.sold_product_id.as_ref().map(|p| p.0.as_str()))
        .bind(f.currency.map(|c| c.as_str()))
        .bind(f.start)
        .bind(f.end)
        .bind(&f.timezone)
        .bind(after.is_some())
        .bind(after.flatten())
        .bind((limit + 1) as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;
    let mut groups = assemble(rows, &f)?;
    let next_cursor = if groups.len() > limit {
        if export {
            return Err(Failure::Invalid);
        }
        groups.pop();
        let cursor = Cursor {
            filter: hash,
            after: groups.last().ok_or(Failure::Unavailable)?.group.clone(),
        };
        let clear = serde_json::to_vec(&cursor).map_err(|_| Failure::Unavailable)?;
        let value =
            URL_SAFE_NO_PAD.encode(store.cipher.seal("analytics-cursor", Uuid::nil(), &clear)?);
        if value.len() > MAX_CURSOR {
            return Err(Failure::Unavailable);
        }
        Some(value)
    } else {
        None
    };
    if export
        && groups
            .iter()
            .map(|g| g.currencies.len().max(1))
            .sum::<usize>()
            > MAX_EXPORT_ROWS
    {
        return Err(Failure::Invalid);
    }
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    Ok(Analytics {
        start: f.start,
        end: f.end,
        timezone: f.timezone,
        groups,
        next_cursor,
    })
}
fn assemble(rows: Vec<PgRow>, filter: &Filter) -> Result<Vec<Group>> {
    let mut groups: BTreeMap<Option<String>, BTreeMap<Option<String>, Metrics>> = BTreeMap::new();
    for row in rows {
        let key: Option<String> = row.try_get("group_key").map_err(db)?;
        let group = groups.entry(key).or_default();
        if let Some(metric) = row.try_get::<Option<String>, _>("metric").map_err(db)? {
            let total: String = row.try_get("total").map_err(db)?;
            group
                .entry(row.try_get("currency").map_err(db)?)
                .or_default()
                .insert(metric, bounded(&total)?);
        }
    }
    groups
        .into_iter()
        .map(|(key, metrics)| Group::build(key, metrics, filter))
        .collect()
}
