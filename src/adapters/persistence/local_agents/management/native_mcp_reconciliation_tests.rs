//! Real broker/worker/unknown/reconciliation paths; the registered ledger verifier
//! is a trusted fixture. These are not native CLI or real-provider ledger proofs.
use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::{
    CompanyReconciliationResources, PostgresActionReconciliation,
};
use crate::application::workflow::decisions::*;
use crate::application::workflow::{IdempotencyKey, LifecycleAuthorizer, RunRevision, actions::*};
use crate::domain::workflow::{
    ActionInvocationId, ActionRemoteMarkerId, EvidenceProviderId, EvidenceRecordReference,
    EvidenceVerifierId, EvidenceVerifierVersion,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy, Debug)]
enum Proof {
    Unknown,
    Applied,
    NotApplied,
}
struct Ledger {
    registration: EvidenceVerifierRegistration,
    proof: Proof,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl ActionEvidenceVerifier for Ledger {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.registration
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        _: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(snapshot.entries.len(), 1);
        let disposition = match self.proof {
            Proof::Unknown => VerifiedDisposition::Unknown,
            Proof::NotApplied => VerifiedDisposition::FinalNotApplied,
            Proof::Applied => VerifiedDisposition::Applied {
                recovered_result: Some(
                    json!({"content":[{"type":"text","text":"Ada recovered from fixture ledger"}],"structuredContent":{"name":"Ada"},"isError":false}),
                ),
                request: AppliedEvidenceRequest::RemoteEntry(snapshot.entries[0].id),
            },
        };
        Ok(EvidenceAttestation {
            disposition,
            authoritative_reference: reference.clone(),
            observed_at: snapshot.database_now,
            valid_until: snapshot.database_now + chrono::Duration::minutes(1),
            diagnostic: AttestationDiagnostic::new(
                EvidenceProviderId::parse("fixture.native_mcp").unwrap(),
            ),
        })
    }
}

