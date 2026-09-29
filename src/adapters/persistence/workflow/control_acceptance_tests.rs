use super::*;
use crate::application::workflow::waits::*;

// Every fixture owns its migrated database, so complete table snapshots also catch
// unexpected extra successors, events or receipts. Ordering is independent of heap layout.
async fn control_state(f: &AdmissionFixture) -> Value {
    let mut state = serde_json::Map::new();
    for table in [
        "workflow_runs",
        "workflow_executions",
        "background_tasks",
        "task_attempts",
        "workflow_waits",
        "workflow_wait_events",
        "workflow_run_events",
        "workflow_control_commands",
        "workflow_admissions",
        "workflow_history_members",
    ] {
        let rows: Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(row) ORDER BY to_jsonb(row)::text),'[]') FROM {table} AS row"
        )).fetch_one(f.persistence().pool()).await.unwrap();
        state.insert(table.into(), rows);
    }
    Value::Object(state)
}

fn waiting_source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("wait.event").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    source["steps"]["start"]["with"]["payload_schema"] = json!({"literal":{"type":"object","properties":{"text":{"type":"string","maxLength":128}},"required":["text"],"additionalProperties":false}});
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    source
}
fn wait_signal(scope: ActivationRequest) -> WorkflowSignal {
    WorkflowSignal {
        scope,
        id: WorkflowEventId(Uuid::new_v4()),
        name: WorkflowEventName("review.completed".into()),
        correlation: WorkflowEventCorrelation("request-1".into()),
        payload: json!({"text":"accepted"}),
    }
}

#[tokio::test]
async fn workflow_control_cancel_claim_competitors_have_one_owner_and_disposition() {
    for _ in 0..3 {
        let (f, scope) = fixture().await;
        let cmd = command(&f, scope, "cancel").await;
        let barrier = Barrier::new(2);
        let cancel = async {
            barrier.wait().await;
            f.persistence().cancel(cmd).await.unwrap()
        };
        let claim = async {
            barrier.wait().await;
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
        };
        let (cancel, claim) = tokio::join!(cancel, claim);
        let state = control_state(&f).await;
        if matches!(cancel, CancelResult::Applied { .. }) {
            assert!(claim.is_none());
            assert_eq!(state["workflow_runs"][0]["state"], "cancelled");
            assert_eq!(state["task_attempts"], json!([]));
        } else {
            assert!(matches!(cancel, CancelResult::RevisionConflict { .. }));
            let claim = claim.unwrap();
            assert!(
                f.persistence()
                    .validate_io(claim.fence, policy())
                    .await
                    .unwrap()
            );
            assert_eq!(state["task_attempts"].as_array().unwrap().len(), 1);
        }
        assert_eq!(state["workflow_executions"].as_array().unwrap().len(), 1);
        assert_eq!(state["background_tasks"][0]["retry_count"], 0);
        assert_eq!(
            state["workflow_control_commands"].as_array().unwrap().len(),
            1
        );
    }
}

#[tokio::test]
async fn workflow_control_cancel_failure_competitors_close_attempt_once() {
    for _ in 0..3 {
        let (f, scope) = fixture().await;
        let claim = f
            .persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let cmd = command(&f, scope, "cancel").await;
        let cause = LeaseReleaseCause::Classified(WorkflowFailure {
            failure: StepFailure::new(
                FailureClass::Terminal,
                FailureCode::parse("provider.rejected").unwrap(),
                None,
            )
            .unwrap(),
            safety: RetrySafety::SafeToRetry,
        });
        let barrier = Barrier::new(2);
        let cancel = async {
            barrier.wait().await;
            f.persistence().cancel(cmd).await.unwrap()
        };
        let failure = async {
            barrier.wait().await;
            f.persistence()
                .release_io(claim.fence, policy(), cause)
                .await
                .unwrap()
        };
        let (cancel, failure) = tokio::join!(cancel, failure);
        let state = control_state(&f).await;
        let (run, retirement) = if matches!(cancel, CancelResult::Applied { .. }) {
            assert!(!failure);
            ("cancelled", "cancel")
        } else {
            assert!(matches!(cancel, CancelResult::RevisionConflict { .. }));
            assert!(failure);
            ("failed", "live")
        };
        assert_eq!(state["workflow_runs"][0]["state"], run);
        assert_eq!(state["task_attempts"].as_array().unwrap().len(), 1);
        assert_eq!(state["task_attempts"][0]["workflow_retirement"], retirement);
        assert_eq!(state["background_tasks"][0]["retry_count"], 1);
        assert_eq!(state["workflow_executions"].as_array().unwrap().len(), 1);
        assert_eq!(
            state["workflow_control_commands"].as_array().unwrap().len(),
            1
        );
    }
}

