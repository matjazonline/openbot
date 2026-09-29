use super::*;
use crate::application::workflow::{completion::*, lease::*, waits::*, wakeups::*};

fn example(kind: &str) -> Value {
    serde_json::from_str(&registry::example(kind).unwrap().source).unwrap()
}

async fn admit_child(
    f: &AdmissionFixture,
    parent: ActivationRequest,
    step: &str,
    key: &str,
) -> ActivationRequest {
    let cause = ExecutionRef::new(
        parent.company,
        parent.run,
        parent.execution,
        StepId::parse(step).unwrap(),
    );
    let trigger = TriggerRef::new(
        parent.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(cause),
        },
    )
    .unwrap();
    let command = f.prepare(f.request(key, trigger)).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: parent.company,
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

fn query(parent: ActivationRequest, child: ActivationRequest) -> ParentWakeupQuery {
    ParentWakeupQuery {
        parent: ExecutionRef::new(
            parent.company,
            parent.run,
            parent.execution,
            StepId::parse("start").unwrap(),
        ),
        child: child.run,
        after_sequence: 0,
        limit: 128,
    }
}

async fn wakeups(
    f: &AdmissionFixture,
    parent: ActivationRequest,
    child: ActivationRequest,
) -> Vec<ParentWakeup> {
    f.persistence()
        .parent_wakeups(query(parent, child))
        .await
        .unwrap()
}

