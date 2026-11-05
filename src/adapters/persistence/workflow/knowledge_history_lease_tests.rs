//! Real claim replacement and statement/COMMIT expiry at the new history mutation seam.
use super::*;
use crate::application::workflow::{history::WorkflowHistory, lease::*};
use std::time::Duration;

struct OwnedHistory {
    fixture: AdmissionFixture,
    claim: ClaimedWorkflow,
}
async fn setup() -> OwnedHistory {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 65_536)).await)
            .await;
    let c = Conversation::new(&f).await;
    sqlx::query(
        "INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role)
        SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(c.channel)
    .bind(c.thread)
    .bind(f.binding.target.actor.user_id())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    super::super::knowledge_admission_tests::stamped(
        &f,
        &c,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await;
    let claim = claimed(&f, &c, "history-owned").await;
    OwnedHistory { fixture: f, claim }
}
async fn snapshot(f: &OwnedHistory) -> Value {
    let scope = f.claim.fence.scope;
    sqlx::query_scalar("SELECT jsonb_build_object('content',run.content_level,'revision',run.revision,
        'proofs',(SELECT COALESCE(jsonb_agg(to_jsonb(proof) ORDER BY proof.attempt_number),'[]'::jsonb)
            FROM workflow_history_consumptions AS proof WHERE proof.company_id=run.company_id AND proof.run_id=run.id))
        FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.fixture.persistence().pool()).await.unwrap()
}
async fn shorten(f: &OwnedHistory, boundary: &str, duration: &str) {
    let statement = match boundary {
        "lease" => {
            "UPDATE background_tasks SET lock_expires_at=clock_timestamp()+$2::interval WHERE id=$1"
        }
        "deadline" => {
            "UPDATE workflow_runs SET deadline=clock_timestamp()+$2::interval WHERE id=$1"
        }
        _ => unreachable!(),
    };
    let id = if boundary == "lease" {
        f.claim.fence.scope.job.0
    } else {
        f.claim.fence.scope.run.as_uuid()
    };
    sqlx::query(statement)
        .bind(id)
        .bind(duration)
        .execute(f.fixture.persistence().pool())
        .await
        .unwrap();
}

async fn competing_read(
    f: &OwnedHistory,
    current: &ClaimedWorkflow,
) -> crate::application::workflow::history::FrozenHistory {
    let barrier = tokio::sync::Barrier::new(2);
    let p = f.fixture.persistence();
    let (old, new) = tokio::join!(
        async {
            barrier.wait().await;
            p.frozen_history(&f.claim).await
        },
        async {
            barrier.wait().await;
            p.frozen_history(current).await
        },
    );
    assert!(matches!(
        old,
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost
        ))
    ));
    new.unwrap()
}

#[tokio::test]
async fn knowledge_run_level_history_real_replacement_claim_fences_old_reader_and_replays_once() {
    let f = Box::pin(setup()).await;
    let old = f.claim.clone();
    let p = f.fixture.persistence();
    let policy = LeasePolicy::new(Duration::from_secs(30)).unwrap();
    assert!(
        p.release_io(old.fence, policy, LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(old.fence.scope.job.0)
    .execute(p.pool())
    .await
    .unwrap();
    let current = p
        .claim_io(old.fence.scope, WorkflowWorkerId(Uuid::new_v4()), policy)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(old.fence.generation, current.fence.generation);
    assert!(
        tokio::time::Instant::now() < old.window.expires,
        "stale cached lease has not elapsed"
    );
    assert_eq!(old.activation, current.activation);
    let before = snapshot(&f).await;
    assert!(matches!(
        p.frozen_history(&old).await,
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost
        ))
    ));
    assert_eq!(
        snapshot(&f).await,
        before,
        "stale caller leaves no durable rise or revision"
    );
    let history = competing_read(&f, &current).await;
    assert_eq!(history.newest.len(), 2);
    let consumed = snapshot(&f).await;
    assert_eq!(consumed["content"], 5);
    assert_eq!(consumed["proofs"].as_array().unwrap().len(), 1);
    assert_eq!(
        consumed["proofs"][0]["execution_generation"],
        json!(current.fence.generation.0)
    );
    p.frozen_history(&current).await.unwrap();
    assert_eq!(
        snapshot(&f).await,
        consumed,
        "same live claim is idempotent"
    );
}