async fn unknown() -> (McpSetup, remote::McpServer, NativeMcpResponse) {
    let remote = remote::mcp_server(remote::Call::Fail).await;
    remote.set_tools(json!([{"name":"lookup","description":"Lookup","inputSchema":{"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}},"outputSchema":{"type":"object"}}]));
    let f = Box::pin(McpSetup::new_at(&remote.url)).await;
    allow_connection(&f.s).await;
    let response =
        f.s.f
            .persistence
            .native_mcp(&f.request(json!({"query":"unknown effect"})))
            .await
            .unwrap();
    let cancel = CancellationToken::new();
    let guard = cancel.clone().drop_guard();
    let task = tokio::spawn(worker(
        f.s.f.persistence.clone(),
        remote.url.clone(),
        response.run_id,
        cancel.clone(),
    ));
    approve(&f.s, response.run_id, &remote, Duration::ZERO).await;
    eventually(async || {
        let parked:bool=sqlx::query_scalar("SELECT state='waiting' AND waiting_reason='reconciliation' FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.s.f.company).bind(response.run_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
        parked.then_some(())
    }).await;
    cancel.cancel();
    task.await.unwrap();
    drop(guard);
    assert_eq!(remote.calls(), 1);
    let repeated =
        f.s.f
            .persistence
            .native_mcp(&f.request(json!({"query":"unknown effect"})))
            .await
            .unwrap();
    assert_eq!(repeated.call_id, response.call_id);
    assert!(matches!(
        repeated.state,
        NativeMcpState::ReconciliationRequired
    ));
    let mut next = f.request(json!({"query":"new effect"}));
    next.body.invocation.model_call_id = "toolu_new_effect".to_owned().try_into().unwrap();
    assert!(
        f.s.f.persistence.native_mcp(&next).await.is_err(),
        "unknown native session must not admit a replacement effect"
    );
    (f, remote, response)
}

async fn command(
    f: &McpSetup,
    response: &NativeMcpResponse,
    proof: Proof,
) -> (ReconcileActionCommand, Arc<Ledger>) {
    let (operation,marker,revision,invocation):(Value,Uuid,i64,Uuid)=sqlx::query_as("SELECT intent.operation,marker.id,run.revision,intent.id FROM workflow_action_intents AS intent JOIN workflow_action_dispatches AS marker ON marker.company_id=intent.company_id AND marker.invocation_id=intent.id JOIN workflow_runs AS run ON run.company_id=intent.company_id AND run.id=intent.run_id WHERE intent.company_id=$1 AND intent.run_id=$2")
        .bind(f.s.f.company).bind(response.run_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    let frozen = FrozenAction::restore(operation).unwrap();
    let ledger = Arc::new(Ledger {
        registration: EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.native_mcp").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture.native_mcp").unwrap(),
            &frozen.request().contract,
            &frozen.request().target,
        )
        .unwrap(),
        proof,
        calls: AtomicUsize::new(0),
    });
    let command = ReconcileActionCommand {
        actor: WorkflowActor::authenticated(f.s.f.admin).unwrap(),
        scope: frozen.scope(),
        subject: ApprovalSubject {
            invocation: ActionInvocationId::new(invocation),
            argument_digest: frozen.argument_digest().clone(),
        },
        marker: ActionRemoteMarkerId::new(marker),
        target: ReconciliationTarget::Remote,
        expected_revision: RunRevision(revision.try_into().unwrap()),
        command_key: IdempotencyKey::parse("native-historical-proof").unwrap(),
        input: EvidenceInput::VerifiedReference {
            registration: ledger.registration.id().clone(),
            reference: EvidenceRecordReference::parse("fixture-native-request").unwrap(),
        },
    };
    (command, ledger)
}

async fn revoke_original(f: &McpSetup) {
    f.s.f
        .cases
        .change(
            f.s.f.owner,
            f.s.f.company,
            f.s.profile.selection.machine_id,
            MachineChange::Revoke,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1")
        .bind(f.s.f.company)
        .execute(&f.s.f.database.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(f.s.f.company)
        .bind(f.s.f.owner)
        .execute(&f.s.f.database.pool)
        .await
        .unwrap();
    assert_eq!(f.s.f.persistence.sweep_native_mcp(16).await.unwrap(), 1);
}

async fn exercise(proof: Proof) {
    let (f, remote, response) = Box::pin(unknown()).await;
    let p = f.s.f.persistence.as_ref();
    Box::pin(revoke_original(&f)).await;
    let (mut command, ledger) = command(&f, &response, proof).await;
    let service = ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), CompanyReconciliationResources),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        Some(ledger.clone()),
    );
    // The operator must independently have the current role and sufficient knowledge.
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.s.f.company)
        .bind(f.s.f.admin)
        .execute(&f.s.f.database.pool)
        .await
        .unwrap();
    assert!(
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .is_err()
    );
    command.actor = WorkflowActor::authenticated(f.s.f.stranger).unwrap();
    assert!(
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .is_err()
    );
    assert_eq!(ledger.calls.load(Ordering::SeqCst), 0);
    sqlx::query("UPDATE company_members SET access_level=9 WHERE company_id=$1 AND user_id=$2")
        .bind(f.s.f.company)
        .bind(f.s.f.admin)
        .execute(&f.s.f.database.pool)
        .await
        .unwrap();
    command.actor = WorkflowActor::authenticated(f.s.f.admin).unwrap();
    let historical = PostgresActionReconciliation::new(p.clone(), CompanyReconciliationResources);
    historical
        .association(&command)
        .await
        .expect("historical operator can authorize the original association");
    historical
        .snapshot(&command)
        .await
        .expect("historical operator can restore the frozen native action");
    let result =
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap();
    match proof {
        Proof::Unknown => assert!(matches!(
            result.outcome,
            ReconciliationOutcome::UnknownRecorded
        )),
        Proof::Applied => assert!(matches!(
            result.outcome,
            ReconciliationOutcome::AppliedRecorded { .. }
        )),
        Proof::NotApplied => assert!(matches!(
            result.outcome,
            ReconciliationOutcome::NotAppliedRecorded
        )),
    }
    let replay =
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap();
    assert!(replay.replayed);
    assert_eq!(ledger.calls.load(Ordering::SeqCst), 1);
    let row:(String,i64,i64)=sqlx::query_as("SELECT run.state,(SELECT count(*) FROM native_mcp_calls WHERE company_id=$1),(SELECT count(*) FROM workflow_action_remote_entries WHERE company_id=$1) FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(f.s.f.company).bind(response.run_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(row, ("cancelled".into(), 1, 1));
    assert_eq!(remote.calls(), 1);
    assert!(
        p.native_mcp(&f.request(json!({"query":"unknown effect"})))
            .await
            .is_err(),
        "historical closure cannot restore revoked native authority"
    );
}

#[tokio::test]
async fn native_mcp_actual_unknown_all_historical_proofs_survive_original_actor_machine_and_grant_revocation_without_effect_retry()
 {
    for proof in [Proof::Unknown, Proof::Applied, Proof::NotApplied] {
        Box::pin(exercise(proof)).await;
    }
}

#[tokio::test]
async fn native_mcp_actual_approval_after_committed_stop_never_reaches_the_gateway() {
    let remote = remote::mcp_server(remote::Call::Json).await;
    remote.set_tools(json!([{"name":"lookup","inputSchema":{"type":"object","required":["query"],"additionalProperties":false,"properties":{"query":{"type":"string","maxLength":128}}},"outputSchema":{"type":"object"}}]));
    let f = Box::pin(McpSetup::new_at(&remote.url)).await;
    allow_connection(&f.s).await;
    let p = f.s.f.persistence.as_ref();
    let response = p
        .native_mcp(&f.request(json!({"query":"approval after stop"})))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let guard = cancel.clone().drop_guard();
    let task = tokio::spawn(worker(
        f.s.f.persistence.clone(),
        remote.url.clone(),
        response.run_id,
        cancel.clone(),
    ));
    let id = eventually(async || {
        sqlx::query_scalar::<_,Uuid>("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND submitted_at IS NULL")
            .bind(f.s.f.company).bind(response.run_id.as_uuid()).fetch_optional(p.pool()).await.unwrap()
    }).await;
    let company = CompanyId::new(f.s.f.company);
    let actor = WorkflowActor::authenticated(f.s.f.owner).unwrap();
    let view = p
        .read_decision(company, actor, DecisionId(id))
        .await
        .unwrap();
    let session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    p.control_session(&f.s.control(SessionId::new(session).unwrap(), SessionCommand::Stop))
        .await
        .unwrap();
    assert_eq!(remote.calls(), 0);
    let approval = p
        .submit_decision(SubmitDecision {
            company,
            actor,
            decision: view.decision,
            command_key: DecisionCommandKey::parse("after-native-stop").unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: "approve".into(),
            feedback: String::new(),
            data: json!({}),
        })
        .await;
    assert!(
        matches!(
            approval,
            Ok(_) | Err(AppError::Conflict(_)) | Err(AppError::NotFound(_))
        ),
        "unexpected approval error: {approval:?}"
    );
    // A competing worker may own the run briefly; the real sweeper retains its
    // durable backoff and must eventually retire it without dispatching.
    eventually(async || {
        p.sweep_native_mcp(16).await.unwrap();
        let state: String =
            sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(f.s.f.company)
                .bind(response.run_id.as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap();
        matches!(state.as_str(), "cancelled" | "failed").then_some(())
    })
    .await;
    cancel.cancel();
    task.await.unwrap();
    drop(guard);
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_dispatches WHERE company_id=$1),(SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1)")
        .bind(f.s.f.company).fetch_one(p.pool()).await.unwrap();
    assert_eq!(counts, (0, 0));
    assert_eq!(remote.calls(), 0);
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.s.f.company)
            .bind(response.run_id.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(matches!(state.as_str(), "cancelled" | "failed"));
    let live: bool = sqlx::query_scalar("SELECT public.native_mcp_run_live($1,$2)")
        .bind(f.s.f.company)
        .bind(response.run_id.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(!live);
    eprintln!("late native approval: {approval:?}; state={state}");
}
