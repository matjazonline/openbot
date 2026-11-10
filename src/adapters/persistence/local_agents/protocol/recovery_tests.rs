use super::{test_support::*, *};

#[derive(Clone, Copy)]
enum Uncertainty {
    Peer,
    Deadline,
}

async fn admission(c: &Context, source: Uncertainty) -> LocalAgentOperation {
    let mut request = c.admission(c.send());
    if matches!(source, Uncertainty::Deadline) {
        request.operation = LocalAgentOperation::queued(
            request.operation.id(),
            c.send(),
            Utc::now() + chrono::Duration::seconds(2),
        )
        .unwrap();
    }
    let operation = c.f.persistence.enqueue_operation(&request).await.unwrap();
    assert!(
        c.f.persistence
            .pending_commands(c.fence)
            .await
            .unwrap()
            .iter()
            .any(|d| d.operation.id() == operation.id())
    );
    for observation in [
        agent(3, operation.id(), LocalEvidence::Receipt {}),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
    operation
}

async fn reconnect_uncertain(source: Uncertainty) {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation = admission(&c, source).await;
    let sequence = match source {
        Uncertainty::Peer => {
            c.f.persistence
                .accept_observation(
                    c.fence,
                    &agent(5, operation.id(), LocalEvidence::Uncertain {}),
                )
                .await
                .unwrap()
        }
        Uncertainty::Deadline => {
            c.expire(&operation).await;
            4
        }
    };
    let untouched =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    let (next, welcome) = c.reconnect(c.journal, sequence).await;
    assert!(!welcome.recovery_required);
    assert!(!c.f.persistence.finish_reconciliation(next).await.unwrap());
    let deliveries = c.f.persistence.pending_commands(next).await.unwrap();
    assert_eq!(
        deliveries.len(),
        1,
        "no untouched effects or reads cross the recovery barrier"
    );
    assert_eq!(deliveries[0].operation.id(), operation.id());
    assert_eq!(deliveries[0].mode, DeliveryMode::Reconcile);
    assert_eq!(
        deliveries[0].operation.outcome(),
        Some(OperationOutcome::Uncertain)
    );
    let attempted: bool = sqlx::query_scalar("SELECT delivery_attempted_at IS NOT NULL FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(untouched.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert!(!attempted);
    let done = sequence + 1;
    accept_recovered(&c, next, &operation, source, done).await;
    assert_next_generation(&c, operation.id(), untouched.id(), done).await;
}

async fn accept_recovered(
    c: &Context,
    next: ConnectionFence,
    operation: &LocalAgentOperation,
    source: Uncertainty,
    done: u64,
) {
    c.f.persistence
        .accept_observation(
            next,
            &turn(
                done,
                operation.id(),
                "turn_recovered",
                TurnState::Completed,
                Some("recovered result"),
            ),
        )
        .await
        .unwrap();
    assert!(c.f.persistence.finish_reconciliation(next).await.unwrap());
    let saved: (String, String, String) = sqlx::query_as("SELECT outcome,observed_turn_state,output FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    let outcome = match source {
        Uncertainty::Peer => "succeeded",
        Uncertainty::Deadline => "uncertain",
    };
    assert_eq!(
        saved,
        (
            outcome.into(),
            "completed".into(),
            "recovered result".into()
        )
    );
    let mut db = c.f.database.pool.acquire().await.unwrap();
    assert_eq!(
        capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
            .await
            .unwrap()
            .unsettled,
        1,
        "only the untouched effect occupies native capacity"
    );
}

async fn assert_next_generation(
    c: &Context,
    recovered: OperationId,
    untouched: OperationId,
    done: u64,
) {
    let (again, _) = c.reconnect(c.journal, done).await;
    assert!(
        c.f.persistence.finish_reconciliation(again).await.unwrap(),
        "known native completion must not block the next generation"
    );
    let deliveries = c.f.persistence.pending_commands(again).await.unwrap();
    assert!(!deliveries.iter().any(|d| d.operation.id() == recovered));
    assert!(
        deliveries
            .iter()
            .any(|d| d.operation.id() == untouched && d.mode == DeliveryMode::Dispatch)
    );
}

#[tokio::test]
async fn cloudroom_commands_expired_uncertain_work_reconciles_before_new_effects() {
    reconnect_uncertain(Uncertainty::Deadline).await;
}

#[tokio::test]
async fn cloudroom_commands_explicit_uncertainty_reconciles_before_new_effects() {
    reconnect_uncertain(Uncertainty::Peer).await;
}

fn refusals() -> [LocalRefusal; 7] {
    [
        LocalRefusal::ProjectUnavailable,
        LocalRefusal::ProviderUnavailable,
        LocalRefusal::ModelUnavailable,
        LocalRefusal::ReasoningUnavailable,
        LocalRefusal::Capacity,
        LocalRefusal::Expired,
        LocalRefusal::RecoveryRequired,
    ]
}

async fn state(c: &Context, id: OperationId) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('operation',to_jsonb(operation),'cursor',machine.acknowledged_sequence,'inbox',(SELECT count(*) FROM local_agent_observations WHERE company_id=$1)) FROM local_agent_operations AS operation JOIN local_machines AS machine ON machine.company_id=operation.company_id AND machine.id=operation.machine_id WHERE operation.company_id=$1 AND operation.id=$2")
        .bind(c.f.company).bind(id.as_uuid()).fetch_one(&c.f.database.pool).await.unwrap()
}

async fn reject_after_intent(c: &Context, operation: &LocalAgentOperation, sequence: u64) {
    let before = state(c, operation.id()).await;
    for reason in refusals() {
        let rejected = agent(sequence, operation.id(), LocalEvidence::Refused { reason });
        assert!(
            matches!(
                c.f.persistence.accept_observation(c.fence, &rejected).await,
                Err(AppError::BadRequest(_))
            ),
            "post-entry refusal {reason:?} cannot establish nonexecution"
        );
        assert_eq!(
            state(c, operation.id()).await,
            before,
            "refusal must not advance the inbox/cursor or mutate the operation"
        );
        let mut db = c.f.database.pool.acquire().await.unwrap();
        assert_eq!(
            capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
                .await
                .unwrap()
                .unsettled,
            1
        );
    }
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                sequence,
                operation.id(),
                "turn_valid_after_refusal",
                TurnState::Completed,
                Some("valid native result"),
            ),
        )
        .await
        .unwrap();
    let mut db = c.f.database.pool.acquire().await.unwrap();
    assert_eq!(
        capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
            .await
            .unwrap()
            .unsettled,
        0
    );
}

