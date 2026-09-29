use super::*;
use crate::application::workflow::{batch::*, completion::CommitDisposition, waits::*};
fn source(timer: bool) -> Value {
    let name = if timer { "wait.timer" } else { "wait.event" };
    let mut source: Value = serde_json::from_str(&registry::example(name).unwrap().source).unwrap();
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    if !timer {
        source["steps"]["start"]["with"]["payload_schema"] = json!({"literal":{"type":"object","properties":{"text":{"type":"string","maxLength":128}},"required":["text"],"additionalProperties":false}});
    }
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    source
}
fn signal(scope: ActivationRequest) -> WorkflowSignal {
    WorkflowSignal {
        scope,
        id: WorkflowEventId(Uuid::new_v4()),
        name: WorkflowEventName("review.completed".into()),
        correlation: WorkflowEventCorrelation("request-1".into()),
        payload: json!({"text":"accepted"}),
    }
}
async fn snapshot_all(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('base',$1::jsonb,'waits',(SELECT jsonb_agg(to_jsonb(wait)) FROM workflow_waits AS wait),'signals',(SELECT jsonb_agg(to_jsonb(event)) FROM workflow_wait_events AS event),'audit',(SELECT jsonb_agg(to_jsonb(event)) FROM workflow_run_events AS event))")
        .bind(snapshot(f).await).fetch_one(f.persistence().pool()).await.unwrap()
}
fn completed(
    progress: WaitProgress,
) -> crate::application::workflow::completion::CommittedWorkflowStep {
    let WaitProgress::Completed(saved) = progress else {
        panic!("expected completion: {progress:?}")
    };
    *saved
}
#[tokio::test]
async fn workflow_wait_competing_park_signal_resume_and_lost_ack() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    let barrier = Barrier::new(2);
    let park = || async {
        barrier.wait().await;
        p.park_wait(scope).await.unwrap().unwrap()
    };
    let (a, b) = tokio::join!(park(), park());
    assert_eq!(a, b);
    let state = snapshot_all(&f).await;
    assert_eq!(state["base"]["jobs"][0]["status"], "completed");
    assert_eq!(state["base"]["runs"][0]["state"], "waiting");
    assert_eq!(
        state["waits"][0]["notification_intent"]["wait_id"],
        a.wait.as_uuid().to_string()
    );
    assert!(
        p.claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Pending);
    let event = signal(scope);
    let record = || async {
        barrier.wait().await;
        p.record_signal(event.clone()).await.unwrap()
    };
    let (a, b) = tokio::join!(record(), record());
    assert!(a && b);
    let resume = || async {
        barrier.wait().await;
        completed(p.resume_wait(scope).await.unwrap())
    };
    let (a, b) = tokio::join!(resume(), resume());
    assert_eq!(a.successor, b.successor);
    assert_ne!(a.disposition, b.disposition);
    let next = a.successor.unwrap();
    assert_eq!(
        p.advance_pure(
            next,
            BatchBudget::new(4, Duration::from_millis(1000)).unwrap()
        )
        .await
        .unwrap()
        .completed,
        1
    );
    let replay = completed(p.resume_wait(scope).await.unwrap());
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.successor, Some(next));
    let state = snapshot_all(&f).await;
    assert_eq!(state["waits"].as_array().unwrap().len(), 1);
    assert_eq!(state["signals"].as_array().unwrap().len(), 1);
    assert_eq!(state["base"]["executions"].as_array().unwrap().len(), 2);
    assert_eq!(
        state["waits"][0]["consumed_event_id"],
        event.id.0.to_string()
    );
    let mut conflicting = event;
    conflicting.payload = json!({"text":"changed"});
    assert!(!p.record_signal(conflicting).await.unwrap());
}
#[tokio::test]
async fn workflow_wait_event_before_park_and_concurrent_arrival() {
    for before in [true, false] {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        let event = signal(scope);
        if before {
            assert!(p.record_signal(event.clone()).await.unwrap());
            p.park_wait(scope).await.unwrap().unwrap();
        } else {
            let barrier = Barrier::new(2);
            let park = async {
                barrier.wait().await;
                p.park_wait(scope).await.unwrap().unwrap()
            };
            let record = async {
                barrier.wait().await;
                p.record_signal(event).await.unwrap()
            };
            let (_, saved) = tokio::join!(park, record);
            assert!(saved);
        }
        let restarted = PostgresPersistence::new(p.pool().clone());
        completed(restarted.resume_wait(scope).await.unwrap());
    }
}
#[tokio::test]
async fn workflow_wait_refuses_scope_kind_and_invalid_events() {
    let (f, scope) = fixture_source(source(false)).await;
    let p = f.persistence();
    let before = snapshot_all(&f).await;
    for part in ["name", "correlation", "company", "run", "execution", "job"] {
        let mut event = signal(scope);
        match part {
            "name" => event.name.0 = "other".into(),
            "correlation" => event.correlation.0 = "other".into(),
            "company" => event.scope.company = CompanyId::new(Uuid::new_v4()),
            "run" => event.scope.run = RunId::new(Uuid::new_v4()),
            "execution" => event.scope.execution = ExecutionId::new(Uuid::new_v4()),
            _ => event.scope.job = WorkflowJobId(Uuid::new_v4()),
        };
        assert!(!p.record_signal(event).await.unwrap());
        assert_eq!(snapshot_all(&f).await, before);
    }
    for payload in [json!(12), json!({"text":"x".repeat(100000)})] {
        let mut event = signal(scope);
        event.payload = payload;
        assert!(p.record_signal(event).await.is_err());
        assert_eq!(snapshot_all(&f).await, before);
    }
    for kind in ["data.map", "decision.human", "context.load"] {
        let source = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        let (f, scope) = fixture_source(source).await;
        let before = snapshot_all(&f).await;
        assert!(f.persistence().park_wait(scope).await.unwrap().is_none());
        assert_eq!(snapshot_all(&f).await, before);
    }
}
#[tokio::test]
async fn workflow_wait_park_and_resume_write_failures_roll_back() {
    for (table, condition, park) in [
        ("workflow_waits", "TG_OP='INSERT'", true),
        ("background_tasks", "true", true),
        ("workflow_runs", "NEW.state='waiting'", true),
        ("workflow_run_events", "true", true),
        ("workflow_waits", "NEW.state='completed'", false),
        ("workflow_executions", "NEW.completed_at IS NOT NULL", false),
        ("workflow_executions", "TG_OP='INSERT'", false),
        ("background_tasks", "TG_OP='INSERT'", false),
        ("workflow_runs", "NEW.state='running'", false),
        (
            "workflow_run_events",
            "NEW.event_kind='wait_completed'",
            false,
        ),
    ] {
        let (f, scope) = fixture_source(source(false)).await;
        let p = f.persistence();
        if !park {
            p.park_wait(scope).await.unwrap().unwrap();
            assert!(p.record_signal(signal(scope)).await.unwrap());
        }
        let before = snapshot_all(&f).await;
        sqlx::raw_sql(&format!("CREATE FUNCTION wait_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF {condition} THEN RAISE EXCEPTION 'injected'; END IF; RETURN NEW; END $$; CREATE TRIGGER wait_fault AFTER INSERT OR UPDATE ON {table} FOR EACH ROW EXECUTE FUNCTION wait_fault();")).execute(p.pool()).await.unwrap();
        if park {
            assert!(p.park_wait(scope).await.is_err())
        } else {
            assert!(p.resume_wait(scope).await.is_err())
        }
        assert_eq!(snapshot_all(&f).await, before);
        sqlx::raw_sql(&format!(
            "DROP TRIGGER wait_fault ON {table}; DROP FUNCTION wait_fault();"
        ))
        .execute(p.pool())
        .await
        .unwrap();
        if park {
            p.park_wait(scope).await.unwrap().unwrap();
        } else {
            completed(p.resume_wait(scope).await.unwrap());
        }
    }
}

#[path = "wait_deadline_tests.rs"]
mod deadline_tests;

#[path = "wait_guard_tests.rs"]
mod guard_tests;
