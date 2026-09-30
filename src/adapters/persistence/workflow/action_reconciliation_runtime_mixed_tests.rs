//! Mixed frozen policies use an explicit current policy registry.
use super::*;

#[path = "action_reconciliation_runtime_mixed_truth_tests.rs"]
mod truth_tests;

struct MixedAuthority;
#[async_trait]
impl SqlActionAuthority for MixedAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        saved: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        let mut locked = Authority.lock_current(tx, saved).await?;
        let tool: Value = sqlx::query_scalar(
            "SELECT tool FROM fixture_current_contracts WHERE name=$1 FOR SHARE",
        )
        .bind(saved.action.request().contract.contract.name.as_str())
        .fetch_one(&mut **tx)
        .await?;
        locked.policy.tool = serde_json::from_value(tool).map_err(|_| invalid())?;
        Ok(locked)
    }
}
fn mixed_writer(f: &AdmissionFixture) -> PostgresActionDispatch<MixedAuthority, Effect> {
    PostgresActionDispatch::new(f.persistence().clone(), MixedAuthority, Effect(0))
}

async fn mixed_requests(
    f: &AdmissionFixture,
    first: &ActionDispatchRequest,
) -> Vec<ActionDispatchRequest> {
    let authority = f
        .persistence()
        .action_authority(first.scope(), &first.subject)
        .await
        .unwrap();
    let original = authority.action.request().clone();
    let mut supported = original.clone();
    supported.contract.contract.name = TypeName::parse("fixture.safe-write").unwrap();
    supported.contract.policy.recovery = ActionRecovery::SafeRepeat;
    supported.arguments = json!({"value":21});
    let bundle = publication::freeze(
        crate::adapters::workflow_source::decode(authority.bundle.compiled().source()).unwrap(),
        first.scope().company,
        authority.bundle.compiled().graph().definition().version_id,
        DependencySnapshots {
            tools: vec![original.contract.clone(), supported.contract.clone()],
            ..Default::default()
        },
        vec![],
    )
    .unwrap();
    sqlx::query("UPDATE workflow_runs SET bundle=$2 WHERE id=$1")
        .bind(first.scope().run.as_uuid())
        .bind(store_bundle(&bundle).unwrap())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE fixture_current_contracts(name text PRIMARY KEY,tool jsonb NOT NULL)",
    )
    .execute(f.persistence().pool())
    .await
    .unwrap();
    for tool in [&original.contract, &supported.contract] {
        sqlx::query("INSERT INTO fixture_current_contracts(name,tool) VALUES($1,$2)")
            .bind(tool.contract.name.as_str())
            .bind(serde_json::to_value(tool).unwrap())
            .execute(f.persistence().pool())
            .await
            .unwrap();
    }
    let second = sibling(f, first, 20).await;
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(supported)
        .await
        .unwrap();
    let third = ActionDispatchRequest {
        subject: intent.approval_subject(),
        ..first.clone()
    };
    let action = f
        .persistence()
        .action_authority(third.scope(), &third.subject)
        .await
        .unwrap()
        .action;
    sqlx::query(
        "INSERT INTO fixture_provider_operations(invocation_id,provider_key) VALUES($1,$2)",
    )
    .bind(intent.invocation.as_uuid())
    .bind(action.idempotency_key().as_str())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    vec![first.clone(), second, third]
}

async fn seed_mixed(f: &AdmissionFixture, requests: &[ActionDispatchRequest]) {
    let writer = mixed_writer(f);
    let service = ActionService::new(writer);
    let receipt = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    assert!(
        service
            .dispatch_remote(&requests[0], &receipt, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(
        service
            .dispatch_remote(
                &requests[1],
                &LedgerProvider::new(f, Delivery::Pending),
                &CancellationToken::new()
            )
            .await
            .is_err()
    );
    barrier(f, &requests[1]).await;
    let supported = RegisteredLedger::new(f, &requests[2], Delivery::Pending).await;
    assert!(
        service
            .dispatch_remote(&requests[2], &supported, &CancellationToken::new())
            .await
            .is_err()
    );
    park(f, &requests[0]).await;
}
async fn continue_mixed(f: &AdmissionFixture, requests: &mut [ActionDispatchRequest]) {
    let service = ActionService::new(mixed_writer(f));
    new_claim(f, &mut requests[0]).await;
    let fence = requests[0].fence;
    for request in &mut requests[1..] {
        request.fence = fence;
    }
    let saved = LedgerProvider::new(f, Delivery::Pending);
    assert!(matches!(
        service
            .dispatch_remote(&requests[0], &saved, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(saved.calls.load(Ordering::SeqCst), 0);
    let proof = LedgerProvider::new(
        f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    assert!(matches!(
        service
            .dispatch_remote(&requests[1], &proof, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    let replay = RegisteredLedger::new(
        f,
        &requests[2],
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    )
    .await;
    assert!(matches!(
        service
            .dispatch_remote(&requests[2], &replay, &CancellationToken::new())
            .await
            .unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert_eq!(proof.calls.load(Ordering::SeqCst), 1);
    assert_eq!(replay.inner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(entry_consumptions(f).await, (5, 1));
    assert_eq!(effects(f).await, 3);
    assert!(
        f.persistence()
            .complete_io(FencedWorkflowResult {
                fence: requests[0].fence,
                output: good()
            })
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn workflow_action_reconciliation_runtime_receipt_final_supported_siblings_continue_exactly()
{
    let (f, first) = setup().await;
    let verifier = ledger(&f, &first).await;
    let mut requests = mixed_requests(&f, &first).await;
    seed_mixed(&f, &requests).await;
    let first = reconcile(
        &f,
        &requests[0],
        scoped_marker(&f, &requests[0]).await,
        verifier.clone(),
        "mixed-receipt",
    )
    .await;
    assert!(matches!(
        first.outcome,
        ReconciliationOutcome::Blocked { .. }
    ));
    let before = all_tables(&f).await;
    let last = reconcile(
        &f,
        &requests[1],
        scoped_marker(&f, &requests[1]).await,
        verifier,
        "mixed-final",
    )
    .await;
    assert_eq!(
        last.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    preserved_history(&before, &all_tables(&f).await);
    continue_mixed(&f, &mut requests).await;
}
