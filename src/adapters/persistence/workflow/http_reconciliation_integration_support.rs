//! Test-host finality over an actual loopback provider; no production rows are fabricated.
use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::task::JoinHandle;

#[derive(Default)]
struct ProviderState {
    calls: AtomicUsize,
    effects: AtomicUsize,
    apply: AtomicBool,
}

pub(super) struct Provider {
    pub origin: String,
    state: Arc<ProviderState>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl Provider {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(ProviderState::default());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let router = Router::new()
            .fallback(post(Self::call))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            origin,
            state,
            stop,
            task: Some(task),
        }
    }

    async fn call(State(state): State<Arc<ProviderState>>) -> Response {
        state.calls.fetch_add(1, Ordering::SeqCst);
        if state.apply.load(Ordering::SeqCst) {
            state.effects.fetch_add(1, Ordering::SeqCst);
            // The effect committed, but the response stream breaks before a complete body.
            // This is actual transport-response loss, not a complete HTTP error response.
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-length", "11")
                .body(Body::from_stream(futures::stream::once(async {
                    Err::<Bytes, _>(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "fixture lost response after effect",
                    ))
                })))
                .unwrap();
        }
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }

    pub fn apply_next(&self) {
        self.state.apply.store(true, Ordering::SeqCst);
    }
    pub fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }
    pub fn effects(&self) -> usize {
        self.state.effects.load(Ordering::SeqCst)
    }

    pub async fn finish(mut self) {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(3), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

pub(super) enum Truth {
    Rejected,
    Applied(ActionRemoteEntryId),
}

/// A fixed observation of the completed provider request. Reusing it after a later request
/// must fail the application's freshness checks; the verifier cannot refresh its timestamp.
pub(super) struct CapturedProof {
    registration: EvidenceVerifierRegistration,
    observed_at: DateTime<Utc>,
    truth: Truth,
}

impl CapturedProof {
    pub async fn capture(setup: &Setup, request: &ActionRequest, truth: Truth) -> Arc<Self> {
        Arc::new(Self {
            registration: EvidenceVerifierRegistration::approve(
                EvidenceVerifierId::parse("fixture.http-ledger").unwrap(),
                EvidenceVerifierVersion::parse("v1").unwrap(),
                EvidenceProviderId::parse("fixture.http").unwrap(),
                &request.contract,
                &request.target,
            )
            .unwrap(),
            observed_at: sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(setup.fixture.persistence.pool())
                .await
                .unwrap(),
            truth,
        })
    }
}

#[async_trait]
impl ActionEvidenceVerifier for CapturedProof {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.registration
    }
    async fn verify(
        &self,
        _: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        _: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        Ok(EvidenceAttestation {
            disposition: match self.truth {
                Truth::Rejected => VerifiedDisposition::FinalNotApplied,
                Truth::Applied(entry) => VerifiedDisposition::Applied {
                    recovered_result: Some(json!({"status":200,"headers":{},"body":{"ok":true}})),
                    request: AppliedEvidenceRequest::RemoteEntry(entry),
                },
            },
            authoritative_reference: reference.clone(),
            observed_at: self.observed_at,
            valid_until: self.observed_at + chrono::Duration::minutes(1),
            diagnostic: AttestationDiagnostic::new(
                EvidenceProviderId::parse("fixture.http").unwrap(),
            ),
        })
    }
}

pub(super) async fn command(
    setup: &Setup,
    request: &ActionRequest,
    key: &str,
) -> ReconcileActionCommand {
    let p = &setup.fixture.persistence;
    let intent = ActionService::new(p.clone())
        .prepare_step(request.clone())
        .await
        .unwrap();
    ReconcileActionCommand {
        actor: setup.fixture.target.actor,
        scope: request.scope,
        subject: intent.approval_subject(),
        marker: ActionRemoteMarkerId::new(
            sqlx::query_scalar(
                "SELECT id FROM workflow_action_dispatches WHERE company_id=$1 AND run_id=$2",
            )
            .bind(request.scope.company.as_uuid())
            .bind(request.scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap(),
        ),
        target: ReconciliationTarget::Remote,
        expected_revision: p
            .head(request.scope.company, request.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
        command_key: IdempotencyKey::parse(key).unwrap(),
        input: EvidenceInput::VerifiedReference {
            registration: EvidenceVerifierId::parse("fixture.http-ledger").unwrap(),
            reference: EvidenceRecordReference::parse(key).unwrap(),
        },
    }
}

pub(super) async fn reconcile(
    setup: &Setup,
    command: &ReconcileActionCommand,
    proof: Arc<CapturedProof>,
) -> AppResult<ReconciliationResult> {
    use crate::adapters::persistence::workflow::action_reconciliation::{
        CompanyReconciliationResources, PostgresActionReconciliation,
    };
    use crate::application::workflow::LifecycleAuthorizer;
    let p = &setup.fixture.persistence;
    // Reconstruct service state on every call, as a restarted application would.
    ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), CompanyReconciliationResources),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        Some(proof),
    )
    .reconcile(command, &CancellationToken::new(), Duration::from_secs(5))
    .await
}

pub(super) async fn snapshot(setup: &Setup) -> Value {
    let mut tx = setup.fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut result = serde_json::Map::new();
    for table in [
        "workflow_runs",
        "workflow_executions",
        "background_tasks",
        "task_attempts",
        "workflow_action_dispatches",
        "workflow_action_remote_entries",
        "workflow_action_receipts",
        "workflow_action_evidence",
        "workflow_action_evidence_consumptions",
        "workflow_action_evidence_commands",
        "workflow_action_claim_episodes",
        "workflow_action_schedule_witnesses",
        "workflow_action_state_witnesses",
    ] {
        let rows: Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(row) ORDER BY to_jsonb(row)::text),'[]'::jsonb) FROM {table} AS row"
        )).fetch_one(&mut *tx).await.unwrap();
        result.insert(table.into(), rows);
    }
    tx.commit().await.unwrap();
    Value::Object(result)
}

pub(super) fn count(state: &Value, table: &str) -> usize {
    state[table].as_array().unwrap().len()
}

pub(super) async fn claim_competitors(setup: &Setup) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let ready = tokio::sync::Barrier::new(2);
    let claim = || async {
        ready.wait().await;
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
    };
    let (left, right) = tokio::join!(claim(), claim());
    assert_eq!(
        usize::from(left.is_some()) + usize::from(right.is_some()),
        1
    );
    left.or(right).unwrap()
}
