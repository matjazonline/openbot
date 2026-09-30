use super::*;
use crate::adapters::persistence::workflow::action_dispatch::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::binding::{ResourceReadiness, ResourceStatus};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;

struct Authority;
#[async_trait]
impl SqlActionAuthority for Authority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        saved: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        let request = saved.action.request();
        let enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM fixture_action_resources WHERE company_id=$1 FOR SHARE",
        )
        .bind(request.scope.company.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
        let (enabled_policy, approval, tool): (bool, bool, Value) = sqlx::query_as("SELECT enabled,approval,tool FROM fixture_action_policies WHERE company_id=$1 FOR SHARE")
            .bind(request.scope.company.as_uuid()).fetch_one(&mut **tx).await?;
        if !enabled_policy {
            return Err(missing());
        }
        Ok(LockedActionAuthority {
            resource: ResourceStatus {
                company_id: request.scope.company,
                id: RuntimeResourceId::new(request.scope.execution.as_uuid()),
                kind: TypeName::parse("fixture").unwrap(),
                authorized: enabled,
                readiness: ResourceReadiness::Ready,
                supported_contracts: [request.contract.contract.name.clone()].into(),
            },
            policy: CurrentActionPolicy {
                tool: serde_json::from_value(tool).map_err(|_| invalid())?,
                approval_required: approval,
            },
        })
    }
}
struct Effect(u8);
#[async_trait]
impl SqlLocalAction for Effect {
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
    ) -> AppResult<Value> {
        sqlx::query("INSERT INTO fixture_action_effects VALUES ($1)")
            .bind(action.scope().execution.as_uuid())
            .execute(&mut **tx)
            .await?;
        match self.0 {
            1 => Err(AppError::Internal("Injected effect fault".into())),
            2 => Ok(json!("invalid output")),
            _ => Ok(json!({"written":true})),
        }
    }
}
type Dispatch = PostgresActionDispatch<Authority, Effect>;
fn adapter(f: &AdmissionFixture, mode: u8) -> Dispatch {
    PostgresActionDispatch::new(f.persistence().clone(), Authority, Effect(mode))
}
async fn setup() -> (AdmissionFixture, ActionDispatchRequest) {
    setup_action(publication::ActionRecovery::Reconcile, json!({"value":1})).await
}
async fn setup_action(
    recovery: publication::ActionRecovery,
    arguments: Value,
) -> (AdmissionFixture, ActionDispatchRequest) {
    let (f, scope) = fixture().await;
    // Box the fixture seam so extended owner-created histories still use stock 2 MiB.
    Box::pin(setup_action_on_fixture(f, scope, recovery, arguments)).await
}
async fn setup_action_on_fixture(
    f: AdmissionFixture,
    scope: ActivationRequest,
    recovery: publication::ActionRecovery,
    arguments: Value,
) -> (AdmissionFixture, ActionDispatchRequest) {
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    // Historical upgrade fixtures enter here with the matching version's real claim owner.
    Box::pin(setup_action_on_claim(f, claim, recovery, arguments)).await
}
async fn setup_action_on_claim(
    f: AdmissionFixture,
    claim: ClaimedWorkflow,
    recovery: publication::ActionRecovery,
    arguments: Value,
) -> (AdmissionFixture, ActionDispatchRequest) {
    sqlx::raw_sql("CREATE TABLE fixture_action_resources(company_id uuid PRIMARY KEY,enabled bool NOT NULL); CREATE TABLE fixture_action_policies(company_id uuid PRIMARY KEY,enabled bool NOT NULL,approval bool NOT NULL,tool jsonb NOT NULL); CREATE TABLE fixture_action_effects(execution_id uuid PRIMARY KEY);")
        .execute(f.persistence().pool()).await.unwrap();
    // Bootstrap once; shared-database companies use the same genuine initialization owner.
    Box::pin(initialize_action_on_claim(f, claim, recovery, arguments)).await
}
async fn initialize_action_on_claim(
    f: AdmissionFixture,
    claim: ClaimedWorkflow,
    recovery: publication::ActionRecovery,
    arguments: Value,
) -> (AdmissionFixture, ActionDispatchRequest) {
    let scope = claim.fence.scope;
    let mut action = crate::application::workflow::actions::tests::request(ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    });
    action.context.actor = f.binding.target.actor.user_id();
    action.contract.policy.recovery = recovery;
    action.arguments = arguments;
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(action.clone())
        .await
        .unwrap();
    let authority = f
        .persistence()
        .action_authority(action.scope, &intent.approval_subject())
        .await
        .unwrap();
    // Production publication remains fail-closed pending04.9/10. This isolated fixture
    // installs a genuinely validated frozen bundle to test the new writer underneath it.
    let bundle = publication::freeze(
        crate::adapters::workflow_source::decode(authority.bundle.compiled().source()).unwrap(),
        scope.company,
        authority.bundle.compiled().graph().definition().version_id,
        DependencySnapshots {
            tools: vec![action.contract.clone()],
            ..Default::default()
        },
        vec![],
    )
    .unwrap();
    sqlx::query("UPDATE workflow_runs SET bundle=$2 WHERE id=$1")
        .bind(scope.run.as_uuid())
        .bind(store_bundle(&bundle).unwrap())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO fixture_action_resources VALUES ($1,true)")
        .bind(scope.company.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO fixture_action_policies VALUES ($1,true,false,$2)")
        .bind(scope.company.as_uuid())
        .bind(serde_json::to_value(action.contract).unwrap())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    (
        f,
        ActionDispatchRequest {
            subject: intent.approval_subject(),
            fence: claim.fence,
            lease: policy(),
        },
    )
}
async fn facts(f: &AdmissionFixture) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_dispatches), (SELECT count(*) FROM workflow_action_receipts), (SELECT count(*) FROM fixture_action_effects)")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_action_dispatch_competing_local_and_restart_receipt() {
    let (f, request) = setup().await;
    let left = ActionService::new(adapter(&f, 0));
    let right = ActionService::new(adapter(&f, 0));
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            left.dispatch_local(&request).await
        },
        async {
            barrier.wait().await;
            right.dispatch_local(&request).await
        }
    );
    for result in [a, b] {
        let LocalDispatchResult::Committed(receipt) = result.unwrap() else {
            panic!("missing receipt")
        };
        assert_eq!(receipt.result, json!({"written":true}));
    }
    assert_eq!(facts(&f).await, (1, 1, 1));
    assert!(matches!(
        ActionService::new(adapter(&f, 1))
            .dispatch_local(&request)
            .await
            .unwrap(),
        LocalDispatchResult::Committed(_)
    ));
    assert_eq!(facts(&f).await, (1, 1, 1));
}

