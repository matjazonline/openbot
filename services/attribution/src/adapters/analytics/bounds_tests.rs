use super::*;

// Bulk fixtures preserve the production schema and reuse a real accepted paid
// snapshot. The independent expected sums below do not invoke report helpers.
async fn bulk(w: &Commerce, n: i32) {
    sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id,paid_fingerprint,order_created_at,paid_original,paid_at,currency,merchandise_amount,tax_amount,shipping_amount,click_id,ad_id,attribution_reason,policy,evidence,dimensions) SELECT gen_random_uuid(),company_id,store_id,'bulk-'||i,paid_fingerprint,order_created_at,paid_original,paid_at,currency,10,0,0,click_id,ad_id,attribution_reason,policy,evidence,dimensions FROM attribution_orders CROSS JOIN generate_series(1,$1) i WHERE external_order_id='order-1'").bind(n).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query("INSERT INTO attribution_order_lines(company_id,store_id,order_id,line_id,product_id,quantity,merchandise_amount) SELECT company_id,store_id,id,'line',external_order_id,1,10 FROM attribution_orders WHERE external_order_id LIKE 'bulk-%'").execute(&w.c.f._database.pool).await.unwrap();
}

#[tokio::test]
async fn attribution_analytics_bounds_export_limit_formula_and_numeric_overflow() {
    let w = Commerce::new().await;
    let mut paid = w.paid();
    paid["lines"][0]["product_id"] = json!("=HYPERLINK(\"x\",\"y\")");
    paid["lines"][1]["product_id"] = json!("-danger");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    let path =
        query(&[("group_by", "sold_product")]).replace("/v1/analytics?", "/v1/analytics/export?");
    let (status, _csv) = document(&w, &path).await;
    // document has only a session cookie; management API additionally needs company.
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let export = |path: String| {
        w.c.f
            .request(&path)
            .header("x-company-id", w.c.f.upstream.company.0.to_string())
            .body(Body::empty())
            .unwrap()
    };
    let result =
        w.c.f
            .app
            .clone()
            .oneshot(export(path.clone()))
            .await
            .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let csv = String::from_utf8(
        to_bytes(result.into_body(), 4_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\",\"\"y\"\")\""));
    assert!(csv.contains("\"'-danger\""));
    // Two original product groups plus 9998 distinct groups hits the exact cap.
    bulk(&w, 9998).await;
    let page = w
        .get(&query(&[("group_by", "sold_product"), ("limit", "100")]))
        .await;
    assert_eq!(page.0, StatusCode::OK);
    assert_eq!(page.1["groups"].as_array().unwrap().len(), 100);
    assert!(page.1["next_cursor"].is_string());

    let result =
        w.c.f
            .app
            .clone()
            .oneshot(export(path.clone()))
            .await
            .unwrap();
    assert_eq!(result.status(), StatusCode::OK, "10000 data rows must fit");
    assert!(to_bytes(result.into_body(), 8_000_000).await.unwrap().len() > 1000000);
    sqlx::query("UPDATE attribution_order_lines SET product_id='extra-group' WHERE order_id=(SELECT id FROM attribution_orders WHERE external_order_id='order-1') AND line_id='line-y'").execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query("INSERT INTO attribution_order_lines(company_id,store_id,order_id,line_id,product_id,quantity,merchandise_amount) SELECT company_id,store_id,id,'extra','overflow-group',1,0 FROM attribution_orders WHERE external_order_id='order-1'").execute(&w.c.f._database.pool).await.unwrap();
    let result = w.c.f.app.clone().oneshot(export(path)).await.unwrap();
    assert_eq!(
        result.status(),
        StatusCode::BAD_REQUEST,
        "10001 data rows must reject"
    );
    sqlx::query("UPDATE attribution_order_lines SET merchandise_amount=9000000000000 WHERE product_id='bulk-1'").execute(&w.c.f._database.pool).await.unwrap();
    assert_eq!(
        w.get(&query(&[])).await.0,
        StatusCode::BAD_REQUEST,
        "aggregate overflow must not wrap or round"
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_analytics_bounds_representative_plan_and_timeout() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    bulk(&w, 999).await;
    sqlx::query("INSERT INTO attribution_clicks(id,company_id,store_id,link_id,clicked_at,dimensions,is_bot,is_prefetch,classifier_version) SELECT gen_random_uuid(),company_id,store_id,link_id,clicked_at,dimensions,i%10=0,i%20=0,classifier_version FROM attribution_clicks CROSS JOIN generate_series(1,9999) i").execute(&w.c.f._database.pool).await.unwrap();
    let install=change(&w.c.f,"POST","/v1/installations",json!({"store_id":w.c.store["id"],"domain_id":w.c.domain["domain"]["id"],"name":"Report observations"})).await;
    sqlx::query("INSERT INTO attribution_observations(company_id,store_id,installation_id,event_id,kind,click_id,observed_at,fingerprint,dimensions) SELECT company_id,store_id,$1,gen_random_uuid(),'image_load',id,clicked_at,decode(repeat('00',32),'hex'),dimensions FROM attribution_clicks ORDER BY id LIMIT 1000").bind(Uuid::parse_str(install["id"].as_str().unwrap()).unwrap()).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query("ANALYZE")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    let start = std::time::Instant::now();
    let (status, report) = w.get(&query(&[])).await;
    let elapsed = start.elapsed();
    assert_eq!(status, StatusCode::OK, "{report}");
    assert!(elapsed < std::time::Duration::from_secs(2));
    let g = &report["groups"][0];
    assert_eq!(g["raw_clicks"], 10000);
    assert_eq!(g["eligible_clicks"], 9001);
    assert_eq!(g["image_loads"], 1000);
    assert_eq!(g["currencies"][0]["paid_orders"], 1000);
    assert_eq!(g["currencies"][0]["merchandise_amount"], 15890);
    assert_eq!(g["currencies"][0]["net_merchandise_amount"], 14690);
    println!(
        "representative fixture: clicks10000 observations1000 orders1000 refund1; report HTTP elapsed {elapsed:?}"
    );
    // Explain the exact production statement, using the same bind types and scope.
    let sql = format!(
        "EXPLAIN (ANALYZE,BUFFERS,FORMAT TEXT) {}",
        crate::adapters::analytics::query::statement(None)
    );
    let plan: Vec<(String,)> = sqlx::query_as(&sql)
        .bind(w.c.f.upstream.company.0)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<Uuid>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(None::<String>)
        .bind(Utc::now() - chrono::Duration::days(1))
        .bind(Utc::now() + chrono::Duration::days(1))
        .bind("UTC")
        .bind(false)
        .bind(None::<String>)
        .bind(101_i64)
        .fetch_all(&w.c.f._database.pool)
        .await
        .unwrap();
    for line in plan {
        println!("{}", line.0);
    }
    let mut blocker = w.c.f._database.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE attribution_orders IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let start = std::time::Instant::now();
    assert_eq!(w.get(&query(&[])).await.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "query wait must be bounded"
    );
    blocker.rollback().await.unwrap();
    assert_eq!(
        w.get(&query(&[])).await.0,
        StatusCode::OK,
        "timeout returns usable pool capacity"
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_analytics_ledger_reasons_observations_full_refund_and_frozen_dimensions() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let mut rest = w.refund();
    rest["refund_id"] = json!("remaining");
    rest["lines"] = json!([{"line_id":"line-x","returned_quantity":1,"merchandise_amount":1200},{"line_id":"line-y","returned_quantity":1,"merchandise_amount":3500}]);
    rest["tax_amount"] = json!(940);
    rest["shipping_amount"] = json!(500);
    assert_eq!(w.event(&rest).await.1["state"], "accepted");
    // Fixed persisted ledger isolates the report oracle from attribution selection,
    // already exercised through actual ingress by commerce's reason tests.
    bulk(&w, 4).await;
    for (i, reason) in ["missing", "no_consent", "expired", "future"]
        .iter()
        .enumerate()
    {
        sqlx::query("UPDATE attribution_orders SET attribution_reason=$1,click_id=NULL,ad_id=NULL,evidence=NULL,dimensions=NULL WHERE external_order_id=$2").bind(reason).bind(format!("bulk-{}",i+1)).execute(&w.c.f._database.pool).await.unwrap();
    }
    let install=change(&w.c.f,"POST","/v1/installations",json!({"store_id":w.c.store["id"],"domain_id":w.c.domain["domain"]["id"],"name":"Report observations"})).await;
    for (kind, n) in [
        ("image_load", 2),
        ("landing_page_load", 3),
        ("visible_placement", 4),
    ] {
        sqlx::query("INSERT INTO attribution_observations(company_id,store_id,installation_id,event_id,kind,click_id,observed_at,fingerprint,dimensions,visibility_percent,visibility_millis) SELECT company_id,store_id,$1,gen_random_uuid(),$2,id,clicked_at,decode(repeat('00',32),'hex'),dimensions,CASE WHEN $2='visible_placement' THEN 50 ELSE NULL END,CASE WHEN $2='visible_placement' THEN 1000 ELSE NULL END FROM attribution_clicks CROSS JOIN generate_series(1,$3)").bind(Uuid::parse_str(install["id"].as_str().unwrap()).unwrap()).bind(kind).bind(n).execute(&w.c.f._database.pool).await.unwrap();
    }
    sqlx::query(
        "UPDATE attribution_ads SET advertised_product_id='changed',name='Changed' WHERE id=$1",
    )
    .bind(Uuid::parse_str(w.c.ad["id"].as_str().unwrap()).unwrap())
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    let (status, result) = w.get(&query(&[])).await;
    assert_eq!(status, StatusCode::OK);
    let g = &result["groups"][0];
    assert_eq!(g["attributed_paid_orders"], 1);
    assert_eq!(g["unattributed_paid_orders"], 4);
    for reason in ["missing", "no_consent", "expired", "future"] {
        assert_eq!(g["unattributed_reasons"][reason], 1);
    }
    assert_eq!(g["image_loads"], 2);
    assert_eq!(g["landing_page_loads"], 3);
    assert_eq!(g["visible_placements"], 4);
    let c = &g["currencies"][0];
    assert_eq!(c["paid_orders"], 5);
    assert_eq!(c["refunds"], 2);
    assert_eq!(c["net_merchandise_amount"], 40);
    assert_eq!(c["returned_units"], 3);
    assert_eq!(c["refunded_tax_amount"], 1180);
    assert_eq!(c["refunded_shipping_amount"], 500);
    let original = w.get(&query(&[("advertised_product_id", "P1")])).await.1;
    assert_eq!(original["groups"][0]["attributed_paid_orders"], 1);
    assert_eq!(original["groups"][0]["image_loads"], 2);
    assert_eq!(
        original["groups"][0]["currencies"][0]["net_merchandise_amount"],
        0
    );
    assert_eq!(
        w.get(&query(&[("advertised_product_id", "changed")]))
            .await
            .1["groups"][0]["raw_clicks"],
        0
    );
    let base = query(&[("group_by", "ad"), ("limit", "1")]);
    let first = w.get(&base).await.1;
    assert!(first["groups"][0]["group"].is_null());
    assert_eq!(first["groups"][0]["unattributed_paid_orders"], 4);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = w.get(&format!("{base}&cursor={cursor}")).await.1;
    assert_eq!(second["groups"][0]["group"], w.c.ad["id"]);
    assert!(second["next_cursor"].is_null());
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_analytics_export_parsed_currency_and_product_semantics() {
    use tokio::io::AsyncWriteExt;
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    let mut usd = w.paid();
    usd["order_id"] = json!("usd");
    usd["currency"] = json!("USD");
    assert_eq!(w.event(&usd).await.1["state"], "accepted");
    for (mode, filters) in [
        ("currencies", vec![]),
        ("products", vec![("group_by", "sold_product")]),
    ] {
        let path = query(&filters).replace("/v1/analytics?", "/v1/analytics/export?");
        let request =
            w.c.f
                .request(&path)
                .header("x-company-id", w.c.f.upstream.company.0.to_string())
                .body(Body::empty())
                .unwrap();
        let response = w.c.f.app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let csv = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        let mut child = tokio::process::Command::new("python3")
            .arg("scripts/check-report-csv.py")
            .arg(mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("Python3 required for independent CSV parser");
        child.stdin.take().unwrap().write_all(&csv).await.unwrap();
        let output = child.wait_with_output().await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }
    w.c.f.close().await;
}
