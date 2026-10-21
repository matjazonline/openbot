//! Protected HTTP and registered tools use real publication, admission and dispatch.
use super::super::approval_test_support::notices;
use super::*;
use crate::adapters::workflow_http::{HttpProvider, NoLocal};
use crate::application::workflow::decisions::*;

fn protected(origin: &str, entrance: Entrance) -> impl std::future::Future<Output = Setup> {
    protected_reviewers(origin, entrance, 1)
}

fn protected_reviewers(
    origin: &str,
    entrance: Entrance,
    count: usize,
) -> impl std::future::Future<Output = Setup> + '_ {
    protected_customized(origin, entrance, count, |_| {})
}

fn protected_customized<'a>(
    origin: &'a str,
    entrance: Entrance,
    count: usize,
    customize: impl FnOnce(&mut Value) + 'a,
) -> impl std::future::Future<Output = Setup> + 'a {
    protected_with_operation(origin, entrance, count, customize, None)
}

async fn protected_with_operation(
    origin: &str,
    entrance: Entrance,
    count: usize,
    customize: impl FnOnce(&mut Value),
    additional: Option<TypeName>,
) -> Setup {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(&fixture, origin, entrance)).await;
    let name = if matches!(entrance, Entrance::Tool) {
        "registered.request"
    } else {
        "http.request"
    };
    let mut op = operation(fixture.target.company, name);
    op.approval_required = true;
    let mut connection = fixture
        .persistence
        .register_http_operation(
            fixture.target.company,
            fixture.target.actor,
            connection.id,
            connection.revision,
            op,
        )
        .await
        .unwrap();
    if let Some(name) = &additional {
        let mut extra = operation(fixture.target.company, name.as_str());
        extra.approval_required = true;
        connection = fixture
            .persistence
            .register_http_operation(
                fixture.target.company,
                fixture.target.actor,
                connection.id,
                connection.revision,
                extra,
            )
            .await
            .unwrap();
    }
    let mut reviewers = vec![json!({"kind":"user","id":fixture.target.actor.user_id()})];
    for _ in 1..count {
        let id = super::super::message_tests::support::member(&fixture, "admin").await;
        reviewers.push(json!({"kind":"user","id":id}));
    }
    let approval = json!({
        "reviewers":reviewers,
        "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()
    });
    let version = Box::pin(published_with_operation(
        &fixture,
        &connection,
        entrance,
        Some(approval),
        customize,
        additional,
    ))
    .await;
    let target = Box::pin(bound(&fixture, &connection, version, entrance)).await;
    let claim = Box::pin(claimed(&fixture, target)).await;
    Setup {
        fixture,
        connection,
        version,
        claim,
        kind: WorkflowStepKind(
            if matches!(entrance, Entrance::Tool) {
                "tool.call"
            } else {
                "http.request"
            }
            .into(),
        ),
    }
}

fn actions(
    setup: &Setup,
) -> HttpActions<
    PostgresPersistence,
    action_dispatch::PostgresActionDispatch<http::HttpActionAuthority, NoLocal>,
    HttpProvider,
> {
    let p = setup.fixture.persistence.clone();
    HttpActions::new(
        p.clone(),
        action_dispatch::PostgresActionDispatch::new(p.clone(), http::HttpActionAuthority, NoLocal)
            .with_decision_notices(notices(&p)),
        HttpProvider {
            persistence: p,
            policy: HttpDestinationPolicy::new([setup.connection.origin.as_str().to_owned()])
                .unwrap(),
        },
        lease(),
    )
}

