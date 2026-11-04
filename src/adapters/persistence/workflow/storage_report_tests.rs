use super::*;
use crate::application::workflow::metrics::WorkflowMetricPersistence;

#[tokio::test]
async fn run_retention_reports_preserve_categories_filters_time_prices_and_live_union() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "report-old").await;
    for category in [Some("reported"), Some("estimated"), Some("mixed"), None] {
        usage(&f, scope, category).await;
    }
    let live = admitted(&f, "report-live").await;
    usage(&f, live, Some("reported")).await;
    sqlx::query("INSERT INTO model_prices(company_id,provider,model,input_micro_usd_per_million,output_micro_usd_per_million) VALUES($1,'openai','fixture',1000000,2000000)")
        .bind(scope.company.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let root = old(&f, scope).await;
    let base = query(&f);
    let (agent,workflow,requested_at):(Uuid,Uuid,chrono::DateTime<Utc>)=sqlx::query_as("SELECT agent_id,workflow_id,requested_at FROM model_usage_calls WHERE run_id=$1 ORDER BY requested_at,request_id LIMIT 1")
        .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    let mut cases = vec![base.clone()];
    let mut q = base.clone();
    q.grouping = UsageGrouping::Workflow;
    cases.push(q);
    let mut q = base.clone();
    q.agent = Some(agent);
    cases.push(q);
    let mut q = base.clone();
    q.workflow = Some(workflow);
    cases.push(q);
    let mut q = base.clone();
    q.start = requested_at;
    q.end = requested_at + Duration::microseconds(1);
    cases.push(q);
    let mut q = base.clone();
    q.end = requested_at;
    cases.push(q);
    let mut before = Vec::new();
    for q in &cases {
        before.push(report(&f, q.clone()).await);
    }
    let metrics = f.persistence().workflow_metrics().await.unwrap();
    assert_eq!(before[0]["groups"][0]["reported_calls"], 2);
    assert_eq!(before[0]["groups"][0]["estimated_calls"], 1);
    assert_eq!(before[0]["groups"][0]["mixed_calls"], 1);
    assert_eq!(before[0]["groups"][0]["unconfirmed_calls"], 1);
    assert_eq!(
        f.persistence()
            .retire_tree(root)
            .await
            .unwrap()
            .unwrap()
            .calls,
        4
    );
    assert_eq!(count(&f, "model_usage_calls").await, 1);
    for (q, expected) in cases.into_iter().zip(&before) {
        assert_eq!(report(&f, q).await, *expected);
    }
    let after = f.persistence().workflow_metrics().await.unwrap();
    assert_eq!(
        (
            metrics.reserved_tokens,
            metrics.charged_tokens_last24h,
            metrics.charged_estimated_usd_last24h,
            metrics.unpriced_model_calls
        ),
        (
            after.reserved_tokens,
            after.charged_tokens_last24h,
            after.charged_estimated_usd_last24h,
            after.unpriced_model_calls
        )
    );
    sqlx::query("UPDATE model_prices SET input_micro_usd_per_million=input_micro_usd_per_million*2,output_micro_usd_per_million=output_micro_usd_per_million*2,revision=nextval('model_price_revision')")
        .execute(f.persistence().pool()).await.unwrap();
    let repriced = report(&f, base).await;
    assert_ne!(
        repriced["groups"][0]["priced_subtotal_usd"],
        before[0]["groups"][0]["priced_subtotal_usd"]
    );
    assert_eq!(
        repriced["groups"][0]["input_tokens"],
        before[0]["groups"][0]["input_tokens"]
    );
}

#[tokio::test]
async fn run_retention_report_global_scan_cap_applies_before_filters_across_sources() {
    let f = AdmissionFixture::new().await;
    let archived = admitted(&f, "cap-archive").await;
    let live = admitted(&f, "cap-live").await;
    usage(&f, archived, Some("reported")).await;
    usage(&f, live, Some("reported")).await;
    // The report must also handle native ledgers larger than one execution's model budget.
    // Clone valid provenance with distinct requests, without manufacturing usage history.
    sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,
        to_jsonb(original)||jsonb_build_object('request_id',gen_random_uuid(),
            'requested_at',original.requested_at-interval '1 hour'))).*
        FROM model_usage_calls AS original CROSS JOIN generate_series(1,5000) WHERE original.run_id=$1")
        .bind(archived.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO model_usage_calls SELECT (jsonb_populate_record(NULL::model_usage_calls,
        to_jsonb(original)||jsonb_build_object('request_id',gen_random_uuid()))).*
        FROM model_usage_calls AS original CROSS JOIN generate_series(1,5000) WHERE original.run_id=$1")
        .bind(live.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let root = old(&f, archived).await;
    let q = query(&f);
    let before = report(&f, q.clone()).await;
    assert_eq!(before["scanned_calls"], 10000);
    assert_eq!(before["truncated"], true);
    f.persistence().retire_tree(root).await.unwrap().unwrap();
    assert_eq!(count(&f, "model_usage_history").await, 5001);
    assert_eq!(report(&f, q).await, before);
}
