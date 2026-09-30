use super::*;
use crate::adapters::persistence::workflow::action_reconciliation::*;

pub(super) struct Resources;
#[async_trait]
impl SqlReconciliationResources for Resources {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus> {
        assert_eq!(actor.user_id(), frozen.actor.user_id());
        let enabled: bool = sqlx::query_scalar(
            "SELECT enabled FROM fixture_action_resources WHERE company_id=$1 FOR SHARE",
        )
        .bind(frozen.action.scope().company.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
        Ok(ResourceStatus {
            company_id: frozen.action.scope().company,
            id: RuntimeResourceId::new(frozen.action.scope().execution.as_uuid()),
            kind: TypeName::parse("fixture").unwrap(),
            authorized: enabled,
            readiness: ResourceReadiness::Ready,
            supported_contracts: [frozen.action.request().contract.contract.name.clone()].into(),
        })
    }
}
pub(super) async fn command(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    marker: Uuid,
) -> ReconcileActionCommand {
    ReconcileActionCommand {
        actor: f.binding.target.actor,
        scope: request.scope(),
        subject: request.subject.clone(),
        marker: ActionRemoteMarkerId::new(marker),
        expected_revision: f
            .persistence()
            .head(request.scope().company, request.scope().run)
            .await
            .unwrap()
            .unwrap()
            .revision,
        command_key: IdempotencyKey::parse("snapshot-command").unwrap(),
        input: EvidenceInput::UnknownNote {
            note: "operator observation".into(),
            claimed: ClaimedDisposition::NotApplied,
        },
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_snapshot_restores_terminal_and_parked_full_provenance() {
    for state in ["waiting", "cancelled", "failed"] {
        let (f, request) = setup().await;
        let writer = adapter(&f, 0);
        let RemoteReservationResult::Reserved(reservation) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("marker");
        };
        let marker = reservation.marker;
        let entry = writer.enter_remote(*reservation).await.unwrap();
        sqlx::query("UPDATE workflow_runs SET state=$2,waiting_reason=CASE WHEN $2='waiting' THEN 'reconciliation' ELSE NULL END WHERE id=$1")
            .bind(request.scope().run.as_uuid()).bind(state).execute(f.persistence().pool()).await.unwrap();
        let command = command(&f, &request, marker).await;
        let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
        assert_eq!(
            reader.association(&command).await.unwrap(),
            RelatedAssociation::Company
        );
        let ReconciliationPreparation::Snapshot(snapshot) =
            reader.snapshot(&command).await.unwrap()
        else {
            panic!("authorized snapshot");
        };
        snapshot.validate(&command).unwrap();
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].id.as_uuid(), entry.entry);
        assert_eq!(snapshot.entries[0].worker.as_uuid(), request.fence.worker.0);
        assert_eq!(snapshot.entries[0].generation, request.fence.generation);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_evidence")
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
        assert_eq!(
            count, 0,
            "reading past truth is not a grant or evidence write"
        );
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_snapshot_revision_refusal_replay_reauthorizes_resource() {
    let (f, request) = setup().await;
    let RemoteReservationResult::Reserved(reservation) =
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker");
    };
    let mut command = command(&f, &request, reservation.marker).await;
    command.expected_revision.0 += 1;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    let ReconciliationPreparation::Recorded { result, .. } =
        reader.snapshot(&command).await.unwrap()
    else {
        panic!("revision refusal");
    };
    assert_eq!(result.outcome, ReconciliationOutcome::RevisionConflict);
    assert!(!result.replayed);
    let ReconciliationPreparation::Recorded { result: replay, .. } =
        reader.snapshot(&command).await.unwrap()
    else {
        panic!("exact replay");
    };
    assert!(replay.replayed);
    assert_eq!(replay.revision, result.revision);
    let original = command.clone();
    command.input = EvidenceInput::UnknownNote {
        note: "different bytes".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    let ReconciliationPreparation::Recorded {
        result: conflict, ..
    } = reader.snapshot(&command).await.unwrap()
    else {
        panic!("key conflict");
    };
    assert_eq!(conflict.outcome, ReconciliationOutcome::IdempotencyConflict);
    sqlx::query("UPDATE fixture_action_resources SET enabled=false")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(reader.snapshot(&original).await.is_err());
    let facts: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_evidence_commands),(SELECT count(*) FROM workflow_action_evidence)")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(facts, (1, 0));
}
