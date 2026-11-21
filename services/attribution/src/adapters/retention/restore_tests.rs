use super::*;
use sqlx::{ConnectOptions, Connection};

async fn command(name: &str, database: &Database, args: &[&str]) {
    let mut url = database.options.to_url_lossy();
    let password = url.password().unwrap_or("").to_owned();
    url.set_password(None).unwrap();
    // libpq does not recognize SQLx's statement-cache-capacity parameter.
    let options: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "statement-cache-capacity")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(options);
    let result = tokio::process::Command::new(name)
        .args(args)
        .arg("--dbname")
        .arg(url.as_str())
        .env("PGPASSWORD", password)
        .env("PGCONNECT_TIMEOUT", "5")
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{name} failed (diagnostics intentionally withheld)"
    );
}
#[tokio::test]
async fn attribution_recovery_real_dump_restore_auth_ledger_pending_and_isolation() {
    let mut w = Commerce::new().await;
    let paid = w.event(&w.paid()).await.1;
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let mut late = w.paid();
    late["order_id"] = json!("late-order");
    let mut pending = w.refund();
    pending["order_id"] = json!("late-order");
    pending["refund_id"] = json!("late-refund");
    assert_eq!(w.event(&pending).await.0, StatusCode::ACCEPTED);
    let tombstone = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at,purge_requested,purged_at) VALUES($1,9,'deleted',clock_timestamp(),true,clock_timestamp())").bind(tombstone).execute(&w.c.f._database.pool).await.unwrap();
    let path = format!("/v1/orders/{}", paid["order_id"].as_str().unwrap());
    let before = w.get(&path).await.1;
    let target = Database::new().await;
    let dump = std::env::temp_dir().join(format!("as-backup-{}.dump", Uuid::new_v4()));
    let started = std::time::Instant::now();
    command(
        "pg_dump",
        &w.c.f._database,
        &[
            "--format=custom",
            "--no-owner",
            "--no-acl",
            "--file",
            dump.to_str().unwrap(),
        ],
    )
    .await;
    let backup = started.elapsed();
    command(
        "pg_restore",
        &target,
        &[
            "--no-owner",
            "--no-acl",
            "--exit-on-error",
            dump.to_str().unwrap(),
        ],
    )
    .await;
    std::fs::remove_file(dump).unwrap();
    let restore = started.elapsed() - backup;
    crate::adapters::postgres::validate_database(&target.pool)
        .await
        .unwrap();
    assert!(
        sqlx::PgConnection::connect_with(
            &target
                .options
                .clone()
                .database(w.c.f._database.options.get_database().unwrap())
        )
        .await
        .is_err()
    );
    assert!(
        sqlx::PgConnection::connect_with(
            &w.c.f
                ._database
                .options
                .clone()
                .database(target.options.get_database().unwrap())
        )
        .await
        .is_err()
    );
    let original = std::mem::replace(&mut w.c.f._database, target);
    let pool = w.c.f._database.pool.clone();
    w.c.f.setup.identity = compose(pool.clone(), w.c.f.setup.identity.config.clone()).unwrap();
    w.c.f.service = w.c.f.setup.identity.service.clone();
    w.c.f.store = Arc::new(IdentityPostgres {
        pool: pool.clone(),
        cipher: w.c.f.setup.identity.config.cipher.clone(),
    });
    w.c.f.setup.store.pool = pool.clone();
    rebuild(&mut w.c.f);
    assert_eq!(
        w.get(&path).await.1,
        before,
        "restored encrypted session and exact financial ledger"
    );
    assert_eq!(
        w.event(&late).await.1["state"],
        "accepted",
        "restored merchant verifier and checkout"
    );
    recovery_tests::due(&w).await;
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    assert_eq!(
        worker::process(&w.worker(), claim).await.unwrap(),
        EventState::Accepted
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT sum(refunded_merchandise_amount)::bigint FROM attribution_orders"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        2400
    );
    let mut status = w.c.f.upstream.status();
    status.company_id = CompanyId(tombstone);
    status.version = 10;
    assert!(matches!(
        w.c.f
            .store
            .observe(&status, Some("delayed"), None, None)
            .await,
        Err(Failure::Forbidden)
    ));
    println!(
        "independent dump={}ms restore={}ms recovered_auth_ledger_pending={}ms (fixture measurements, no production RPO/RTO)",
        backup.as_millis(),
        restore.as_millis(),
        started.elapsed().as_millis()
    );
    drop(original);
    w.c.f.close().await;
}
