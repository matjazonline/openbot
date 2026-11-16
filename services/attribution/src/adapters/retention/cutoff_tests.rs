use super::*;
use crate::{adapters::postgres, test_support::Database};

#[tokio::test]
async fn attribution_retention_exact_cutoffs_and_protected_delivery_evidence() {
    let db = Database::new().await;
    postgres::migrate(&db.pool).await.unwrap();
    let company = Uuid::new_v4();
    let mut tx = db.pool.begin().await.unwrap();
    // All clocks and the production cleanup predicate share this transaction's
    // fixed instant. The paired row one microsecond older is the control.
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',transaction_timestamp())").bind(company).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$1,'Fixture','UTC')",
    )
    .bind(company)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attribution_domains(id,company_id,store_id,host,challenge) VALUES($1,$1,$1,'shop.example.test',repeat('x',43))").bind(company).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO attribution_installations(id,company_id,store_id,domain_id,name) VALUES($1,$1,$1,$1,'Fixture')").bind(company).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_campaigns(id,company_id,store_id,name) VALUES($1,$1,$1,'Fixture')",
    )
    .bind(company)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attribution_ads(id,company_id,store_id,campaign_id,name) VALUES($1,$1,$1,$1,'Fixture')").bind(company).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO attribution_links(id,company_id,store_id,ad_id,code,destination,placement) VALUES($1,$1,$1,$1,repeat('x',24),'https://shop.example.test/','Fixture')").bind(company).execute(&mut *tx).await.unwrap();
    for n in [0, 1] {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO attribution_clicks(id,company_id,store_id,link_id,clicked_at,dimensions,is_bot,is_prefetch,classifier_version) VALUES($1,$2,$2,$2,transaction_timestamp()-interval '38 days'-$3*interval '1 microsecond','{}',false,false,1)").bind(id).bind(company).bind(n).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_observations(company_id,store_id,installation_id,event_id,kind,link_id,observed_at,received_at,fingerprint,dimensions) VALUES($1,$1,$1,$2,'image_load',$1,transaction_timestamp(),transaction_timestamp()-interval '30 days'-$3*interval '1 microsecond',decode(repeat('00',32),'hex'),'{}')").bind(company).bind(id).bind(n).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_checkouts(company_id,store_id,checkout_id,fingerprint,order_created_at,reason,policy,received_at) VALUES($1,$1,$2,decode(repeat('00',32),'hex'),'fixture','missing','last-eligible-click-30d-v1',transaction_timestamp()-interval '38 days'-$3*interval '1 microsecond')").bind(company).bind(format!("cutoff-{n}")).bind(n).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id,received_at) VALUES($1,$2,$2,$3,transaction_timestamp()-interval '400 days'-$4*interval '1 microsecond')").bind(id).bind(company).bind(format!("cutoff-{n}")).bind(n).execute(&mut *tx).await.unwrap();
        // Separate recent parent isolates the event cutoff from financial expiry.
        let parent = Uuid::new_v4();
        sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id) VALUES($1,$2,$2,$3)").bind(parent).bind(company).bind(format!("event-{n}")).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_merchant_events(id,company_id,store_id,order_id,external_event_id,kind,fingerprint,envelope,occurred_at,received_at,state) VALUES($1,$2,$2,$3,$4,'cancelled',decode(repeat('00',32),'hex'),'{}',transaction_timestamp(),transaction_timestamp()-interval '8 days'-$5*interval '1 microsecond','accepted')").bind(id).bind(company).bind(parent).bind(format!("cutoff-{n}")).bind(n).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_management_receipts(company_id,principal_kind,principal_id,operation,key,original_time,fingerprint,result,created_at) VALUES($1,'user',$1,'fixture',$2,'fixture',decode(repeat('00',32),'hex'),'{}',transaction_timestamp()-interval '48 hours'-$3*interval '1 microsecond')").bind(company).bind(id).bind(n).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO attribution_audit(id,company_id,actor_kind,actor_id,action,resource_id,created_at) VALUES($1,$2,'user',$2,'fixture',$1,transaction_timestamp()-interval '90 days'-$3*interval '1 microsecond')").bind(id).bind(company).bind(n).execute(&mut *tx).await.unwrap();
    }
    let mut budget = BATCH_ROWS;
    prune::retained(&mut tx, company, db.config().retention, &mut budget)
        .await
        .unwrap();
    for table in [
        "attribution_clicks",
        "attribution_observations",
        "attribution_checkouts",
        "attribution_merchant_events",
        "attribution_management_receipts",
        "attribution_audit",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            count, 1,
            "{table}: exact boundary retained, older control pruned"
        );
    }
    let orders: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attribution_orders WHERE external_order_id LIKE 'cutoff-%'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(orders, 1);
    tx.rollback().await.unwrap();
}
