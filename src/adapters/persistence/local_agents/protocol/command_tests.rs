use super::{test_support::*, *};
use tokio::sync::Barrier;

#[path = "admission_failure_tests.rs"]
mod failures;

#[tokio::test]
async fn cloudroom_commands_competing_logical_invocations_reuse_frozen_operation() {
    let Some(c) = Context::new().await else {
        return;
    };
    let admission = c.admission(c.send());
    let mut replay = admission.clone();
    replay.operation = LocalAgentOperation::queued(
        OperationId::new(Uuid::new_v4()).unwrap(),
        admission.operation.command().clone(),
        admission.operation.deadline(),
    )
    .unwrap();
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        c.f.persistence.enqueue_operation(&admission).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        c.f.persistence.enqueue_operation(&replay).await.unwrap()
    };
    let (a, b) = tokio::join!(first, second);
    assert_eq!(a.id(), b.id());
    assert_eq!(a.payload_digest(), b.payload_digest());
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND logical_key=$2",
    )
    .bind(c.f.company)
    .bind(admission.logical_key.as_str())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);
    replay.operation = LocalAgentOperation::queued(
        replay.operation.id(),
        LocalCommand::Send {
            session_id: c.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            prompt: LocalPrompt::try_from("changed inputs".to_owned()).unwrap(),
        },
        admission.operation.deadline(),
    )
    .unwrap();
    assert!(matches!(
        c.f.persistence.enqueue_operation(&replay).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn cloudroom_commands_competing_capacity_keeps_eight_control_slots() {
    let Some(c) = Context::new().await else {
        return;
    };
    for _ in 0..7 {
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    }
    let a = c.admission(c.send());
    let b = c.admission(c.send());
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        c.f.persistence.enqueue_operation(&a).await
    };
    let second = async {
        barrier.wait().await;
        c.f.persistence.enqueue_operation(&b).await
    };
    let (a, b) = tokio::join!(first, second);
    assert_ne!(a.is_ok(), b.is_ok());
    for _ in 0..24 {
        c.f.persistence
            .enqueue_operation(&c.admission(LocalCommand::Inspect {
                session_id: c.session.id,
                thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            }))
            .await
            .unwrap();
    }
    let extra = c.admission(LocalCommand::StopThread {
        session_id: c.session.id,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
    });
    assert!(matches!(
        c.f.persistence.enqueue_operation(&extra).await,
        Err(AppError::Conflict(_))
    ));
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND outcome IS NULL",
    )
    .bind(c.f.company)
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(queued, 32);
}

#[tokio::test]
async fn cloudroom_commands_withdrawal_refuses_untouched_but_reconciles_possible_delivery() {
    let Some(c) = Context::new().await else {
        return;
    };
    let attempted =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    let deliveries = c.f.persistence.pending_commands(c.fence).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].mode, DeliveryMode::Dispatch);
    let untouched =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.cases
        .set_projects(c.f.owner, c.f.company, c.paired.receipt.machine.id, &[])
        .await
        .unwrap();
    let deliveries = c.f.persistence.pending_commands(c.fence).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].operation.id(), attempted.id());
    assert_eq!(deliveries[0].mode, DeliveryMode::Reconcile);
    let state:(Option<String>,Option<DateTime<Utc>>)=sqlx::query_as("SELECT outcome,delivery_attempted_at FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(untouched.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(state.0.as_deref(), Some("cancelled"));
    assert!(state.1.is_none());
    c.f.cases
        .change(
            c.f.owner,
            c.f.company,
            c.paired.receipt.machine.id,
            MachineChange::Revoke,
        )
        .await
        .unwrap();
    let outcome: String = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(attempted.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(outcome, "uncertain");
    assert!(matches!(
        c.f.persistence.pending_commands(c.fence).await,
        Err(AppError::InvalidCredentials)
    ));
}

#[tokio::test]
async fn cloudroom_commands_competing_expiration_sweeps_leave_no_poison_batch() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut ids = Vec::new();
    for attempted in [false, true, false] {
        let id = Uuid::new_v4();
        ids.push(id);
        sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,logical_key,command,payload_digest,deadline,delivery_attempted_at) VALUES($1,$2,$3,$4,$5,'{\"kind\":\"refresh_catalog\"}',$6,clock_timestamp()-INTERVAL '1 second',CASE WHEN $7 THEN clock_timestamp() ELSE NULL END)")
            .bind(c.f.company).bind(id).bind(c.paired.receipt.machine.id.as_uuid()).bind(c.paired.receipt.machine.installation_id.as_uuid())
            .bind(id.to_string()).bind("0".repeat(64)).bind(attempted).execute(&c.f.database.pool).await.unwrap();
    }
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        c.f.persistence.expire_operations(2).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        c.f.persistence.expire_operations(2).await.unwrap()
    };
    let (a, b) = tokio::join!(first, second);
    assert_eq!(a + b, 3);
    assert_eq!(c.f.persistence.expire_operations(100).await.unwrap(), 0);
    let outcomes:Vec<String>=sqlx::query_scalar("SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=ANY($2) ORDER BY outcome")
        .bind(c.f.company).bind(ids).fetch_all(&c.f.database.pool).await.unwrap();
    assert_eq!(outcomes, vec!["expired", "expired", "uncertain"]);
}
#[tokio::test]
async fn cloudroom_commands_readiness_stays_open_for_new_delivery_but_not_replacement() {
    let Some(c) = Context::new().await else {
        return;
    };
    let op =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    let deliveries = c.f.persistence.pending_commands(c.fence).await.unwrap();
    assert!(
        deliveries
            .iter()
            .any(|d| d.operation.id() == op.id() && d.mode == DeliveryMode::Dispatch)
    );
    assert!(
        c.f.persistence
            .finish_reconciliation(c.fence)
            .await
            .unwrap(),
        "a new unacknowledged dispatch cannot close an already-open generation"
    );
    let (next, welcome) = c.reconnect(c.journal, 2).await;
    assert!(!welcome.recovery_required);
    assert!(
        !c.f.persistence.finish_reconciliation(next).await.unwrap(),
        "the replacement must still reconcile possible native work"
    );
}
