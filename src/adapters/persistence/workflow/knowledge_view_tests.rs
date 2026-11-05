//! Current classification reads and a real pair of readers racing a demotion/content raise.
use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::workflow::run_views::{RunReadScope, RunViews};
use crate::entities::access_level::AccessLevel;

fn read_scope(setup: &Setup, user: Uuid) -> RunReadScope {
    RunReadScope {
        company: setup.scope().company,
        run: setup.scope().run,
        viewer: WorkflowActor::authenticated(user).unwrap(),
    }
}
async fn raise(setup: &Setup, level: AccessLevel) {
    let mut tx = setup.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, setup.scope().company, setup.scope().run, level)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
async fn production_guards(setup: &Setup) {
    // The shared fault fixture disables this guard for unrelated corruption tests. Restore it here.
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(setup.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn knowledge_view_current_run_and_step_stamp_controls_payloads() {
    let setup = Box::pin(send(&[To::Channel])).await;
    production_guards(&setup).await;
    raise(&setup, AccessLevel::L5).await;
    let p = &setup.fixture.persistence;
    let scope = read_scope(&setup, setup.user);
    let view = p.run_view(scope).await.unwrap().unwrap();
    assert!(view.payload_restricted && view.input.is_null() && view.params.is_null());
    let step = p
        .step_view(scope, setup.scope().execution.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert!(step.payload_restricted && step.frozen_inputs.is_none());
    assert!(step.receipts.is_empty() && step.model_calls.is_empty());
    let html =
        crate::adapters::http::pages::run_step_body(&crate::adapters::http::pages::StepBodyView {
            company_id: scope.company.as_uuid(),
            step: &step,
            agent: None,
        });
    assert!(html.contains("Restricted run content") && html.contains(&step.step));
    if step.activated_at.is_some() {
        assert!(html.contains("started"));
    }
    assert!(!html.contains("The run finished."));
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(scope.company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    for user in [setup.user, setup.owner] {
        let view = p.run_view(read_scope(&setup, user)).await.unwrap().unwrap();
        assert!(!view.payload_restricted && !view.input.is_null());
        let step = p
            .step_view(read_scope(&setup, user), setup.scope().execution.as_uuid())
            .await
            .unwrap()
            .unwrap();
        assert!(!step.payload_restricted && step.frozen_inputs.is_some());
    }
    let foreign = RunReadScope {
        company: CompanyId::new(Uuid::new_v4()),
        ..scope
    };
    assert!(p.run_view(foreign).await.unwrap().is_none());
}

async fn wait_for_blocked_readers(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid))",
            )
            .bind(blocker)
            .fetch_one(pool)
            .await
            .unwrap();
            if count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both real readers reached the payload table lock");
}

#[tokio::test]
async fn knowledge_view_competing_readers_demotion_and_content_raise_use_final_facts() {
    let setup = Box::pin(send(&[To::Channel])).await;
    production_guards(&setup).await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(setup.scope().company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    // This fixture owns its entire database. Hold the payload table until both reads have begun.
    let mut transaction = setup.pool().begin().await.unwrap();
    sqlx::query("LOCK TABLE workflow_executions IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let low_scope = read_scope(&setup, setup.user);
    let owner_scope = read_scope(&setup, setup.owner);
    let low_store = setup.fixture.persistence.clone();
    let owner_store = setup.fixture.persistence.clone();
    let mut readers = tokio::task::JoinSet::new();
    readers.spawn(async move {
        (
            low_scope.viewer.user_id(),
            low_store.run_view(low_scope).await,
        )
    });
    readers.spawn(async move {
        (
            owner_scope.viewer.user_id(),
            owner_store.run_view(owner_scope).await,
        )
    });
    wait_for_blocked_readers(setup.pool(), blocker).await;
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(low_scope.company.as_uuid())
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    // The same transaction owns the table lock, so production guards can read it while raising.
    knowledge::raise_content_level_on(
        &mut transaction,
        low_scope.company,
        low_scope.run,
        AccessLevel::L5,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let mut completed = 0;
    while let Some(result) = readers.join_next().await {
        let (viewer, result) = result.unwrap();
        let view = result.unwrap().unwrap();
        if viewer == setup.user {
            assert!(view.payload_restricted && view.input.is_null() && view.params.is_null());
        } else {
            assert_eq!(viewer, setup.owner);
            assert!(!view.payload_restricted && !view.input.is_null());
        }
        completed += 1;
    }
    assert_eq!(completed, 2);
}

#[tokio::test]
async fn knowledge_view_refusal_attention_and_native_constraints_keep_only_historical_levels() {
    use crate::application::attention::AttentionPersistence;
    use crate::entities::attention::{AttentionQuery, AttentionSourceKind, AttentionView};
    use crate::entities::transport::PrincipalId;
    let setup = Box::pin(send(&[To::User])).await;
    production_guards(&setup).await;
    raise(&setup, AccessLevel::L5).await;
    let WorkflowHandlerOutcome::Completed(Err(report)) =
        setup.runtime().execute(&setup.kind, &setup.claim).await
    else {
        panic!("knowledge release must refuse");
    };
    let fact = report.failure.knowledge_refusal().unwrap();
    assert_eq!(fact.content, AccessLevel::L5);
    assert_eq!(fact.destination, AccessLevel::PUBLIC);
    assert!(
        setup
            .fixture
            .persistence
            .release_io(
                setup.claim.fence,
                lease(),
                LeaseReleaseCause::Classified(report)
            )
            .await
            .unwrap()
    );
    let scope = setup.scope();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(scope.company.as_uuid())
            .bind(setup.owner)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    let listed = setup
        .fixture
        .persistence
        .list_attention(AttentionQuery {
            company_id: scope.company.as_uuid(),
            principal_id: PrincipalId::new(principal),
            visible_channel_ids: &[setup.channel],
            manager: true,
            view: AttentionView::MyWork,
            cursor: None,
            limit: 10,
        })
        .await
        .unwrap();
    let item = listed
        .items
        .iter()
        .find(|item| item.source_kind == AttentionSourceKind::RunFailure)
        .unwrap();
    for expected in [
        "content level 5".to_string(),
        "destination level 0".to_string(),
        scope.run.as_uuid().to_string(),
    ] {
        assert!(item.next_action.contains(&expected), "{}", item.next_action);
    }
    assert!(!item.next_action.contains("The run finished"));
    check_refusal_constraints(&setup).await;
}

async fn check_refusal_constraints(setup: &Setup) {
    for mutation in [
        "knowledge_destination_level=NULL",
        "knowledge_content_level=NULL",
        "knowledge_destination_level=5",
        "knowledge_content_level=10",
        "workflow_failure_code='other.failure'",
        "workflow_failure_class='retryable'",
    ] {
        let mut tx = setup.pool().begin().await.unwrap();
        // LIKE copies the production constraints/domains; no duplicated constraint implementation.
        sqlx::query("CREATE TEMP TABLE refusal_constraint_probe (LIKE task_attempts INCLUDING CONSTRAINTS) ON COMMIT DROP")
            .execute(&mut *tx).await.unwrap();
        sqlx::query(
            "INSERT INTO refusal_constraint_probe SELECT * FROM task_attempts WHERE task_id=$1",
        )
        .bind(setup.scope().job.0)
        .execute(&mut *tx)
        .await
        .unwrap();
        let error = sqlx::query(&format!("UPDATE refusal_constraint_probe SET {mutation}"))
            .execute(&mut *tx)
            .await
            .expect_err("invalid safe-level fact must fail");
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        tx.rollback().await.unwrap();
    }
}
