use super::*;
use crate::application::workflow::children::WorkflowChildren;

#[tokio::test]
async fn workflow_control_authority_lock_allows_runtime_fk_and_fences_owner_mutation() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::authority::authorize_company(
        &mut tx,
        scope.company,
        f.binding.target.actor,
    )
    .await
    .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    authority_fk_and_mutation_probes(&f, scope).await;
    let completion = f.persistence().complete_io(FencedWorkflowResult {
        policy: crate::application::workflow::lease::LeasePolicy::new(
            std::time::Duration::from_secs(3),
        )
        .unwrap(),
        fence: claim.fence,
        output: json!({"items":[],"token_count":0,"omitted":0}),
    });
    tokio::pin!(completion);
    tokio::select! {
        result = completion.as_mut() => panic!("runtime completion bypassed company authority: {result:?}"),
        () = completion_company_wait(&f, blocker) => {},
    }
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("completion must await company before taking run ownership");
    tx.rollback().await.unwrap();
    let result = completion
        .await
        .unwrap()
        .expect("actual completion commits after authority releases");
    assert_eq!(
        result.output,
        json!({"items":[],"token_count":0,"omitted":0})
    );
}

async fn authority_fk_and_mutation_probes(f: &AdmissionFixture, scope: ActivationRequest) {
    // A real PostgreSQL FK against the held production company retains KEY SHARE
    // compatibility. Full completion is a company writer and now serializes separately.
    let mut probe = f.persistence().pool().begin().await.unwrap();
    let table = format!("workflow_control_fk_{}", Uuid::new_v4().simple());
    // PostgreSQL rejects a TEMP-to-permanent FK. This ordinary relation and its
    // inserted row exist only in this transaction and disappear on rollback.
    let definition =
        format!("CREATE TABLE {table}(company_id uuid NOT NULL REFERENCES public.companies(id))");
    sqlx::raw_sql(&definition)
        .execute(&mut *probe)
        .await
        .unwrap();
    let insert = format!("INSERT INTO {table}(company_id) VALUES($1) RETURNING company_id");
    let inserted: Uuid = sqlx::query_scalar(&insert)
        .bind(scope.company.as_uuid())
        .fetch_one(&mut *probe)
        .await
        .unwrap();
    assert_eq!(inserted, scope.company.as_uuid());
    probe.rollback().await.unwrap();
    for sql in [
        "UPDATE companies SET name=name WHERE id=$1",
        "DELETE FROM companies WHERE id=$1",
    ] {
        let mut contender = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *contender)
            .await
            .unwrap();
        let error = sqlx::query(sql)
            .bind(scope.company.as_uuid())
            .execute(&mut *contender)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        contender.rollback().await.unwrap();
    }
}

async fn completion_company_wait(f: &AdmissionFixture, blocker: i32) {
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut *observer).await.unwrap();
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.datname=current_database() AND activity.state='active' AND activity.wait_event_type='Lock' AND activity.query LIKE '%FROM companies%FOR NO KEY UPDATE%' AND $1=ANY(pg_blocking_pids(activity.pid)))")
                .bind(blocker).fetch_one(&mut *observer).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("exact company authority wait must be observed");
}

#[tokio::test]
async fn workflow_control_receipt_delete_and_deferred_fault_preserve_all_writes() {
    let (f, scope) = fixture().await;
    f.persistence()
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let before = snapshot(&f).await;
    sqlx::query("CREATE FUNCTION control_test_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'control fault'; END $$")
        .execute(f.persistence().pool()).await.unwrap();
    sqlx::query("CREATE CONSTRAINT TRIGGER control_test_fault AFTER INSERT ON workflow_control_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION control_test_fault()")
        .execute(f.persistence().pool()).await.unwrap();
    let cmd = command(&f, scope, "cancel").await;
    assert!(f.persistence().cancel(cmd.clone()).await.is_err());
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(receipts(&f).await, json!([]));
    sqlx::query("DROP TRIGGER control_test_fault ON workflow_control_commands")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    f.persistence().cancel(cmd).await.unwrap();
    let saved = receipts(&f).await;
    assert!(
        sqlx::query("DELETE FROM workflow_control_commands WHERE company_id=$1")
            .bind(scope.company.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    assert_eq!(receipts(&f).await, saved);
}

#[tokio::test]
async fn workflow_control_cancel_competes_with_child_creation() {
    // Competing claimants on the parent run lock (07-D1 §D1.5): whichever commits first, no live
    // child remains under a cancelled parent.
    let source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    for _ in 0..4 {
        let (f, scope) = AdmissionFixture::caller(source.clone()).await;
        let cancel = command(&f, scope, "cancel").await;
        let barrier = Barrier::new(2);
        let park = async {
            barrier.wait().await;
            f.persistence().park_child(scope).await.unwrap()
        };
        let control = async {
            barrier.wait().await;
            f.persistence().cancel(cancel).await.unwrap()
        };
        let (parked, cancelled) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(park, control)
        })
        .await
        .unwrap();
        // A park that commits first advances the parent's revision, so a cancel prepared
        // before it conflicts and the operator retries with the current revision.
        if !matches!(cancelled, CancelResult::Applied { .. }) {
            assert!(parked.is_some(), "{cancelled:?}");
            assert!(matches!(cancelled, CancelResult::RevisionConflict { .. }));
            let retried = f
                .persistence()
                .cancel(command(&f, scope, "cancel-current").await)
                .await
                .unwrap();
            assert!(
                matches!(retried, CancelResult::Applied { .. }),
                "{retried:?}"
            );
        }
        let live: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1 AND state IN ('queued','running','waiting')")
            .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
        assert_eq!(live, 0);
        let children: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
                .bind(scope.run.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(children, i64::from(parked.is_some()));
    }
}
