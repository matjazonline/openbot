use super::*;

#[tokio::test]
async fn channel_reply_intent_schema_competing_claims_and_fenced_release() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let barrier = tokio::sync::Barrier::new(2);
    let claim = || async {
        barrier.wait().await;
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(64).unwrap(),
        )
        .await
        .unwrap()
    };
    let (a, b) = tokio::join!(claim(), claim());
    assert_eq!(a.len() + b.len(), 1);
    let claim = a.into_iter().chain(b).next().unwrap();
    assert_eq!(claim.attempt, 1);
    let mut stale = claim;
    stale.generation = Uuid::new_v4();
    assert!(
        !f.p.release_reply(
            stale,
            ReplyPublicationRetry::Terminal(ReplyPublicationFailure::PublicationFailed)
        )
        .await
        .unwrap()
    );
    assert!(
        f.p.release_reply(claim, ReplyPublicationRetry::Retry)
            .await
            .unwrap()
    );
    assert!(
        !f.p.release_reply(claim, ReplyPublicationRetry::Retry)
            .await
            .unwrap()
    );
    assert!(
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(64).unwrap()
        )
        .await
        .unwrap()
        .is_empty(),
        "retry must respect backoff"
    );
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_intent_schema_suppressed_is_never_claimable() {
    let f = FinalFixture::new("channel.no_reply", true).await;
    f.advance().await;
    assert_eq!(f.state().await["decision"], "suppressed");
    assert!(
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(64).unwrap()
        )
        .await
        .unwrap()
        .is_empty()
    );
    let decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_decisions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(
        decisions, 0,
        "suppression must not require message approval"
    );
    assert_decision_retained_with_run(&f).await;
    retain_closed_run(&f).await;
}