async fn view(setup: &Setup) -> DecisionView {
    let p = &setup.fixture.persistence;
    let id = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE execution_id=$1")
        .bind(setup.claim.fence.scope.execution.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    p.read_decision(
        setup.fixture.target.company,
        setup.fixture.target.actor,
        DecisionId(id),
    )
    .await
    .unwrap()
}

fn submission(setup: &Setup, view: &DecisionView, choice: &str) -> SubmitDecision {
    SubmitDecision {
        company: setup.fixture.target.company,
        actor: setup.fixture.target.actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse("approval-command").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest.clone(),
        choice: choice.into(),
        feedback: String::new(),
        data: json!({}),
    }
}

async fn parked(setup: &Setup) -> crate::domain::workflow::WaitId {
    let outcome = Box::pin(actions(setup).run(&setup.kind, &setup.claim)).await;
    match outcome {
        WorkflowHandlerOutcome::Suspended(wait) => wait.wait,
        WorkflowHandlerOutcome::Completed(result) => {
            panic!("expected approval suspension: {result:?}")
        }
    }
}

#[tokio::test]
async fn workflow_action_approval_dispatch_claims_next_attempt_and_dispatches_once() {
    for entrance in [Entrance::Http, Entrance::Tool] {
        let (origin, server) =
            server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
        let setup = Box::pin(protected(&origin, entrance)).await;
        let p = &setup.fixture.persistence;
        let wait = parked(&setup).await;
        assert!(!server.is_finished(), "no request before approval");
        assert_eq!(
            action_counts(&setup.fixture).await,
            ActionCounts {
                dispatches: 0,
                entries: 0,
                receipts: 0
            }
        );
        let count: i32 = sqlx::query_scalar("SELECT retry_count FROM background_tasks WHERE id=$1")
            .bind(setup.claim.fence.scope.job.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let decision = view(&setup).await;
        let command = submission(&setup, &decision, "approve");
        assert_eq!(
            p.submit_decision(command.clone())
                .await
                .unwrap()
                .disposition,
            DecisionDisposition::Applied
        );
        assert_eq!(
            p.submit_decision(command).await.unwrap().disposition,
            DecisionDisposition::Replayed
        );
        assert!(p.confirm_suspension(setup.claim.fence, wait).await.unwrap());
        let next = p
            .claim_io(
                setup.claim.fence.scope,
                WorkflowWorkerId(Uuid::new_v4()),
                lease(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.fence.attempt.0, setup.claim.fence.attempt.0 + 1);
        let runtime = actions(&setup);
        let (a, b) = tokio::join!(
            runtime.run(&setup.kind, &next),
            runtime.run(&setup.kind, &next)
        );
        let (a, b) = (a.completed_result(), b.completed_result());
        assert!(a.is_ok() || b.is_ok(), "first={a:?}, second={b:?}");
        server.await.unwrap();
        assert_eq!(
            action_counts(&setup.fixture).await,
            ActionCounts {
                dispatches: 1,
                entries: 1,
                receipts: 1
            }
        );
    }
}

#[tokio::test]
async fn workflow_action_approval_reject_registered_operation_has_http_failure() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Tool)).await;
    parked(&setup).await;
    let decision = view(&setup).await;
    let p = &setup.fixture.persistence;
    p.submit_decision(submission(&setup, &decision, "reject"))
        .await
        .unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(setup.claim.fence.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "failed");
    let code: String = sqlx::query_scalar("SELECT event_kind FROM workflow_run_events WHERE run_id=$1 AND event_kind='http.approval_rejected'")
        .bind(setup.claim.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(code, "http.approval_rejected");
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn workflow_action_approval_park_with_sixteen_approvers_commits_within_bound() {
    let setup = Box::pin(protected_reviewers(
        "http://127.0.0.1:1",
        Entrance::Http,
        16,
    ))
    .await;
    let started = std::time::Instant::now();
    parked(&setup).await;
    assert!(started.elapsed() < lease().attempt_bound());
    assert_eq!(view(&setup).await.reviewers.len(), 16);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM message_deliveries")
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 16);
}

#[tokio::test]
async fn workflow_action_approval_last_failure_attempt_can_park_and_resume() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE background_tasks SET max_retries=retry_count+1 WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    parked(&setup).await;
    let decision = view(&setup).await;
    p.submit_decision(submission(&setup, &decision, "approve"))
        .await
        .unwrap();
    let counters: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('failures',retry_count,'continuations',continuation_count,'attempt',attempt_ordinal) FROM background_tasks WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        counters,
        json!({"failures":0,"continuations":1,"attempt":2})
    );
    let next = p
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.fence.attempt.0, 2);
}

#[tokio::test]
async fn workflow_action_approval_reopen_without_submission_is_rejected() {
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    parked(&setup).await;
    let p = &setup.fixture.persistence;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE id=$1")
        .bind(setup.claim.fence.scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE background_tasks SET status='pending',continuation_count=continuation_count+1 WHERE id=$1",
    )
    .bind(setup.claim.fence.scope.job.0)
    .execute(&mut *tx)
    .await
    .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(
        error.as_database_error().unwrap().message(),
        "workflow job reopen requires an approval or delivery continuation"
    );
    assert_eq!(view(&setup).await.state, "waiting");
}

