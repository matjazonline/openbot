use super::*;
use crate::application::workflow::{batch::WorkflowBatch, completion::*, polling::*, waits::*};

#[tokio::test]
async fn workflow_poison_io_activation_and_wrong_kind_park() {
    let (f, scope) = fixture().await;
    let before = snapshot(&f).await;
    assert!(f.persistence().park_wait(scope).await.unwrap().is_none());
    assert_eq!(snapshot(&f).await, before);
    sqlx::query("UPDATE workflow_runs SET input='{}'::jsonb WHERE id=$1")
        .bind(scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "failed");
    assert_eq!(saved["executions"][0]["frozen_inputs"], Value::Null);
    assert_eq!(saved["attempts"], Value::Null);
    assert!(
        f.persistence()
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_poison_effect_output_and_spent_pending_reconcile() {
    for spent in [false, true] {
        let source: Value =
            serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
        let (f, scope) = fixture_source(source).await;
        if spent {
            sqlx::query("UPDATE background_tasks SET retry_count=max_retries WHERE id=$1")
                .bind(scope.job.0)
                .execute(f.persistence().pool())
                .await
                .unwrap();
            assert!(
                f.persistence()
                    .retire_exhausted_work(scope, policy())
                    .await
                    .unwrap()
            );
        } else {
            let owned = f
                .persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .unwrap();
            assert!(
                f.persistence()
                    .complete_io(FencedWorkflowResult {
                        fence: owned.fence,
                        output: Value::Null
                    })
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let saved = snapshot(&f).await;
        assert_eq!(saved["runs"][0]["state"], "waiting");
        assert_eq!(saved["runs"][0]["waiting_reason"], "reconciliation");
        assert_eq!(saved["jobs"][0]["status"], "failed");
        assert!(
            f.persistence()
                .poll_work(None, 128)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(snapshot(&f).await, saved);
    }
}

#[tokio::test]
async fn workflow_poison_wait_deadlines_and_invalid_input_settle() {
    for kind in ["wait.event", "wait.timer"] {
        let mut source: Value =
            serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(10)).to_rfc3339()});
        let (f, scope) = fixture_source(source).await;
        let parked = f.persistence().park_wait(scope).await.unwrap();
        if kind == "wait.timer" {
            assert!(parked.is_some());
            f.persistence().resume_wait(scope).await.unwrap();
        } else {
            assert!(parked.is_none());
            assert_eq!(snapshot(&f).await["runs"][0]["state"], "failed");
        }
        for _ in 0..2 {
            assert!(
                f.persistence()
                    .poll_work(None, 128)
                    .await
                    .unwrap()
                    .candidates
                    .is_empty()
            );
        }
    }
}

fn error_routed_wait() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("wait.event").unwrap().source).unwrap();
    source["input_schema"] =
        json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]});
    source["steps"]["start"]["with"]["deadline"] = json!({"ref":"/input/value"});
    source["steps"]["start"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"object":{"fallback":{"default":{"ref":"/steps/start/output","value":{"literal":"missing"}}},"present":{"exists":"/steps/start/output"}}},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
    source
}

async fn wait_with_deadline(deadline: &str) -> (AdmissionFixture, ActivationRequest) {
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(error_routed_wait()).await)
        .await;
    let mut request = f.request("invalid-wait", f.manual());
    request.input = json!({"value":deadline});
    let command = f.prepare(request).await;
    f.persistence().admit(&command).await.unwrap();
    let scope = f.persistence().poll_work(None, 1).await.unwrap().candidates[0].scope;
    (f, scope)
}

#[tokio::test]
async fn workflow_poison_invalid_wait_input_terminates_before_activation() {
    let (f, scope) = wait_with_deadline("not-an-rfc3339-date").await;
    assert!(f.persistence().park_wait(scope).await.unwrap().is_none());
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "failed");
    assert_eq!(saved["executions"][0]["frozen_inputs"], Value::Null);
    assert_eq!(saved["executions"][0]["activated_at"], Value::Null);
    assert_eq!(saved["executions"].as_array().unwrap().len(), 1);
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 1)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
        assert!(f.persistence().park_wait(scope).await.unwrap().is_none());
    }
    assert_eq!(snapshot(&f).await, saved);
}

#[tokio::test]
async fn workflow_poison_activated_expired_wait_takes_final_error() {
    let deadline = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
    let (f, scope) = wait_with_deadline(&deadline).await;
    assert!(f.persistence().park_wait(scope).await.unwrap().is_none());
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "running");
    let failed = saved["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == scope.execution.as_uuid().to_string())
        .unwrap();
    assert_eq!(failed["committed_route"], "final_error");
    assert_eq!(failed["frozen_inputs"]["deadline"], deadline);
    assert!(!failed["activated_at"].is_null());
    let next = f.persistence().poll_work(None, 1).await.unwrap().candidates[0].scope;
    assert_ne!(next.execution, scope.execution);
    f.persistence()
        .advance_pure(
            next,
            crate::application::workflow::batch::BatchBudget::new(16, Duration::from_secs(1))
                .unwrap(),
        )
        .await
        .unwrap();
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["state"], "succeeded");
    let recovered = saved["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == next.execution.as_uuid().to_string())
        .unwrap();
    assert_eq!(
        recovered["committed_output"],
        json!({"fallback":"missing","present":false})
    );
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 1)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    assert_eq!(snapshot(&f).await, saved);
}
