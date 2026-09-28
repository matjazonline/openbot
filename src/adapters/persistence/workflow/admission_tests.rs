use super::*;
#[path = "admission_authority_tests.rs"]
mod admission_authority_tests;
#[path = "admission_history_tests.rs"]
mod admission_history_tests;
#[path = "admission_source_tests.rs"]
mod admission_source_tests;
use crate::domain::entities::correlation::CorrelationId;
use binding_tests::BindingFixture;
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Option<PreparedAdmission>>>);
#[async_trait]
impl WorkflowAdmission for Capture {
    async fn admit(&self, command: &PreparedAdmission) -> AppResult<AdmissionResult> {
        *self.0.lock().unwrap() = Some(command.clone());
        Ok(AdmissionResult::Created(command.proposed_run_id()))
    }
}
// The admission service does not use runtime ports. Fail loudly if that changes;
// no production runtime implementation is introduced by this admission fixture.
#[derive(Clone, Copy)]
struct NoRuntime;
#[async_trait]
impl WorkflowRunTransitions for NoRuntime {
    async fn cancel(&self, _: CancelCommand) -> AppResult<CancelResult> {
        panic!("admission must not call runtime cancellation")
    }
}
#[async_trait]
impl WorkflowInspection for NoRuntime {
    async fn head(&self, _: CompanyId, _: RunId) -> AppResult<Option<RunHead>> {
        panic!("admission must not call runtime inspection")
    }
}

