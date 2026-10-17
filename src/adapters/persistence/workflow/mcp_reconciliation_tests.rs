//! A test-host verifier attests final rejection by the recording MCP server. Production
//! MCP still installs no verifier: metadata and operator claims never permit replay.
use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::{
    PostgresActionReconciliation, SqlReconciliationResources,
};
use crate::application::workflow::{LifecycleAuthorizer, binding::ResourceStatus};

struct Resources;
#[async_trait]
impl SqlReconciliationResources for Resources {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus> {
        assert_eq!(actor, frozen.actor);
        let ActionTarget::Connection { resource, .. } = frozen.action.request().target else {
            panic!("MCP connection");
        };
        super::super::resources::inspect_on(
            tx,
            frozen.action.scope().company,
            RuntimeResourceId::new(resource),
        )
        .await?
        .ok_or_else(crate::adapters::persistence::workflow::invalid)
    }
}

struct RejectedCall(EvidenceVerifierRegistration);
#[async_trait]
impl ActionEvidenceVerifier for RejectedCall {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        &self.0
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        _: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        assert_eq!(snapshot.entries.len(), 1);
        Ok(EvidenceAttestation {
            disposition: VerifiedDisposition::FinalNotApplied,
            authoritative_reference: reference.clone(),
            observed_at: snapshot.database_now,
            valid_until: snapshot.database_now + chrono::Duration::minutes(1),
            diagnostic: AttestationDiagnostic::new(
                EvidenceProviderId::parse("fixture.mcp").unwrap(),
            ),
        })
    }
}

async fn settle_rejected(setup: &McpSetup, claim: &ClaimedWorkflow, failure: WorkflowFailure) {
    let p = &setup.fixture.persistence;
    let request = p.mcp_step_request(claim, &kind()).await.unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request.clone())
        .await
        .unwrap();
    assert_eq!(
        retire(p, claim, lease(), failure).await.run_state,
        "waiting"
    );
    let verifier = RejectedCall(
        EvidenceVerifierRegistration::approve(
            EvidenceVerifierId::parse("fixture.mcp").unwrap(),
            EvidenceVerifierVersion::parse("v1").unwrap(),
            EvidenceProviderId::parse("fixture.mcp").unwrap(),
            &request.contract,
            &request.target,
        )
        .unwrap(),
    );
    let marker: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    let command = ReconcileActionCommand {
        actor: setup.actor(),
        scope: request.scope,
        subject: intent.approval_subject(),
        marker: ActionRemoteMarkerId::new(marker),
        expected_revision: p
            .head(request.scope.company, request.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
        command_key: IdempotencyKey::parse("mcp-final-rejection").unwrap(),
        input: EvidenceInput::VerifiedReference {
            registration: verifier.0.id().clone(),
            reference: EvidenceRecordReference::parse("fixture-request-rejected").unwrap(),
        },
    };
    let service = ActionReconciliationService::new(
        PostgresActionReconciliation::new(p.clone(), Resources),
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        Some(Arc::new(verifier)),
    );
    let result =
        Box::pin(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5)))
            .await
            .unwrap();
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
}

#[tokio::test]
async fn workflow_mcp_verified_not_applied_retries_lazily_and_checks_live_drift() {
    for drift in [false, true] {
        let setup = Box::pin(mcp_setup(Call::Fail)).await;
        let p = &setup.fixture.persistence;
        let claim = claimed(&setup).await;
        let runtime = setup.runtime();
        let failure = runtime.run(&kind(), &claim).await.unwrap_err();
        assert_eq!(setup.server.calls(), 1);
        Box::pin(settle_rejected(&setup, &claim, failure)).await;
        let next = reclaim(p, claim.fence.scope).await;
        setup.server.set_call(Call::Json);
        if drift {
            setup.server.set_tools(json!([]));
        }
        let before = setup.server.count("POST initialize");
        let result = runtime.run(&kind(), &next).await;
        assert_eq!(
            setup.server.count("POST initialize"),
            before + 1,
            "result={result:?}; counts={:?}",
            action_counts(p).await
        );
        if drift {
            let failure = result.unwrap_err();
            assert_eq!(code(&failure), "mcp.unresolved");
            assert_eq!(setup.server.calls(), 1);
            assert_eq!(
                retire(p, &next, lease(), failure).await.run_state,
                "waiting"
            );
            assert_eq!(action_counts(p).await, counts(1, 2, 0));
        } else {
            assert_eq!(result.unwrap()["structuredContent"]["name"], "Ada");
            assert_eq!(setup.server.calls(), 2);
            assert_eq!(action_counts(p).await, counts(1, 2, 1));
        }
    }
}
