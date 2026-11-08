use super::*;

async fn delete(f: &ModelFixture) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("DELETE FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(f.write.fence.scope.company.as_uuid())
        .bind(f.write.fence.scope.execution.as_uuid())
        .execute(f.native.fixture.persistence().pool())
        .await
}

#[tokio::test]
async fn agent_history_storage_failed_exchange_delete_and_actual_control_retry_do_not_deadlock() {
    let f = Box::pin(settled_history()).await;
    fail(&f).await;
    let original = exchange(&f).await;
    let scope = f.write.fence.scope;
    let fixture = &f.native.fixture;
    // Hold the real job before retry's new exchange/cap preflight. Once retry
    // owns the run and blocks here, DELETE must refuse its reverse run lock NOWAIT.
    let mut blocker = fixture.persistence().pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM background_tasks WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .bind(scope.job.0)
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let command = control(&f, "delete-versus-retry").await;
    let retry = RetryCommand {
        company_id: command.company_id,
        run_id: command.run_id,
        actor: command.actor,
        command_key: command.command_key,
        expected_revision: command.expected_revision,
    };
    let p = fixture.persistence().clone();
    let pending = tokio::spawn(async move { p.retry(retry).await });
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker_pid).fetch_one(fixture.persistence().pool()).await.unwrap();
            if blocked { break; }
            assert!(!pending.is_finished(),"actual retry must reach its job lock");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(2), delete(&f))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    blocker.rollback().await.unwrap();
    assert!(matches!(
        pending.await.unwrap().unwrap(),
        RetryResult::Applied { .. }
    ));
    let queued = exchange(&f).await;
    assert_eq!(queued["state"], "queued");
    for field in [
        "id",
        "position",
        "checkpoint_id",
        "input_digest",
        "provenance",
    ] {
        assert_eq!(queued[field], original[field], "{field}");
    }
}

#[tokio::test]
async fn agent_history_storage_expired_failed_exchange_can_release_its_source_pin() {
    let f = Box::pin(settled_history()).await;
    fail(&f).await;
    let p = f.native.fixture.persistence();
    crate::adapters::persistence::workflow::agent_history_retention_fixture::expire(
        p,
        f.write.fence.scope,
    )
    .await;
    let command = control(&f, "expired-retry").await;
    let result = p
        .retry(RetryCommand {
            company_id: command.company_id,
            run_id: command.run_id,
            actor: command.actor,
            command_key: command.command_key,
            expected_revision: command.expected_revision,
        })
        .await
        .unwrap();
    assert!(!matches!(result, RetryResult::Applied { .. }));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
            .bind(f.write.fence.scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "expired metadata still pins its exact source until retirement"
    );
    crate::adapters::persistence::workflow::agent_history_retention_fixture::retire_source(
        p,
        f.write.fence.scope,
    )
    .await;
}

#[tokio::test]
async fn agent_history_storage_nonresumable_failed_exchange_can_be_deleted_before_deadline() {
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let f = Box::pin(settled_history()).await;
    let p = f.native.fixture.persistence();
    let failure = WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("agent.budget").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    };
    p.settled()
        .release_io(
            f.write.fence,
            f.write.lease_policy,
            LeaseReleaseCause::Classified(failure),
        )
        .await
        .unwrap();
    assert_eq!(exchange(&f).await["state"], "failed");
    // Metadata cannot be erased while its source remains, even for a terminal failure.
    assert_eq!(
        delete(&f)
            .await
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    crate::adapters::persistence::workflow::agent_history_retention_fixture::retire(
        p,
        f.write.fence.scope,
    )
    .await;
}

#[tokio::test]
async fn agent_history_storage_company_teardown_removes_even_retryable_failed_exchange() {
    let f = Box::pin(settled_history()).await;
    fail(&f).await;
    let p = f.native.fixture.persistence();
    let company = f.write.fence.scope.company.as_uuid();
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company)
        .execute(p.pool())
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1")
            .bind(company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}