struct AdmissionFixture {
    binding: BindingFixture,
}
impl AdmissionFixture {
    async fn new() -> Self {
        let binding = BindingFixture::new(json!([])).await;
        Self::with_binding(binding).await
    }
    async fn with_binding(binding: BindingFixture) -> Self {
        let configured = binding
            .service()
            .configure(binding.request(None))
            .await
            .unwrap();
        binding
            .service()
            .activate(SetBindingActivity {
                target: binding.target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        Self { binding }
    }
    fn persistence(&self) -> &PostgresPersistence {
        &self.binding.fixture.persistence
    }
    fn request(&self, key: &str, trigger: TriggerRef) -> AdmitWorkflowRequest {
        AdmitWorkflowRequest {
            company_id: self.binding.target.company,
            actor: self.binding.target.actor,
            association: RelatedAssociation::Company,
            trigger,
            correlation_id: CorrelationId::new(),
            binding_id: self.binding.target.binding,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({"value":1}),
        }
    }
    fn manual(&self) -> TriggerRef {
        TriggerRef::new(
            self.binding.target.company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap()
    }
    async fn prepare(&self, request: AdmitWorkflowRequest) -> PreparedAdmission {
        let capture = Capture::default();
        WorkflowService::new(
            self.persistence().clone(),
            capture.clone(),
            NoRuntime,
            NoRuntime,
            Preflight,
        )
        .admit(request)
        .await
        .unwrap();
        capture.0.lock().unwrap().take().unwrap()
    }
    async fn counts(&self) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_build_object( \
             'runs',(SELECT count(*) FROM workflow_runs), \
             'executions',(SELECT count(*) FROM workflow_executions), \
             'jobs',(SELECT count(*) FROM background_tasks WHERE queue_kind = 'workflow'), \
             'admissions',(SELECT count(*) FROM workflow_admissions), \
             'commands',(SELECT count(*) FROM workflow_admission_commands), \
             'events',(SELECT count(*) FROM workflow_run_events), \
             'history',(SELECT count(*) FROM workflow_history_members))",
        )
        .fetch_one(self.persistence().pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn workflow_admit_sql_exact_snapshot_job_and_replay_preserves_deadline() {
    let f = AdmissionFixture::new().await;
    let trigger = f.manual();
    let command = f.prepare(f.request("original", trigger.clone())).await;
    let p = f.persistence();
    assert_eq!(
        p.admit(&command).await.unwrap(),
        AdmissionResult::Created(command.proposed_run_id())
    );
    let snapshot: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('input',input,'params',params,'resources',resources, \
         'revision',binding_revision,'steps',max_steps,'bytes',max_context_bytes, \
         'lifetime',extract(epoch FROM deadline-created_at),'deadline',deadline, \
         'actor',actor_id,'trigger',trigger_id,'correlation',correlation_id) FROM workflow_runs WHERE id=$1",
    ).bind(command.proposed_run_id().as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(snapshot["input"], *command.input());
    assert_eq!(snapshot["params"], *command.params());
    assert_eq!(snapshot["resources"], json!({}));
    assert_eq!(
        snapshot["revision"],
        json!(command.binding().revision().get())
    );
    let limits = command
        .binding()
        .bundle()
        .compiled()
        .graph()
        .definition()
        .limits;
    assert_eq!(snapshot["steps"], json!(limits.max_steps));
    assert_eq!(snapshot["bytes"], json!(limits.max_context_bytes));
    assert_eq!(
        snapshot["lifetime"].as_f64().unwrap(),
        f64::from(admission::ADMISSION_DEADLINE_SECONDS)
    );
    assert_eq!(snapshot["actor"], json!(command.actor().user_id()));
    assert_eq!(snapshot["trigger"], json!(trigger.trigger_id().as_uuid()));
    assert_eq!(
        snapshot["correlation"],
        json!(command.causality().correlation_id().as_uuid())
    );
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT bundle FROM workflow_runs WHERE id=$1")
        .bind(command.proposed_run_id().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(bytes, store_bundle(command.binding().bundle()).unwrap());
    let job: Value =
        sqlx::query_scalar("SELECT payload FROM background_tasks WHERE workflow_execution_id=$1")
            .bind(command.first_execution_id().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        job,
        json!({"version":1,"execution_id":command.first_execution_id().as_uuid()})
    );
    let replay = f.prepare(f.request("original", trigger)).await;
    assert_ne!(replay.proposed_run_id(), command.proposed_run_id());
    assert_eq!(
        p.admit(&replay).await.unwrap(),
        AdmissionResult::Replayed(command.proposed_run_id())
    );
    let deadline: Value =
        sqlx::query_scalar("SELECT to_jsonb(deadline) FROM workflow_runs WHERE id=$1")
            .bind(command.proposed_run_id().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(snapshot["deadline"], deadline);
    assert_eq!(
        f.counts().await,
        json!({"runs":1,"executions":1,"jobs":1,"admissions":1,"commands":1,"events":1,"history":0})
    );
}

#[tokio::test]
async fn workflow_admit_sql_competing_same_key_and_canonical_source() {
    let f = AdmissionFixture::new().await;
    let source = f.manual();
    let first = f.prepare(f.request("same", source.clone())).await;
    let second = f.prepare(f.request("same", source.clone())).await;
    let p = f.persistence();
    let (a, b) = tokio::join!(p.admit(&first), p.admit(&second));
    let results = [a.unwrap(), b.unwrap()];
    let run = results
        .iter()
        .find_map(|result| match result {
            AdmissionResult::Created(run) => Some(*run),
            _ => None,
        })
        .unwrap();
    assert!(results.contains(&AdmissionResult::Replayed(run)));
    assert_eq!(f.counts().await["jobs"], 1);
    let alias_a = f.prepare(f.request("alias-a", source.clone())).await;
    let alias_b = f.prepare(f.request("alias-b", source)).await;
    let (a, b) = tokio::join!(p.admit(&alias_a), p.admit(&alias_b));
    assert_eq!(a.unwrap(), AdmissionResult::Replayed(run));
    assert_eq!(b.unwrap(), AdmissionResult::Replayed(run));
    assert_eq!(f.counts().await["commands"], 3);
    assert_eq!(f.counts().await["runs"], 1);
    // Different-key competitors on a source which has not been admitted yet.
    let fresh = f.manual();
    let third = f.prepare(f.request("fresh-a", fresh.clone())).await;
    let fourth = f.prepare(f.request("fresh-b", fresh)).await;
    let (a, b) = tokio::join!(p.admit(&third), p.admit(&fourth));
    let results = [a.unwrap(), b.unwrap()];
    let created = results
        .iter()
        .find_map(|result| match result {
            AdmissionResult::Created(run) => Some(*run),
            _ => None,
        })
        .unwrap();
    assert!(results.contains(&AdmissionResult::Replayed(created)));
    assert_eq!(f.counts().await["jobs"], 2);
    assert_eq!(f.counts().await["commands"], 5);
}

#[tokio::test]
async fn workflow_admit_sql_stale_selection_conflicts_and_saved_replay_survives_removal() {
    let f = AdmissionFixture::new().await;
    let source = f.manual();
    let command = f.prepare(f.request("original", source.clone())).await;
    let stale = f.prepare(f.request("stale", f.manual())).await;
    let p = f.persistence();
    p.admit(&command).await.unwrap();
    let state = f.binding.state().await;
    let mut changed = f.binding.request(Some(state.revision));
    changed.params = json!({"later":true});
    f.binding.service().configure(changed).await.unwrap();
    assert!(matches!(p.admit(&stale).await, Err(AppError::Conflict(_))));
    let mut wrong = f.request("changed-input", source.clone());
    wrong.input = json!({"value":2});
    let wrong = f.prepare(wrong).await;
    assert_eq!(p.admit(&wrong).await.unwrap(), AdmissionResult::Conflict);
    let mut same_key_wrong = f.request("original", source.clone());
    same_key_wrong.input = json!({"value":3});
    let wrong = f.prepare(same_key_wrong).await;
    assert_eq!(p.admit(&wrong).await.unwrap(), AdmissionResult::Conflict);
    sqlx::query("DELETE FROM workflow_bindings WHERE company_id=$1 AND id=$2")
        .bind(f.binding.target.company.as_uuid())
        .bind(f.binding.target.binding.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_definitions SET archived=true WHERE company_id=$1")
        .bind(f.binding.target.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let replay = f.prepare(f.request("alias", source)).await;
    assert_eq!(replay.params(), command.params());
    assert_eq!(
        p.admit(&replay).await.unwrap(),
        AdmissionResult::Replayed(command.proposed_run_id())
    );
    assert_eq!(f.counts().await["jobs"], 1);
    assert_eq!(f.counts().await["commands"], 2);
}

#[tokio::test]
async fn workflow_admit_sql_final_and_deferred_failure_roll_back_all_records() {
    let f = AdmissionFixture::new().await;
    let c = admission_history_tests::Conversation::new(&f).await;
    let p = f.persistence();
    sqlx::query("CREATE FUNCTION reject_test_admission() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected final admission failure'; END; $$")
        .execute(p.pool()).await.unwrap();
    for timing in ["BEFORE", "DEFERRED"] {
        let statement = if timing == "BEFORE" {
            "CREATE TRIGGER reject_admission BEFORE INSERT ON workflow_run_events FOR EACH ROW EXECUTE FUNCTION reject_test_admission()"
        } else {
            "CREATE CONSTRAINT TRIGGER reject_admission AFTER INSERT ON workflow_run_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_test_admission()"
        };
        sqlx::query(statement).execute(p.pool()).await.unwrap();
        let command = f.prepare(c.request(&f, timing, c.message)).await;
        assert!(p.admit(&command).await.is_err());
        assert_eq!(
            f.counts().await,
            json!({"runs":0,"executions":0,"jobs":0,"admissions":0,"commands":0,"events":0,"history":0})
        );
        sqlx::query("DROP TRIGGER reject_admission ON workflow_run_events")
            .execute(p.pool())
            .await
            .unwrap();
    }
}
