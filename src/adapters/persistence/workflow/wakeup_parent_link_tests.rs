use super::*;

#[tokio::test]
async fn workflow_wakeup_parent_link_is_immutable_and_owner_cascades() {
    let (f, parent) = fixture(example("data.map")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    let pool = f.persistence().pool();
    for query in [
        "UPDATE workflow_run_parents SET parent_execution_id=child_run_id WHERE child_run_id=$1",
        "DELETE FROM workflow_run_parents WHERE child_run_id=$1",
    ] {
        assert!(
            sqlx::query(query)
                .bind(child.run.as_uuid())
                .execute(pool)
                .await
                .is_err()
        );
    }
    // Remove the parent's own job so its FK cannot mask the cross-tree guard.
    sqlx::query("DELETE FROM background_tasks WHERE id=$1")
        .bind(parent.job.0)
        .execute(pool)
        .await
        .unwrap();
    let error = sqlx::query("DELETE FROM workflow_executions WHERE id=$1")
        .bind(parent.execution.as_uuid())
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_parent_execution_fk")
    );
    sqlx::query("DELETE FROM background_tasks WHERE id=$1")
        .bind(child.job.0)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_runs WHERE id=$1")
        .bind(child.run.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_parents")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(links, 0);
    let child = admit_child(&f, parent, "start", "another-child").await;
    assert_ne!(child.run, parent.run);
    sqlx::query("DELETE FROM background_tasks WHERE id=$1")
        .bind(child.job.0)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(parent.company.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_parents")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(links, 0);
}

#[tokio::test]
async fn workflow_wakeup_parent_link_requires_exact_committed_owner() {
    let (f, parent) = fixture(example("data.map")).await;
    let pool = f.persistence().pool();
    // A real parent is insufficient: neither an absent child nor a root run
    // with no declared parent may own this lineage tuple.
    for child in [Uuid::new_v4(), parent.run.as_uuid()] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO workflow_run_parents(company_id,child_run_id,parent_run_id,parent_execution_id) VALUES($1,$2,$3,$4)")
            .bind(parent.company.as_uuid()).bind(child).bind(parent.run.as_uuid())
            .bind(parent.execution.as_uuid()).execute(&mut *tx).await.unwrap();
        let error = tx.commit().await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_parent_owner_fk")
        );
    }
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_parents")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(links, 0);
}

#[tokio::test]
async fn workflow_wakeup_parent_link_rolls_back_with_failed_run_insert() {
    let (f, parent) = fixture(example("data.map")).await;
    let pool = f.persistence().pool();
    let child = Uuid::new_v4();
    // The BEFORE trigger creates the link, then the run's budget CHECK fails.
    let error = sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'parent_run_id',$1::uuid,'parent_execution_id',$3::uuid,'max_steps',0))).* FROM workflow_runs AS run WHERE run.id=$1")
        .bind(parent.run.as_uuid()).bind(child).bind(parent.execution.as_uuid())
        .execute(pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_parents")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(links, 0);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE id=$1")
        .bind(child)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(runs, 0);
}