#[tokio::test]
async fn channel_reply_intent_schema_live_obligation_and_proposal_are_protected() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    assert!(sqlx::query("UPDATE channel_reply_proposals SET business_output='{}'::jsonb WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).execute(f.p.pool()).await.is_err());
    assert!(
        sqlx::query("DELETE FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(f.p.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(f.p.pool())
            .await
            .is_err()
    );
    assert_eq!(f.state().await["decision"], "pending");
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.scope.company.as_uuid())
        .execute(f.p.pool())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_reply_decisions")
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn channel_reply_intent_schema_success_without_decision_rolls_back() {
    let f = FinalFixture::new("data.map", false).await;
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_runs SET state='succeeded',terminal_execution_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(f.scope.execution.as_uuid()).execute(&mut *tx).await.unwrap();
    assert!(tx.commit().await.is_err());
    assert_ne!(f.state().await["run"], "succeeded");
    assert_eq!(f.state().await["decision"], Value::Null);
}

#[tokio::test]
async fn channel_reply_intent_schema_cross_tenant_links_and_duplicate_decisions_are_rejected() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let other = Fixture::in_database(f.s.fixture._db.clone(), "foreign-final").await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(f.p.pool())
        .await
        .unwrap();
    assert!(sqlx::query("INSERT INTO channel_reply_decisions(company_id,run_id,execution_id,intent_id,disposition,proposal_id,integrity,state) \
        SELECT $3,run_id,execution_id,intent_id,disposition,proposal_id,integrity,state FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(other.target.company.as_uuid()).execute(f.p.pool()).await.is_err());
    assert!(sqlx::query("INSERT INTO channel_reply_decisions SELECT * FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).execute(f.p.pool()).await.is_err());
    assert_eq!(f.state().await["decision"], "pending");
}

#[tokio::test]
async fn channel_reply_intent_schema_queued_root_cannot_forge_a_suppressed_decision() {
    let f = FinalFixture::new("data.map", false).await;
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO channel_reply_decisions(company_id,run_id,execution_id,intent_id,disposition,state) VALUES($1,$2,$3,$4,'suppressed','suppressed')")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(f.scope.execution.as_uuid()).bind(f.intent).execute(&mut *tx).await.unwrap();
    assert!(tx.commit().await.is_err());
    assert_eq!(f.state().await["decision"], Value::Null);
}

#[tokio::test]
async fn channel_reply_intent_schema_terminal_failure_keeps_success_and_allows_retention() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let claim =
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(1).unwrap(),
        )
        .await
        .unwrap()[0];
    assert!(
        f.p.release_reply(
            claim,
            ReplyPublicationRetry::Terminal(ReplyPublicationFailure::AuthorityRevoked)
        )
        .await
        .unwrap()
    );
    assert_eq!(f.state().await["decision"], "failed");
    assert_eq!(f.state().await["run"], "succeeded");
    assert_decision_retained_with_run(&f).await;
    retain_closed_run(&f).await;
}

async fn assert_decision_retained_with_run(f: &FinalFixture) {
    let before = f.state().await;
    let error =
        sqlx::query("DELETE FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(f.p.pool())
            .await
            .expect_err("a surviving succeeded run must retain its final decision");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    assert_eq!(f.state().await, before);
}

pub(super) async fn retain_closed_run(f: &FinalFixture) {
    let mut retention = f.p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .execute(&mut *retention)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *retention)
        .await
        .unwrap();
    retention.commit().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn channel_reply_intent_schema_expired_lease_has_one_new_owner() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let old_worker = ReplyPublicationWorker(Uuid::new_v4());
    let generation = Uuid::new_v4();
    // SQL-only claimants may use a shorter lease; the production bound remains30seconds.
    let expiry: chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE channel_reply_decisions SET state='claimed',attempts=1,last_claimed_at=clock_timestamp(),generation=$3,worker_id=$4,lease_expires_at=clock_timestamp()+interval '5 milliseconds' \
        WHERE company_id=$1 AND run_id=$2 RETURNING lease_expires_at")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(generation).bind(old_worker.0).fetch_one(f.p.pool()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let barrier = tokio::sync::Barrier::new(2);
    let claim = || async {
        barrier.wait().await;
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(1).unwrap(),
        )
        .await
        .unwrap()
    };
    let (a, b) = tokio::join!(claim(), claim());
    assert_eq!(a.len() + b.len(), 1);
    let current = a.into_iter().chain(b).next().unwrap();
    assert_eq!(current.attempt, 2);
    assert_ne!(current.generation, generation);
    let stale = ReplyPublicationClaim {
        worker: old_worker,
        generation,
        expires_at: expiry,
        attempt: 1,
        ..current
    };
    assert!(
        !f.p.release_reply(
            stale,
            ReplyPublicationRetry::Terminal(ReplyPublicationFailure::PublicationFailed)
        )
        .await
        .unwrap()
    );
    assert!(
        f.p.release_reply(
            current,
            ReplyPublicationRetry::Terminal(ReplyPublicationFailure::PublicationFailed)
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn channel_reply_intent_schema_eighth_expired_claim_is_terminal() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    for attempt in 1..=8 {
        sqlx::query("UPDATE channel_reply_decisions SET state='claimed',attempts=$3,last_claimed_at=clock_timestamp(),generation=$4,worker_id=$5,lease_expires_at=clock_timestamp()+interval '5 milliseconds' WHERE company_id=$1 AND run_id=$2")
            .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(attempt).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(f.p.pool()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    for _ in 0..2 {
        assert!(
            f.p.claim_replies(
                ReplyPublicationWorker(Uuid::new_v4()),
                ReplyPublicationPage::try_from(1).unwrap()
            )
            .await
            .unwrap()
            .is_empty()
        );
    }
    let saved: Value=sqlx::query_scalar("SELECT jsonb_build_object('state',state,'attempts',attempts,'code',diagnostic_code) FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(
        saved,
        json!({"state":"failed","attempts":8,"code":"attempts_exhausted"})
    );
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_intent_schema_claim_page_serves_another_tenant_before_a_burst() {
    for page in [1, 2] {
        let QueueTenants { burst: f, other } = Box::pin(queue_tenants()).await;
        if page == 1 {
            let first =
                f.p.claim_replies(
                    ReplyPublicationWorker(Uuid::new_v4()),
                    ReplyPublicationPage::try_from(1).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(first[0].company, f.scope.company);
            let next =
                f.p.claim_replies(
                    ReplyPublicationWorker(Uuid::new_v4()),
                    ReplyPublicationPage::try_from(1).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                next[0].company, other.scope.company,
                "a backlog cannot take every one-item page"
            );
        } else {
            let claimed =
                f.p.claim_replies(
                    ReplyPublicationWorker(Uuid::new_v4()),
                    ReplyPublicationPage::try_from(2).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(claimed.len(), 2);
            assert!(claimed.iter().any(|claim| claim.company == f.scope.company));
            assert!(
                claimed
                    .iter()
                    .any(|claim| claim.company == other.scope.company)
            );
        }
    }
}

struct QueueTenants {
    burst: FinalFixture,
    other: FinalFixture,
}
async fn queue_tenants() -> QueueTenants {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    for _ in 0..3 {
        f.s.intent(TO).await;
    }
    f.s.drain(&f.s.admissions(admission(&f.p))).await;
    let scopes: Vec<ScopeRow>=sqlx::query_as("SELECT execution.run_id,execution.id AS execution_id,job.id AS job_id FROM workflow_executions AS execution \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.completed_at IS NULL")
        .bind(f.scope.company.as_uuid()).fetch_all(f.p.pool()).await.unwrap();
    for scope in scopes {
        f.p.advance_pure(
            ActivationRequest {
                run: RunId::new(scope.run_id),
                execution: ExecutionId::new(scope.execution_id),
                job: WorkflowJobId(scope.job_id),
                ..f.scope
            },
            BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        )
        .await
        .unwrap();
    }
    let other =
        Intents::in_fixture(Fixture::in_database(f.s.fixture._db.clone(), "fair-final").await)
            .await;
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["steps"]["start"]["with"] =
        json!({"value":{"literal":{"result":42}},"output_schema":{"literal":true}});
    let other = FinalFixture::configured(other, source, literal_contract("Done"), false).await;
    other.advance().await;
    QueueTenants { burst: f, other }
}