async fn parent_state(f: &AdmissionFixture, parent: ActivationRequest) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(run) FROM workflow_runs AS run WHERE id=$1")
        .bind(parent.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_wakeup_completion_has_no_parent_run_or_execution_lock() {
    let (f, parent) = fixture(example("data.map")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    let before = parent_state(&f, parent).await;
    let mut held = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(parent.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM workflow_executions WHERE id=$1 FOR UPDATE")
        .bind(parent.execution.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    // A hidden parent FK lock would time out here; the parent is never released
    // until child completion has committed on another connection.
    tokio::time::timeout(
        Duration::from_secs(2),
        f.persistence().advance_pure(child, budget(4)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(wakeups(&f, parent, child).await.len(), 1);
    held.rollback().await.unwrap();
    assert_eq!(parent_state(&f, parent).await, before);
    let saved = wakeups(&f, parent, child).await;
    assert_eq!(saved[0].state, ChildTerminalState::Succeeded);
    assert_eq!(saved[0].terminal_execution, Some(child.execution));
    f.persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    assert_eq!(wakeups(&f, parent, child).await, saved);
    let fresh = PostgresPersistence::new(f.persistence().pool().clone());
    assert_eq!(
        fresh.parent_wakeups(query(parent, child)).await.unwrap(),
        saved
    );
    let mut cursor = query(parent, child);
    cursor.after_sequence = saved[0].sequence;
    assert!(fresh.parent_wakeups(cursor).await.unwrap().is_empty());
}

#[tokio::test]
async fn workflow_wakeup_real_child_completion_and_parent_cancel_contenders() {
    let (f, parent) = fixture(example("data.map")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    let barrier = Barrier::new(3);
    let complete = || async {
        barrier.wait().await;
        f.persistence()
            .advance_pure(child, budget(4))
            .await
            .unwrap()
    };
    let cancel = async {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(parent.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        barrier.wait().await;
        sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE id=$1")
            .bind(parent.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(complete(), complete(), cancel)
    })
    .await
    .unwrap();
    assert_eq!(parent_state(&f, parent).await["state"], "cancelled");
    assert_eq!(wakeups(&f, parent, child).await.len(), 1);
    let parent_completed: bool =
        sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM workflow_executions WHERE id=$1")
            .bind(parent.execution.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert!(!parent_completed);
}

#[tokio::test]
async fn workflow_wakeup_fenced_io_and_timer_completion_use_same_fact() {
    let (f, parent) = fixture(example("context.load")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    let claimed = f
        .persistence()
        .claim_io(
            child,
            WorkflowWorkerId(Uuid::new_v4()),
            LeasePolicy::new(Duration::from_secs(3)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    f.persistence()
        .complete_io(FencedWorkflowResult {
            fence: claimed.fence,
            output: json!({"items":[],"token_count":0}),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        wakeups(&f, parent, child).await[0].state,
        ChildTerminalState::Succeeded
    );

    let mut source = example("wait.timer");
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    let (f, parent) = fixture(source).await;
    let child = admit_child(&f, parent, "start", "child").await;
    f.persistence().park_wait(child).await.unwrap().unwrap();
    assert!(wakeups(&f, parent, child).await.is_empty());
    f.persistence().resume_wait(child).await.unwrap();
    assert_eq!(
        wakeups(&f, parent, child).await[0].state,
        ChildTerminalState::Succeeded
    );
}

#[tokio::test]
async fn workflow_wakeup_write_and_commit_failures_rollback_progression() {
    for deferred in [false, true] {
        let (f, parent) = fixture(example("data.map")).await;
        let child = admit_child(&f, parent, "start", "child").await;
        sqlx::raw_sql("CREATE FUNCTION reject_wakeup() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_kind='parent_wakeup' THEN RAISE EXCEPTION 'wakeup injected'; END IF; RETURN NEW; END $$")
            .execute(f.persistence().pool()).await.unwrap();
        let trigger = if deferred {
            "CREATE CONSTRAINT TRIGGER reject_wakeup AFTER INSERT ON workflow_run_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_wakeup()"
        } else {
            "CREATE TRIGGER reject_wakeup BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION reject_wakeup()"
        };
        sqlx::raw_sql(trigger)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let before = snapshot(&f).await;
        assert!(
            f.persistence()
                .advance_pure(child, budget(4))
                .await
                .is_err()
        );
        assert_eq!(snapshot(&f).await, before);
        sqlx::raw_sql("DROP TRIGGER reject_wakeup ON workflow_run_events")
            .execute(f.persistence().pool())
            .await
            .unwrap();
        f.persistence()
            .advance_pure(child, budget(4))
            .await
            .unwrap();
        assert_eq!(wakeups(&f, parent, child).await.len(), 1);
    }
}

#[tokio::test]
async fn workflow_wakeup_terminal_transitions_are_immutable_and_noops_do_not_repeat() {
    let (f, parent) = fixture(example("data.map")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    for state in ["failed", "cancelled"] {
        sqlx::query("UPDATE workflow_runs SET state=$2 WHERE id=$1")
            .bind(child.run.as_uuid())
            .bind(state)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        let saved = wakeups(&f, parent, child).await;
        sqlx::query("UPDATE workflow_runs SET state=state WHERE id=$1")
            .bind(child.run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(wakeups(&f, parent, child).await, saved);
    }
    let saved = wakeups(&f, parent, child).await;
    assert_eq!(
        saved.iter().map(|event| event.state).collect::<Vec<_>>(),
        vec![ChildTerminalState::Failed, ChildTerminalState::Cancelled]
    );
    assert!(sqlx::query("UPDATE workflow_run_events SET terminal_state='succeeded' WHERE run_id=$1 AND event_kind='parent_wakeup'")
        .bind(child.run.as_uuid()).execute(f.persistence().pool()).await.is_err());
    assert_eq!(wakeups(&f, parent, child).await, saved);
    f.persistence()
        .advance_pure(parent, budget(4))
        .await
        .unwrap();
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_events WHERE event_kind='parent_wakeup'",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(total, 2);
}

#[tokio::test]
async fn workflow_wakeup_scope_bounds_and_parent_identity_guards() {
    let (f, parent) = fixture(example("data.map")).await;
    let child = admit_child(&f, parent, "start", "child").await;
    f.persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    for limit in [0, 129] {
        let mut q = query(parent, child);
        q.limit = limit;
        assert!(f.persistence().parent_wakeups(q).await.is_err());
    }
    let mut q = query(parent, child);
    q.after_sequence = u64::MAX;
    assert!(f.persistence().parent_wakeups(q).await.is_err());
    let mut q = query(parent, child);
    q.parent = ExecutionRef::new(
        CompanyId::new(Uuid::new_v4()),
        parent.run,
        parent.execution,
        StepId::parse("start").unwrap(),
    );
    assert!(f.persistence().parent_wakeups(q).await.unwrap().is_empty());
    let mut q = query(parent, child);
    q.parent = ExecutionRef::new(
        parent.company,
        parent.run,
        parent.execution,
        StepId::parse("wrong").unwrap(),
    );
    assert!(f.persistence().parent_wakeups(q).await.unwrap().is_empty());
    assert!(
        sqlx::query(
            "UPDATE workflow_runs SET parent_run_id=NULL,parent_execution_id=NULL WHERE id=$1"
        )
        .bind(child.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE workflow_admissions SET source_key='v1:[\"manual\",\"fake\"]' WHERE run_id=$1"
        )
        .bind(child.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .is_err()
    );
}

#[path = "wakeup_guard_tests.rs"]
mod guard_tests;

#[path = "budget_schema_tests.rs"]
mod budget_schema_tests;

#[path = "budget_activation_tests.rs"]
mod budget_activation_tests;
