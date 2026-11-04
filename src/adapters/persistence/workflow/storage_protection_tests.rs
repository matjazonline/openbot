use super::*;

#[tokio::test]
async fn run_retention_protects_open_decision_until_its_wait_closes() {
    let d = Box::pin(decision_tests::DecisionFixture::new(1, None)).await;
    d.park().await;
    let f = &d.admission;
    run_finished_at_tests::drive(
        f,
        d.scope.run,
        crate::application::workflow::reads::RunStateName::Cancelled,
    )
    .await;
    let root = age(f, d.scope).await;
    assert_eq!(f.persistence().retire_tree(root).await.unwrap(), None);
    assert_eq!(count(f, "workflow_decisions").await, 1);
    sqlx::query(
        "UPDATE workflow_waits SET state='cancelled',settled_at=clock_timestamp() WHERE run_id=$1",
    )
    .bind(d.scope.run.as_uuid())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    assert!(f.persistence().retire_tree(root).await.unwrap().is_some());
    assert_eq!(count(f, "workflow_decisions").await, 0);
}
#[tokio::test]
async fn run_retention_external_rerun_blocks_referent_until_independent_root_is_removed() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "original").await;
    let original = old(&f, scope).await;
    let mut request = f.request("rerun", f.manual());
    request.rerun_of = Some(scope.run);
    let prepared = f.prepare(request).await;
    f.persistence().admit(&prepared).await.unwrap();
    let rerun = child_support::entry(&f, prepared.proposed_run_id()).await;
    let rerun = old(&f, rerun).await;
    assert_eq!(f.persistence().retire_tree(original).await.unwrap(), None);
    assert!(f.persistence().retire_tree(rerun).await.unwrap().is_some());
    assert!(
        f.persistence()
            .retire_tree(original)
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn run_retention_deferred_job_provenance_still_rejects_native_commit() {
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "native-fk").await;
    let root = old(&f, scope).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("DELETE FROM workflow_runs WHERE id=$1")
        .bind(root.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("background_tasks_workflow_execution_fk")
    );
    assert_eq!(count(&f, "workflow_runs").await, 1);
    assert_eq!(count(&f, "background_tasks").await, 1);
}

#[tokio::test]
async fn run_retention_shutdown_rolls_back_the_actual_inflight_archive_and_delete() {
    use crate::{adapters::monitoring::InMemoryMonitor, application::workflow::storage};
    use tokio_util::sync::CancellationToken;
    let f = AdmissionFixture::new().await;
    let scope = admitted(&f, "cancel-write").await;
    usage(&f, scope, None).await;
    old(&f, scope).await;
    sqlx::raw_sql("CREATE FUNCTION retained_job_barrier() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(602207); RETURN OLD; END $$;
        CREATE TRIGGER retained_job_barrier BEFORE DELETE ON background_tasks FOR EACH ROW EXECUTE FUNCTION retained_job_barrier()")
        .execute(f.persistence().pool()).await.unwrap();
    let mut blocker = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(602207)")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    let monitor = InMemoryMonitor::new();
    let stop = async {
        tokio::time::timeout(std::time::Duration::from_secs(3),async {
            loop {
                let pid:Option<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event='advisory' AND query LIKE 'DELETE FROM background_tasks AS job USING pg_temp.workflow_retention_jobs%'")
                    .fetch_optional(f.persistence().pool()).await.unwrap();
                if let Some(pid)=pid {shutdown.cancel();break pid;}
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap()
    };
    let (_, pid) = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        tokio::join!(storage::run(f.persistence(), &monitor, &shutdown), stop)
    })
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3),async{
        loop {
            let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND backend_xid IS NOT NULL)")
                .bind(pid).fetch_one(f.persistence().pool()).await.unwrap();
            if !active {break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(count(&f, "workflow_runs").await, 1);
    assert_eq!(count(&f, "background_tasks").await, 1);
    assert_eq!(count(&f, "model_usage_calls").await, 1);
    assert_eq!(count(&f, "model_usage_history").await, 0);
}
