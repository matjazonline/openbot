use super::*;
use std::time::Duration as StdDuration;

#[tokio::test]
async fn bundle_store_shared_exact_bytes_and_native_scope_immutability() {
    let f = AdmissionFixture::new().await;
    let first = f.prepare(f.request("shared-1", f.manual())).await;
    let second = f.prepare(f.request("shared-2", f.manual())).await;
    f.persistence().admit(&first).await.unwrap();
    f.persistence().admit(&second).await.unwrap();
    assert_eq!(count(&f, "workflow_runs").await, 2);
    assert_eq!(count(&f, "workflow_bundles").await, 1);
    let saved: Vec<u8> = sqlx::query_scalar(
        "SELECT blob.bundle FROM workflow_runs AS run JOIN workflow_bundles AS blob
        ON blob.company_id=run.company_id AND blob.digest=run.bundle_digest WHERE run.id=$1",
    )
    .bind(first.proposed_run_id().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(saved, store_bundle(first.binding().bundle()).unwrap());
    for sql in [
        "UPDATE workflow_bundles SET bundle=bundle",
        "DELETE FROM workflow_bundles",
    ] {
        assert!(
            sqlx::query(sql)
                .execute(f.persistence().pool())
                .await
                .is_err()
        );
    }
    let foreign = f.foreign().await;
    let scope = admitted(&foreign, "scope").await;
    let digest: Vec<u8> = sqlx::query_scalar("SELECT bundle_digest FROM workflow_runs WHERE id=$1")
        .bind(first.proposed_run_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE workflow_runs SET bundle_digest=$2 WHERE id=$1")
            .bind(scope.run.as_uuid())
            .bind(digest)
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("INSERT INTO workflow_bundles(company_id,digest,bundle) VALUES($1,$2,$3)")
            .bind(first.company_id().as_uuid())
            .bind(vec![0_u8; 32])
            .bind(vec![1_u8])
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    for bytes in [Vec::new(), vec![0; 4 * 1024 * 1024 + 1]] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        assert!(
            bundle_store::insert_on(&mut tx, first.company_id(), &bytes)
                .await
                .is_err()
        );
        tx.rollback().await.unwrap();
    }
}

async fn wait_lock(f: &AdmissionFixture, pattern: &str) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity
                WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE $1)",
            )
            .bind(pattern)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(StdDuration::from_millis(10)).await;
        }
    })
    .await
    .expect("production writer reached the controlled database lock");
}
async fn install_barrier(f: &AdmissionFixture, admission_first: bool) {
    let (table, event, image) = if admission_first {
        ("workflow_runs", "INSERT", "NEW")
    } else {
        ("workflow_bundles", "DELETE", "OLD")
    };
    sqlx::raw_sql(&format!("CREATE FUNCTION bundle_test_barrier() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(602206); RETURN {image}; END $$;
        CREATE TRIGGER bundle_test_barrier BEFORE {event} ON {table} FOR EACH ROW EXECUTE FUNCTION bundle_test_barrier()"))
        .execute(f.persistence().pool()).await.unwrap();
}
async fn competing_gc(admission_first: bool) {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("gc-race", f.manual())).await;
    let mut seed = f.persistence().pool().begin().await.unwrap();
    bundle_store::insert_on(
        &mut seed,
        command.company_id(),
        &store_bundle(command.binding().bundle()).unwrap(),
    )
    .await
    .unwrap();
    seed.commit().await.unwrap();
    install_barrier(&f, admission_first).await;
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(602206)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let persistence = f.persistence().clone();
    let gc = if admission_first {
        None
    } else {
        Some(tokio::spawn(async move {
            persistence.sweep_unused_bundles(256).await
        }))
    };
    if !admission_first {
        wait_lock(&f, "%DELETE FROM workflow_bundles AS blob%").await;
    }
    let persistence = f.persistence().clone();
    let writer = tokio::spawn(async move { persistence.admit(&command).await });
    wait_lock(
        &f,
        if admission_first {
            "%INSERT INTO workflow_runs%"
        } else {
            "INSERT INTO workflow_bundles%"
        },
    )
    .await;
    if admission_first {
        assert_eq!(f.persistence().sweep_unused_bundles(256).await.unwrap(), 0);
    }
    blocker.commit().await.unwrap();
    if let Some(gc) = gc {
        assert_eq!(gc.await.unwrap().unwrap(), 1);
    }
    assert!(matches!(
        writer.await.unwrap().unwrap(),
        AdmissionResult::Created(_)
    ));
    assert_eq!(count(&f, "workflow_runs").await, 1);
    assert_eq!(count(&f, "workflow_bundles").await, 1);
}
#[tokio::test]
async fn bundle_store_admission_holds_reused_blob_against_gc() {
    competing_gc(true).await;
}
#[tokio::test]
async fn bundle_store_gc_first_waits_and_admission_recreates_payload() {
    competing_gc(false).await;
}