#[tokio::test]
async fn knowledge_run_level_history_expired_database_lease_or_deadline_returns_no_output_or_rise()
{
    for boundary in ["lease", "deadline"] {
        let f = Box::pin(setup()).await;
        shorten(&f, boundary, "-1 millisecond").await;
        let before = snapshot(&f).await;
        assert!(
            f.fixture
                .persistence()
                .frozen_history(&f.claim)
                .await
                .is_err(),
            "{boundary}"
        );
        assert_eq!(snapshot(&f).await, before, "{boundary}");
    }
}

async fn pause(f: &OwnedHistory, phase: &str) {
    let pool = f.fixture.persistence().pool();
    sqlx::raw_sql("CREATE SEQUENCE history_pause_entered; CREATE FUNCTION history_pause() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM nextval('history_pause_entered'); PERFORM pg_sleep(0.6); RETURN NEW; END $$")
        .execute(pool).await.unwrap();
    let trigger = match phase {
        "statement" => "CREATE TRIGGER history_pause BEFORE UPDATE OF content_level ON workflow_runs
            FOR EACH ROW WHEN (NEW.content_level>OLD.content_level) EXECUTE FUNCTION history_pause()",
        "commit" => "CREATE CONSTRAINT TRIGGER aa_history_pause AFTER INSERT ON workflow_history_consumptions
            DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history_pause()",
        _ => unreachable!(),
    };
    sqlx::raw_sql(trigger).execute(pool).await.unwrap();
}

#[tokio::test]
async fn knowledge_run_level_history_expiry_during_statement_and_commit_rolls_back_taint_and_proof()
{
    for phase in ["statement", "commit"] {
        for boundary in ["lease", "deadline"] {
            let f = Box::pin(setup()).await;
            pause(&f, phase).await;
            shorten(&f, boundary, "400 milliseconds").await;
            let before = snapshot(&f).await;
            let Err(error) = f.fixture.persistence().frozen_history(&f.claim).await else {
                panic!("expired history must not return content")
            };
            let entered: bool = sqlx::query_scalar("SELECT is_called FROM history_pause_entered")
                .fetch_one(f.fixture.persistence().pool())
                .await
                .unwrap();
            assert!(
                entered,
                "{phase}/{boundary}: actual transaction reached pause"
            );
            if phase == "commit" {
                assert!(matches!(error, AppError::WorkflowGuard(_)), "{error:?}");
                assert!(
                    error
                        .to_string()
                        .contains("workflow_history_consumption_live"),
                    "{error:?}"
                );
            }
            assert_eq!(
                snapshot(&f).await,
                before,
                "{phase}/{boundary}: entire consumption rolled back"
            );
        }
    }
}

#[tokio::test]
async fn knowledge_run_level_history_consumption_rejects_foreign_scope_and_rewritten_proof() {
    let f = Box::pin(setup()).await;
    let p = f.fixture.persistence();
    p.frozen_history(&f.claim).await.unwrap();
    let before = snapshot(&f).await;
    for statement in [
        "UPDATE workflow_history_consumptions SET content_level=6 WHERE job_id=$1",
        "DELETE FROM workflow_history_consumptions WHERE job_id=$1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(f.claim.fence.scope.job.0)
                .execute(p.pool())
                .await
                .is_err()
        );
        assert_eq!(snapshot(&f).await, before);
    }
    let foreign = AdmissionFixture::new().await;
    let error = sqlx::query("INSERT INTO workflow_history_consumptions SELECT $2,run_id,execution_id,job_id,attempt_number,
        execution_generation,worker_id,content_level FROM workflow_history_consumptions WHERE job_id=$1")
        .bind(f.claim.fence.scope.job.0).bind(foreign.binding.target.company.as_uuid())
        .execute(p.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(
        snapshot(&f).await,
        before,
        "valid foreign company cannot own this execution"
    );
}