#[tokio::test]
async fn workflow_control_cancel_resume_competitors_preserve_event_and_notification() {
    for _ in 0..3 {
        let (f, scope) = fixture_source(waiting_source()).await;
        f.persistence().park_wait(scope).await.unwrap().unwrap();
        f.persistence()
            .record_signal(wait_signal(scope))
            .await
            .unwrap();
        let before = control_state(&f).await;
        let cmd = command(&f, scope, "cancel").await;
        let barrier = Barrier::new(2);
        let cancel = async {
            barrier.wait().await;
            f.persistence().cancel(cmd).await.unwrap()
        };
        let resume = async {
            barrier.wait().await;
            f.persistence().resume_wait(scope).await.unwrap()
        };
        let (cancel, resume) = tokio::join!(cancel, resume);
        let state = control_state(&f).await;
        if matches!(cancel, CancelResult::Applied { .. }) {
            assert!(matches!(resume, WaitProgress::Refused));
            assert_eq!(state["workflow_executions"].as_array().unwrap().len(), 1);
            assert_eq!(state["workflow_waits"][0]["state"], "cancelled");
        } else {
            assert!(matches!(cancel, CancelResult::RevisionConflict { .. }));
            assert!(matches!(resume, WaitProgress::Completed(_)));
            assert_eq!(state["workflow_executions"].as_array().unwrap().len(), 2);
            assert_eq!(state["workflow_waits"][0]["state"], "completed");
        }
        assert_eq!(
            state["workflow_wait_events"],
            before["workflow_wait_events"]
        );
        assert_eq!(
            state["workflow_waits"][0]["notification_intent"],
            before["workflow_waits"][0]["notification_intent"]
        );
        assert_eq!(state["workflow_waits"].as_array().unwrap().len(), 1);
    }
}

async fn child_command(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    key: &str,
) -> PreparedAdmission {
    let parent = ExecutionRef::new(
        scope.company,
        scope.run,
        scope.execution,
        StepId::parse("start").unwrap(),
    );
    let trigger = TriggerRef::new(
        scope.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(parent),
        },
    )
    .unwrap();
    f.prepare(f.request(key, trigger)).await
}