#[tokio::test]
async fn cloudroom_observations_every_preflight_refusal_rejects_after_possible_submission() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation = admission(&c, Uncertainty::Peer).await;
    reject_after_intent(&c, &operation, 5).await;
}

#[tokio::test]
async fn cloudroom_observations_every_preflight_refusal_preserves_explicit_uncertainty() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation = admission(&c, Uncertainty::Peer).await;
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(5, operation.id(), LocalEvidence::Uncertain {}),
        )
        .await
        .unwrap();
    reject_after_intent(&c, &operation, 6).await;
}

#[tokio::test]
async fn cloudroom_observations_every_preflight_refusal_preserves_expired_uncertainty() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation = admission(&c, Uncertainty::Deadline).await;
    c.expire(&operation).await;
    reject_after_intent(&c, &operation, 5).await;
    let saved = state(&c, operation.id()).await;
    assert_eq!(saved["operation"]["outcome"], "uncertain");
    assert_eq!(saved["operation"]["observed_turn_state"], "completed");
}

#[tokio::test]
async fn cloudroom_observations_preflight_refusals_before_intent_are_definitive() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut sequence = 2;
    for reason in refusals() {
        let operation =
            c.f.persistence
                .enqueue_operation(&c.admission(c.send()))
                .await
                .unwrap();
        c.f.persistence.pending_commands(c.fence).await.unwrap();
        sequence += 1;
        c.f.persistence
            .accept_observation(
                c.fence,
                &agent(sequence, operation.id(), LocalEvidence::Receipt {}),
            )
            .await
            .unwrap();
        sequence += 1;
        c.f.persistence
            .accept_observation(
                c.fence,
                &agent(sequence, operation.id(), LocalEvidence::Refused { reason }),
            )
            .await
            .unwrap();
        let saved = state(&c, operation.id()).await;
        assert_eq!(
            saved["operation"]["outcome"],
            if reason == LocalRefusal::Expired {
                "expired"
            } else {
                "failed"
            }
        );
        let mut db = c.f.database.pool.acquire().await.unwrap();
        assert_eq!(
            capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
                .await
                .unwrap()
                .unsettled,
            0
        );
    }
}