#[tokio::test]
async fn workflow_action_approval_expiry_fails_without_dispatch() {
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    parked(&setup).await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1")
        .bind(setup.claim.fence.scope.run.as_uuid()).execute(p.pool()).await.unwrap();
    assert!(matches!(
        p.resume_wait(setup.claim.fence.scope).await.unwrap(),
        WaitProgress::Expired(_)
    ));
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(setup.claim.fence.scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "failed");
    let code: String = sqlx::query_scalar("SELECT event_kind FROM workflow_run_events WHERE run_id=$1 AND event_kind='http.approval_expired'")
        .bind(setup.claim.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(code, "http.approval_expired");
    assert_eq!(view(&setup).await.state, "expired");
}

#[tokio::test]
async fn workflow_action_approval_admin_demotion_refuses_park_and_submission() {
    for phase in ["park", "submit"] {
        let setup = Box::pin(protected_reviewers("http://127.0.0.1:1", Entrance::Http, 2)).await;
        let p = &setup.fixture.persistence;
        let admin: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM company_members WHERE company_id=$1 AND user_id<>$2",
        )
        .bind(setup.fixture.target.company.as_uuid())
        .bind(setup.fixture.target.actor.user_id())
        .fetch_one(p.pool())
        .await
        .unwrap();
        let mut command = if phase == "submit" {
            parked(&setup).await;
            Some(submission(&setup, &view(&setup).await, "approve"))
        } else {
            None
        };
        sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
            .bind(setup.fixture.target.company.as_uuid())
            .bind(admin)
            .execute(p.pool())
            .await
            .unwrap();
        if let Some(ref mut command) = command {
            command.actor = WorkflowActor::authenticated(admin).unwrap();
            assert!(matches!(
                p.submit_decision(command.clone()).await,
                Err(AppError::NotFound(_))
            ));
            assert_eq!(view(&setup).await.state, "waiting");
        } else {
            let failure = Box::pin(actions(&setup).run(&setup.kind, &setup.claim))
                .await
                .completed_result()
                .unwrap_err();
            assert_eq!(failure.failure.code().as_str(), "http.approval_required");
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
                .fetch_one(p.pool())
                .await
                .unwrap();
            assert_eq!(count, 0);
        }
    }
}

#[tokio::test]
async fn workflow_action_approval_competing_reviewers_settle_once() {
    let setup = Box::pin(protected_reviewers("http://127.0.0.1:1", Entrance::Http, 2)).await;
    parked(&setup).await;
    let decision = view(&setup).await;
    let p = &setup.fixture.persistence;
    let approve = submission(&setup, &decision, "approve");
    let mut reject = submission(&setup, &decision, "reject");
    reject.actor = WorkflowActor::authenticated(
        *decision
            .reviewers
            .iter()
            .find(|id| **id != setup.fixture.target.actor.user_id())
            .unwrap(),
    )
    .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            p.submit_decision(approve).await
        },
        async {
            barrier.wait().await;
            p.submit_decision(reject).await
        }
    );
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "{a:?} {b:?}"
    );
    let after = view(&setup).await;
    assert_eq!(after.revision, decision.revision + 1);
    assert_eq!(after.state, "completed");
}

#[tokio::test]
async fn workflow_action_approval_submit_competes_with_expiry() {
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
    parked(&setup).await;
    let decision = view(&setup).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let barrier = tokio::sync::Barrier::new(2);
    let submit = async {
        barrier.wait().await;
        p.submit_decision(submission(&setup, &decision, "approve"))
            .await
    };
    let expire = async {
        barrier.wait().await;
        let mut tx = p.pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'")
            .bind(scope.run.as_uuid()).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        p.resume_wait(scope).await.unwrap()
    };
    let (submitted, expired) = tokio::join!(submit, expire);
    let state: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('run',run.state,'job',job.status,'retries',job.retry_count,'wait',wait.state,'choice',decision.choice) FROM workflow_runs AS run JOIN workflow_waits AS wait ON wait.run_id=run.id JOIN background_tasks AS job ON job.workflow_execution_id=wait.execution_id JOIN workflow_decisions AS decision ON decision.wait_id=wait.id WHERE run.id=$1")
        .bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    if submitted.is_ok() {
        assert!(matches!(expired, WaitProgress::Refused));
        assert_eq!(
            state,
            json!({"run":"running","job":"pending","retries":0,"wait":"completed","choice":"approve"})
        );
    } else {
        assert!(matches!(submitted, Err(AppError::Conflict(_))));
        assert!(matches!(expired, WaitProgress::Expired(_)));
        assert_eq!(state["run"], "failed");
        assert_ne!(state["job"], "pending");
        assert_eq!(state["wait"], "expired");
        assert!(state["choice"].is_null());
    }
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[path = "action_approval_resolution_tests.rs"]
mod resolution_tests;

#[path = "action_approval_sql_tests.rs"]
mod sql_tests;

#[path = "action_approval_continuation_tests.rs"]
mod continuation_tests;

#[path = "action_approval_review_tests.rs"]
mod review_tests;
