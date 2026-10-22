use super::*;

struct Siblings {
    fixture: AdmissionFixture,
    parent: ActivationRequest,
    first: ActivationRequest,
    next_parent: ActivationRequest,
    second: ActivationRequest,
}

async fn siblings() -> Siblings {
    let mut source = example("data.map");
    source["steps"]["finish"] = source["steps"]["start"].clone();
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    // Category B: children of consecutive executions while their parent keeps progressing is a
    // state the runtime never produces (a parent parks on its child); it guards fact scoping.
    let (fixture, parent) = fixture(source).await;
    let first = fixture.lineage_child(parent).await;
    let next_parent = fixture
        .persistence()
        .advance_pure(parent, budget(1))
        .await
        .unwrap()
        .continuation
        .unwrap();
    let second = fixture.lineage_child(next_parent).await;
    Siblings {
        fixture,
        parent,
        first,
        next_parent,
        second,
    }
}

fn second_query(f: &Siblings) -> ParentWakeupQuery {
    let mut query = query(f.next_parent, f.second);
    query.parent = ExecutionRef::new(
        f.parent.company,
        f.parent.run,
        f.next_parent.execution,
        StepId::parse("finish").unwrap(),
    );
    query
}

#[tokio::test]
async fn workflow_wakeup_sibling_progress_and_cursors_are_independent() {
    let f = siblings().await;
    let p = f.fixture.persistence();
    let mut held = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(f.first.run.as_uuid())
        .execute(&mut *held)
        .await
        .unwrap();
    p.advance_pure(f.second, budget(4)).await.unwrap();
    let second = p.parent_wakeups(second_query(&f)).await.unwrap();
    assert_eq!(second.len(), 1);
    assert!(wakeups(&f.fixture, f.parent, f.first).await.is_empty());
    // Parent and sibling progress despite another child's held lock.
    p.advance_pure(f.next_parent, budget(4)).await.unwrap();
    held.rollback().await.unwrap();
    p.advance_pure(f.first, budget(4)).await.unwrap();
    assert_eq!(wakeups(&f.fixture, f.parent, f.first).await.len(), 1);
    let mut cursor = second_query(&f);
    cursor.after_sequence = second[0].sequence;
    assert!(p.parent_wakeups(cursor).await.unwrap().is_empty());
}

#[tokio::test]
async fn workflow_wakeup_actual_wait_expiry_produces_failed_fact() {
    let mut source = example("wait.event");
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(1)).to_rfc3339()});
    let (f, parent, child) = chained(source).await;
    f.persistence().park_wait(child).await.unwrap().unwrap();
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1")
        .bind(child.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    assert_eq!(f.persistence().sweep_waits(8).await.unwrap(), 1);
    assert_eq!(f.persistence().sweep_waits(8).await.unwrap(), 0);
    let saved = wakeups(&f, parent, child).await;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].state, ChildTerminalState::Failed);
    assert_eq!(saved[0].terminal_execution, None);
}

async fn forged(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    sequence: i64,
    execution: Option<Uuid>,
) -> sqlx::Error {
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,terminal_state,terminal_execution_id) SELECT company_id,id,$2,'parent_wakeup',actor_id,'succeeded',$3 FROM workflow_runs WHERE id=$1")
        .bind(scope.run.as_uuid()).bind(sequence).bind(execution).execute(f.persistence().pool()).await.unwrap_err()
}

#[tokio::test]
async fn workflow_wakeup_rejects_forged_duplicate_and_overridden_transition_facts() {
    let (f, parent, child) = chained(example("data.map")).await;
    assert_eq!(
        forged(&f, child, 99, None)
            .await
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert_eq!(
        forged(&f, parent, 99, None)
            .await
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert!(
        sqlx::query(
            "UPDATE workflow_runs SET state='failed',terminal_wakeup_sequence=99 WHERE id=$1"
        )
        .bind(child.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .is_err()
    );
    f.persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    let saved = wakeups(&f, parent, child).await;
    let sequence = saved[0].sequence as i64;
    assert_eq!(
        forged(&f, child, sequence, Some(child.execution.as_uuid()))
            .await
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23505")
    );
    assert_eq!(
        forged(&f, child, sequence, Some(parent.execution.as_uuid()))
            .await
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert_eq!(
        forged(&f, child, sequence + 100, Some(child.execution.as_uuid()))
            .await
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    assert_eq!(wakeups(&f, parent, child).await, saved);
}

#[tokio::test]
async fn workflow_wakeup_parent_composite_fk_rejects_wrong_run_execution_pair() {
    let (f, parent, child) = chained(example("data.map")).await;
    let error = sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'parent_run_id',$3::uuid,'parent_execution_id',$4::uuid))).* FROM workflow_runs AS run WHERE run.id=$1")
        .bind(child.run.as_uuid()).bind(Uuid::new_v4()).bind(parent.run.as_uuid()).bind(child.execution.as_uuid())
        .execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_parent_execution_fk")
    );
}

#[tokio::test]
async fn workflow_wakeup_parent_composite_fk_rejects_valid_foreign_parent() {
    let (f, _parent, child) = chained(example("data.map")).await;
    let foreign = admission_source_tests::ForeignSource::new(&f).await;
    let foreign = admission_source_tests::foreign_parent(&f, foreign.company).await;
    let error = sqlx::query("INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs,to_jsonb(run)||jsonb_build_object('id',$2::uuid,'parent_run_id',$3::uuid,'parent_execution_id',$4::uuid))).* FROM workflow_runs AS run WHERE run.id=$1")
        .bind(child.run.as_uuid()).bind(Uuid::new_v4()).bind(foreign.run_id().as_uuid()).bind(foreign.execution_id().as_uuid())
        .execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_parent_execution_fk")
    );
}

#[path = "wakeup_parent_link_tests.rs"]
mod parent_link_tests;
