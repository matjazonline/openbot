use crate::{
    adapters::{
        catalog::postgres::link_dimensions,
        identity::crypto,
        postgres::{Tx, failure as db},
        setup::{merchant::Merchant, postgres::SetupPostgres},
    },
    application::{
        catalog::{ClickId, LinkId},
        freshness::Ticket,
        identity::*,
        setup::KeyScope,
        website::*,
    },
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;
fn fingerprint(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(crypto::verifier(
        &serde_json::to_string(value).map_err(|_| Failure::Invalid)?,
    ))
}
async fn now(tx: &mut Tx) -> Result<DateTime<Utc>> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
        .map_err(db)
}
/// Parse an Origin, never a URL with a path/credentials or a wildcard.
pub fn origin_host(origin: &str) -> Result<String> {
    if origin.len() > 512 {
        return Err(Failure::Forbidden);
    }
    let url = url::Url::parse(origin).map_err(|_| Failure::Forbidden)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.origin().ascii_serialization() != origin
    {
        return Err(Failure::Forbidden);
    }
    url.host_str().map(str::to_owned).ok_or(Failure::Forbidden)
}
/// CORS is a browser response policy only. The write checks its exact installation
/// and tenant again under lock; an origin never authorizes a trusted merchant fact.
pub async fn cors_origin(store: &SetupPostgres, origin: &str) -> Result<()> {
    let host = origin_host(origin)?;
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attribution_domains d JOIN attribution_stores s ON (s.company_id,s.id)=(d.company_id,d.store_id) JOIN attribution_tenants t ON t.company_id=d.company_id WHERE d.host=$1 AND d.revoked_at IS NULL AND d.verified_until>clock_timestamp() AND s.enabled AND t.state='active' AND t.status_valid_until>clock_timestamp())")
        .bind(host).fetch_one(&store.pool).await.map_err(db)?;
    if allowed {
        Ok(())
    } else {
        Err(Failure::Forbidden)
    }
}
pub async fn observe(
    store: &SetupPostgres,
    company: CompanyId,
    ticket: &Ticket,
    event: &Observation,
    origin: Option<&str>,
    head: bool,
) -> Result<()> {
    event.validate()?;
    let digest = fingerprint(event)?;
    let mut tx = store.pool.begin().await.map_err(db)?;
    let installation = store
        .public_installation(&mut tx, event.installation_id, company, ticket)
        .await?;
    if let Some(origin) = origin
        && origin_host(origin)? != installation.host
    {
        return Err(Failure::Forbidden);
    }
    horizon(event.observed_at, now(&mut tx).await?)?;
    if let Some(old)=sqlx::query_scalar::<_,Vec<u8>>("SELECT fingerprint FROM attribution_observations WHERE installation_id=$1 AND event_id=$2 AND company_id=$3 AND store_id=$4")
        .bind(event.installation_id.0).bind(event.event_id.0).bind(company.0).bind(installation.store.0).fetch_optional(&mut *tx).await.map_err(db)? {
        if old!=digest {return Err(Failure::Conflict);}
        tx.commit().await.map_err(|_|Failure::Unavailable)?;
        return Ok(());
    }
    let dimensions = if let Some(click) = event.click_id {
        let row=sqlx::query("SELECT link_id,dimensions FROM attribution_clicks WHERE company_id=$1 AND store_id=$2 AND id=$3")
            .bind(company.0).bind(installation.store.0).bind(click.0).fetch_optional(&mut *tx).await.map_err(db)?.ok_or(Failure::NotFound)?;
        let link = LinkId(row.try_get("link_id").map_err(db)?);
        if event.link_id.is_some_and(|id| id != link) {
            return Err(Failure::NotFound);
        }
        row.try_get::<Value, _>("dimensions").map_err(db)?
    } else {
        link_dimensions(
            &mut tx,
            company,
            installation.store,
            event.link_id.ok_or(Failure::Invalid)?,
        )
        .await?
    };
    if !head {
        let visible = event.kind == ObservationKind::VisiblePlacement;
        sqlx::query("INSERT INTO attribution_observations(company_id,store_id,installation_id,event_id,kind,click_id,link_id,observed_at,fingerprint,dimensions,visibility_percent,visibility_millis) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
            .bind(company.0).bind(installation.store.0).bind(event.installation_id.0).bind(event.event_id.0).bind(event.kind.as_str()).bind(event.click_id.map(|id|id.0)).bind(event.link_id.map(|id|id.0)).bind(event.observed_at).bind(digest).bind(dimensions).bind(visible.then_some(50i16)).bind(visible.then_some(1000i32)).execute(&mut *tx).await.map_err(|_|Failure::Unavailable)?;
    }
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    Ok(())
}
pub async fn checkout(
    store: &SetupPostgres,
    p: &Merchant,
    input: CheckoutWrite,
) -> Result<Checkout> {
    p.require(KeyScope::CheckoutsWrite)?;
    let input = input.normalize()?;
    let digest = fingerprint(&input)?;
    let mut tx = store.begin_merchant(p).await?;
    horizon(input.order_created_at, now(&mut tx).await?)?;
    if let Some(row)=sqlx::query("SELECT fingerprint,order_created_at,click_id,reason,policy FROM attribution_checkouts WHERE company_id=$1 AND store_id=$2 AND checkout_id=$3")
        .bind(p.company.0).bind(p.store.0).bind(&input.checkout_id.0).fetch_optional(&mut *tx).await.map_err(db)? {
        if row.try_get::<Vec<u8>,_>("fingerprint").map_err(db)?!=digest {return Err(Failure::Conflict);}
        let result=Checkout{
            checkout_id:input.checkout_id,
            order_created_at:parse_time(&row.try_get::<String,_>("order_created_at").map_err(db)?)?,
            click_id:row.try_get::<Option<Uuid>,_>("click_id").map_err(db)?.map(ClickId),
            reason:serde_json::from_value(Value::String(row.try_get("reason").map_err(db)?)).map_err(|_|Failure::Unavailable)?,
            policy:row.try_get("policy").map_err(db)?,
        };
        tx.commit().await.map_err(|_|Failure::Unavailable)?;return Ok(result);
    }
    let mut candidates = Vec::new();
    if input.consent && !input.candidate_click_ids.is_empty() {
        let ids: Vec<Uuid> = input.candidate_click_ids.iter().map(|id| id.0).collect();
        for row in sqlx::query("SELECT id,clicked_at,is_bot,is_prefetch,classifier_version,dimensions FROM attribution_clicks WHERE company_id=$1 AND store_id=$2 AND id=ANY($3)")
            .bind(p.company.0).bind(p.store.0).bind(ids).fetch_all(&mut *tx).await.map_err(db)? {
            candidates.push(Candidate{id:ClickId(row.try_get("id").map_err(db)?),clicked_at:row.try_get("clicked_at").map_err(db)?,is_bot:row.try_get("is_bot").map_err(db)?,is_prefetch:row.try_get("is_prefetch").map_err(db)?,classifier_version:row.try_get("classifier_version").map_err(db)?,dimensions:row.try_get("dimensions").map_err(db)?});
        }
    }
    let (result, selected) = select(&input, candidates);
    let evidence = selected
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| Failure::Unavailable)?;
    let reason = serde_json::to_value(result.reason).map_err(|_| Failure::Unavailable)?;
    sqlx::query("INSERT INTO attribution_checkouts(company_id,store_id,checkout_id,fingerprint,order_created_at,click_id,reason,policy,evidence) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(p.company.0).bind(p.store.0).bind(&result.checkout_id.0).bind(digest).bind(result.order_created_at.to_rfc3339()).bind(result.click_id.map(|id|id.0)).bind(reason.as_str().ok_or(Failure::Unavailable)?).bind(POLICY).bind(evidence).execute(&mut *tx).await.map_err(|_|Failure::Unavailable)?;
    tx.commit().await.map_err(|_| Failure::Unavailable)?;
    Ok(result)
}