#[tokio::test]
async fn workflow_control_child_admission_both_deterministic_orders() {
    let (f, scope) = fixture().await;
    let existing = child_command(&f, scope, "existing").await;
    let admitted = f.persistence().admit(&existing).await.unwrap();
    assert!(matches!(admitted, AdmissionResult::Created(_)));
    f.persistence()
        .cancel(command(&f, scope, "cancel").await)
        .await
        .unwrap();
    let before = control_state(&f).await;
    assert_eq!(
        f.persistence().admit(&existing).await.unwrap(),
        AdmissionResult::Replayed(existing.proposed_run_id())
    );
    assert_eq!(control_state(&f).await, before);
    let (f, scope) = fixture().await;
    let fresh = child_command(&f, scope, "fresh").await;
    f.persistence()
        .cancel(command(&f, scope, "cancel").await)
        .await
        .unwrap();
    let before = control_state(&f).await;
    assert!(matches!(
        f.persistence().admit(&fresh).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(control_state(&f).await, before);
}

async fn install_fault(f: &AdmissionFixture, table: &str, event: &str) {
    sqlx::query("CREATE FUNCTION control_acceptance_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'control acceptance fault'; END $$")
        .execute(f.persistence().pool()).await.unwrap();
    sqlx::query(&format!("CREATE CONSTRAINT TRIGGER control_acceptance_fault AFTER {event} ON {table} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION control_acceptance_fault()"))
        .execute(f.persistence().pool()).await.unwrap();
}
async fn remove_fault(f: &AdmissionFixture, table: &str) {
    sqlx::query(&format!("DROP TRIGGER control_acceptance_fault ON {table}"))
        .execute(f.persistence().pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_control_cancel_deferred_faults_roll_back_complete_state() {
    for (table, event) in [
        ("workflow_run_events", "INSERT"),
        ("workflow_control_commands", "INSERT"),
        ("task_attempts", "UPDATE"),
    ] {
        let (f, scope) = fixture().await;
        f.persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        install_fault(&f, table, event).await;
        let cmd = command(&f, scope, "cancel").await;
        let before = control_state(&f).await;
        let error = f.persistence().cancel(cmd.clone()).await.unwrap_err();
        assert!(
            error.to_string().contains("control acceptance fault"),
            "{error}"
        );
        assert_eq!(control_state(&f).await, before, "{table}");
        remove_fault(&f, table).await;
        assert!(matches!(
            f.persistence().cancel(cmd).await.unwrap(),
            CancelResult::Applied { .. }
        ));
    }
}

#[tokio::test]
async fn workflow_control_wait_and_retry_deferred_faults_roll_back_complete_state() {
    let (f, scope) = fixture_source(waiting_source()).await;
    f.persistence().park_wait(scope).await.unwrap().unwrap();
    f.persistence()
        .record_signal(wait_signal(scope))
        .await
        .unwrap();
    install_fault(&f, "workflow_control_commands", "INSERT").await;
    let before = control_state(&f).await;
    let cmd = command(&f, scope, "cancel").await;
    assert!(f.persistence().cancel(cmd.clone()).await.is_err());
    assert_eq!(control_state(&f).await, before);
    remove_fault(&f, "workflow_control_commands").await;
    assert!(matches!(
        f.persistence().cancel(cmd).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    let (f, scope) = fixture().await;
    failed(&f, scope, RetrySafety::SafeToRetry).await;
    install_fault(&f, "workflow_control_commands", "INSERT").await;
    let before = control_state(&f).await;
    let cmd = retry(command(&f, scope, "retry").await);
    assert!(f.persistence().retry(cmd.clone()).await.is_err());
    assert_eq!(control_state(&f).await, before);
    remove_fault(&f, "workflow_control_commands").await;
    assert!(matches!(
        f.persistence().retry(cmd).await.unwrap(),
        RetryResult::Applied { .. }
    ));
}

#[tokio::test]
async fn workflow_control_owner_cascade_removes_immutable_receipt() {
    let (f, scope) = fixture().await;
    f.persistence()
        .cancel(command(&f, scope, "cancel").await)
        .await
        .unwrap();
    let before = control_state(&f).await;
    assert!(sqlx::query("UPDATE workflow_control_commands SET result_revision=result_revision+1 WHERE company_id=$1")
        .bind(scope.company.as_uuid()).execute(f.persistence().pool()).await.is_err());
    assert_eq!(control_state(&f).await, before);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(scope.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(receipts(&f).await, json!([]));
}

#[path = "control_revision_tests.rs"]
mod revision_tests;

#[tokio::test]
async fn workflow_control_principal_revocation_and_authority_locks() {
    let (f, scope) = fixture().await;
    let cmd = command(&f, scope, "cancel").await;
    f.persistence().cancel(cmd.clone()).await.unwrap();
    let before = control_state(&f).await;
    let mut authority = f.persistence().pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::authority::authorize_company(
        &mut authority,
        scope.company,
        cmd.actor,
    )
    .await
    .unwrap();
    for table in ["principals", "company_members"] {
        let mut contender = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *contender)
            .await
            .unwrap();
        let error = sqlx::query(&format!(
            "DELETE FROM {table} WHERE company_id=$1 AND user_id=$2"
        ))
        .bind(scope.company.as_uuid())
        .bind(cmd.actor.user_id())
        .execute(&mut *contender)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        contender.rollback().await.unwrap();
    }
    authority.rollback().await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(scope.company.as_uuid())
        .bind(cmd.actor.user_id())
        .execute(&mut *tx)
        .await
        .unwrap();
    let revoke = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.commit().await.unwrap();
    };
    let (_, replay) = tokio::join!(revoke, f.persistence().cancel(cmd));
    assert!(matches!(replay, Err(AppError::NotFound(_))));
    assert_eq!(control_state(&f).await, before);
}

#[tokio::test]
async fn workflow_control_foreign_scope_and_conflicting_authorized_actor_run_keys() {
    let (f, scope) = fixture().await;
    let cmd = command(&f, scope, "cancel").await;
    f.persistence().cancel(cmd.clone()).await.unwrap();
    let other = f.prepare(f.request("other", f.manual())).await;
    f.persistence().admit(&other).await.unwrap();
    let name = format!("control_{}", Uuid::new_v4().simple());
    let email = format!("{name}@example.test");
    f.persistence()
        .create_user(&name, &email, "hash")
        .await
        .unwrap();
    let user = f.persistence().get_by_email(&email).await.unwrap().unwrap();
    sqlx::query("INSERT INTO company_members (id,company_id,user_id,role) VALUES (gen_random_uuid(),$1,$2,'admin')")
        .bind(scope.company.as_uuid())
        .bind(user.id)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals (id,company_id,kind,user_id,display_label) VALUES ($1,$2,'person',$3,'Control admin') ON CONFLICT DO NOTHING")
        .bind(Uuid::new_v4()).bind(scope.company.as_uuid()).bind(user.id).execute(f.persistence().pool()).await.unwrap();
    let foreign = super::super::super::admission_source_tests::ForeignSource::new(&f).await;
    let parent =
        super::super::super::admission_source_tests::foreign_parent(&f, foreign.company).await;
    let before = control_state(&f).await;
    let mut conflict = cmd.clone();
    conflict.actor = WorkflowActor::authenticated(user.id).unwrap();
    assert!(matches!(
        f.persistence().cancel(conflict).await,
        Err(AppError::Conflict(_))
    ));
    let mut conflict = cmd.clone();
    conflict.run_id = other.proposed_run_id();
    assert!(matches!(
        f.persistence().cancel(conflict).await,
        Err(AppError::Conflict(_))
    ));
    let mut wrong_company = cmd.clone();
    wrong_company.company_id = foreign.company;
    assert!(matches!(
        f.persistence().cancel(wrong_company).await.unwrap(),
        CancelResult::NotFound
    ));
    let mut wrong_run = cmd.clone();
    wrong_run.run_id = parent.run_id();
    assert!(matches!(
        f.persistence().cancel(wrong_run).await.unwrap(),
        CancelResult::NotFound
    ));
    let mut unauthorized = cmd.clone();
    unauthorized.company_id = foreign.company;
    unauthorized.run_id = parent.run_id();
    unauthorized.actor = WorkflowActor::authenticated(user.id).unwrap();
    assert!(matches!(
        f.persistence().cancel(unauthorized).await,
        Err(AppError::NotFound(_))
    ));
    let mut missing = cmd;
    missing.run_id = RunId::new(Uuid::new_v4());
    assert!(matches!(
        f.persistence().cancel(missing).await.unwrap(),
        CancelResult::NotFound
    ));
    assert_eq!(control_state(&f).await, before);
}