#[tokio::test]
async fn workflow_action_dispatch_local_faults_rollback_every_write() {
    for mode in [1, 2] {
        let (f, request) = setup().await;
        assert!(
            ActionService::new(adapter(&f, mode))
                .dispatch_local(&request)
                .await
                .is_err()
        );
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
    for (table, deferred) in [
        ("fixture_action_effects", false),
        ("workflow_action_receipts", false),
        ("workflow_action_dispatches", true),
    ] {
        let (f, request) = setup().await;
        sqlx::raw_sql(&format!("CREATE FUNCTION action_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected fault'; END $$; CREATE {}TRIGGER action_fault AFTER INSERT ON {table} {}FOR EACH ROW EXECUTE FUNCTION action_fault();",if deferred {"CONSTRAINT "} else {""},if deferred {"DEFERRABLE INITIALLY DEFERRED "} else {""}))
            .execute(f.persistence().pool()).await.unwrap();
        assert!(
            ActionService::new(adapter(&f, 0))
                .dispatch_local(&request)
                .await
                .is_err()
        );
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_dispatch_stale_fences_scopes_protection_and_errors() {
    let (f, request) = setup().await;
    let service = ActionService::new(adapter(&f, 0));
    for mode in 0..8 {
        let mut wrong = request.clone();
        match mode {
            0 => wrong.fence.worker = worker(),
            1 => wrong.fence.generation = WorkflowGeneration(Uuid::new_v4()),
            2 => wrong.fence.attempt = WorkflowAttempt(99),
            3 => wrong.fence.scope.company = CompanyId::new(Uuid::new_v4()),
            4 => wrong.fence.scope.run = RunId::new(Uuid::new_v4()),
            5 => wrong.fence.scope.execution = ExecutionId::new(Uuid::new_v4()),
            6 => wrong.fence.scope.job = WorkflowJobId(Uuid::new_v4()),
            _ => wrong.subject.invocation = ActionInvocationId::new(Uuid::new_v4()),
        }
        assert!(service.dispatch_local(&wrong).await.is_err());
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
    sqlx::query("UPDATE fixture_action_policies SET approval=true")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(matches!(
        service.dispatch_local(&request).await.unwrap(),
        LocalDispatchResult::ApprovalRequired
    ));
    assert!(matches!(
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap(),
        RemoteReservationResult::ApprovalRequired
    ));
    assert_eq!(facts(&f).await, (0, 0, 0));
    sqlx::query("DROP TABLE fixture_action_policies")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(service.dispatch_local(&request).await.is_err());
    assert_eq!(facts(&f).await, (0, 0, 0));
}

struct Provider<'a> {
    f: &'a AdmissionFixture,
    calls: AtomicUsize,
    lost: bool,
}
#[async_trait]
impl RemoteAction for Provider<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, invocation: &ProviderInvocation) -> AppResult<Value> {
        assert!(invocation.idempotency_key().is_none());
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            facts(self.f).await,
            (1, 0, 0),
            "marker must be committed before polling provider"
        );
        if self.lost {
            return Err(AppError::Timeout("Lost remote response".into()));
        }
        Ok(json!({"observed":true}))
    }
}
#[tokio::test]
async fn workflow_action_dispatch_remote_marker_crash_response_and_no_replay() {
    for lost in [false, true] {
        let (f, request) = setup().await;
        let provider = Provider {
            f: &f,
            calls: AtomicUsize::new(0),
            lost,
        };
        let service = ActionService::new(adapter(&f, 0));
        let result = service
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await;
        if lost {
            assert!(result.is_err());
        } else {
            assert!(matches!(
                result.unwrap(),
                RemoteDispatchObservation::Committed(_)
            ));
        }
        let fresh = ActionService::new(adapter(&f, 0));
        let replay = fresh
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap();
        assert!(if lost {
            matches!(replay, RemoteDispatchObservation::PossibleDispatchExists)
        } else {
            matches!(replay, RemoteDispatchObservation::Committed(_))
        });
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(facts(&f).await, (1, i64::from(!lost), 0));
    }
    let (f, request) = setup().await;
    let writer = adapter(&f, 0);
    let (a, b) = tokio::join!(
        writer.reserve_remote(&request, None),
        writer.reserve_remote(&request, None)
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, RemoteReservationResult::Reserved(_)))
            .count(),
        1
    );
    drop(outcomes); // crash after committed marker, before provider polling
    let provider = Provider {
        f: &f,
        calls: AtomicUsize::new(0),
        lost: false,
    };
    assert!(matches!(
        ActionService::new(adapter(&f, 0))
            .dispatch_remote(&request, &provider, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::PossibleDispatchExists
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn workflow_action_dispatch_waiting_revokers_recheck_current_authority() {
    for sql in [
        "UPDATE fixture_action_resources SET enabled=false WHERE company_id=$1",
        "UPDATE fixture_action_policies SET enabled=false WHERE company_id=$1",
        "DELETE FROM company_members WHERE company_id=$1",
    ] {
        let (f, request) = setup().await;
        let mut revoke = f.persistence().pool().begin().await.unwrap();
        sqlx::query(sql)
            .bind(request.scope().company.as_uuid())
            .execute(&mut *revoke)
            .await
            .unwrap();
        let writer = adapter(&f, 0);
        let release = async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                f.persistence().pool(),
                1,
            )
            .await;
            revoke.commit().await.unwrap();
        };
        let (result, ()) = tokio::join!(writer.local(&request), release);
        assert!(result.is_err());
        assert_eq!(facts(&f).await, (0, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_dispatch_post_marker_revocation_cancel_and_expiry_refuse_entry() {
    for sql in [
        "UPDATE fixture_action_resources SET enabled=false WHERE company_id=$1",
        "UPDATE fixture_action_policies SET approval=true WHERE company_id=$1",
        "UPDATE workflow_runs SET state='cancelled' WHERE company_id=$1",
        "UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds', lock_expires_at=clock_timestamp()-interval '1 second' WHERE company_id=$1",
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '10 seconds', deadline=clock_timestamp()-interval '1 second' WHERE company_id=$1",
    ] {
        let (f, request) = setup().await;
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(reservation) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("no first reservation")
        };
        sqlx::query(sql)
            .bind(request.scope().company.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        assert!(writer.enter_remote(*reservation).await.is_err());
        assert_eq!(facts(&f).await, (1, 0, 0));
    }
}

#[tokio::test]
async fn workflow_action_dispatch_sql_commit_expiry_and_append_only_facts() {
    let (f, request) = setup().await;
    let writer = adapter(&f, 0);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let AuthorizedAction {
        action,
        current_policy: policy,
        approval_required: protected,
    } = writer.authorize_on(&mut tx, &request).await.unwrap();
    assert!(!protected);
    let marker = Uuid::new_v4();
    PostgresActionDispatch::<Authority, Effect>::insert(&mut tx, &request, marker, "local", policy)
        .await
        .unwrap();
    let output = Effect(0).apply(&mut tx, &action).await.unwrap();
    sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,'local',$7)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).bind(marker).bind(output).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds', lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    assert!(
        tx.commit().await.is_err(),
        "deferred SQL fence must reject independently of Rust checks"
    );
    assert_eq!(facts(&f).await, (0, 0, 0));
    assert!(matches!(
        writer.local(&request).await.unwrap(),
        LocalDispatchResult::Committed(_)
    ));
    for table in ["workflow_action_dispatches", "workflow_action_receipts"] {
        for command in [
            format!("DELETE FROM {table}"),
            format!("UPDATE {table} SET created_at=created_at"),
        ] {
            assert!(
                sqlx::query(&command)
                    .execute(f.persistence().pool())
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(facts(&f).await, (1, 1, 1));
}

#[path = "action_dispatch_remote_tests.rs"]
mod remote_tests;
#[path = "action_dispatch_sql_tests.rs"]
mod sql_tests;

#[path = "action_replay_tests.rs"]
mod replay_tests;

#[path = "action_receipt_tests.rs"]
mod action_receipt_tests;

#[path = "action_evidence_binding_tests.rs"]
mod action_evidence_binding_tests;

#[path = "action_reconciliation_snapshot_tests.rs"]
mod action_reconciliation_snapshot_tests;

#[path = "action_reconciliation_receipt_tests.rs"]
mod action_reconciliation_receipt_tests;
