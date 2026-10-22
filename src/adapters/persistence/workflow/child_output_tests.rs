//! Invalid child output (07-D1 §D1.4, §D1.9): a child result that the parent cannot accept fails
//! the parent's wait as `invalid_output`; the run fails unrouted, never with a fabricated
//! success, and a replay answers the same way.
use super::*;
use crate::application::workflow::completion::CommitDisposition;
use crate::application::workflow::waits::*;

fn data_map() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

/// The parent's run and child wait after a resume: `(run state, wait state, failure code)`.
async fn settled(
    f: &AdmissionFixture,
    parent: ActivationRequest,
) -> (String, String, Option<String>) {
    sqlx::query_as("SELECT run.state, wait.state, wait.failure_code FROM workflow_runs AS run JOIN workflow_waits AS wait ON wait.company_id=run.company_id AND wait.run_id=run.id WHERE run.id=$1 AND wait.execution_id=$2 AND wait.reason='child_run'")
        .bind(parent.run.as_uuid())
        .bind(parent.execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn fails_as_invalid_output(f: &AdmissionFixture, parent: ActivationRequest) {
    let p = f.persistence();
    assert_eq!(
        p.resume_wait(parent).await.unwrap(),
        WaitProgress::Failed {
            disposition: CommitDisposition::Committed,
            reason: WaitFailureReason::InvalidOutput,
        }
    );
    let expected = (
        "failed".to_owned(),
        "failed".to_owned(),
        Some("invalid_output".to_owned()),
    );
    assert_eq!(settled(f, parent).await, expected);
    assert_eq!(
        p.resume_wait(parent).await.unwrap(),
        WaitProgress::Failed {
            disposition: CommitDisposition::Replayed,
            reason: WaitFailureReason::InvalidOutput,
        }
    );
    assert_eq!(settled(f, parent).await, expected);
    let successors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1 AND id<>$2")
            .bind(parent.run.as_uuid())
            .bind(parent.execution.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(successors, 0);
}

#[tokio::test]
async fn workflow_call_child_output_the_parent_cannot_complete_with_fails_the_wait() {
    // The child's string result reaches the parent's `$end`, whose output must be an integer.
    let (f, parent) = AdmissionFixture::with_child(data_map(), |child, version| {
        let mut source = child_support::call_source(child, version);
        source["output_schema"] = json!({"type": "integer"});
        source
    })
    .await;
    let child = f.spawn(parent).await;
    f.persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    fails_as_invalid_output(&f, parent).await;
}

#[tokio::test]
async fn workflow_call_stored_child_output_outside_the_pinned_schema_fails_the_wait() {
    // The pinned child promises a short string. Its stored result is then corrupted outside the
    // runtime (triggers bypassed for this one raw update), as only a defect or a manual edit
    // could do; the parent re-checks it against the pinned schema rather than trusting it.
    let mut leaf = data_map();
    leaf["output_schema"] = json!({"type": "string", "maxLength": 8});
    let (f, parent) = AdmissionFixture::with_child(leaf, child_support::call_source).await;
    let child = f.spawn(parent).await;
    let p = f.persistence();
    p.advance_pure(child, budget(4)).await.unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    let corrupted = sqlx::query("UPDATE workflow_executions SET committed_output='\"far more than eight\"'::jsonb WHERE run_id=$1 AND completed_at IS NOT NULL")
        .bind(child.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(corrupted.rows_affected(), 1);
    tx.commit().await.unwrap();
    fails_as_invalid_output(&f, parent).await;
}
