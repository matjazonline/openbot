use super::*;

#[tokio::test]
async fn workflow_action_evidence_state_witness_keeps_first_state_and_reason() {
    for initial in ["cancelled", "failed", "waiting"] {
        let (f, request) = setup().await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("UPDATE workflow_runs SET state=$2,waiting_reason=CASE WHEN $2='waiting' THEN 'decision' ELSE NULL END WHERE id=$1")
            .bind(request.scope().run.as_uuid()).bind(initial).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query(
            "UPDATE workflow_runs SET state='waiting',waiting_reason='reconciliation' WHERE id=$1",
        )
        .bind(request.scope().run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE id=$1")
            .bind(request.scope().run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        let witness: (String, Option<String>) = sqlx::query_as("SELECT initial_state,initial_waiting_reason FROM workflow_action_state_witnesses WHERE run_id=$1 AND transaction_id=pg_current_xact_id()")
            .bind(request.scope().run.as_uuid()).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(witness.0, initial);
        assert_eq!(
            witness.1.as_deref(),
            (initial == "waiting").then_some("decision")
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn workflow_action_evidence_witness_rejects_direct_caller_insert() {
    let (f, request) = setup().await;
    let error = sqlx::query("INSERT INTO workflow_action_state_witnesses(company_id,run_id,transaction_id,initial_state,initial_waiting_reason) VALUES($1,$2,pg_current_xact_id(),'waiting','reconciliation')")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn workflow_action_evidence_remote_entry_transaction_identity_cannot_be_spoofed() {
    let (f, request) = setup().await;
    let writer = adapter(&f, 0);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker");
    };
    let marker = reservation.marker;
    let prior: String = sqlx::query_scalar("SELECT pg_current_xact_id()::text")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let error = sqlx::query("INSERT INTO workflow_action_remote_entries(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,job_id,attempt_number,execution_generation,worker_id,reservation_xid) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12::text::xid8)")
        .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).bind(marker).bind(Uuid::new_v4())
        .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
        .bind(request.fence.generation.0).bind(request.fence.worker.0).bind(prior)
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    let entry = writer.enter_remote(*reservation).await.unwrap();
    let current: bool = sqlx::query_scalar(
        "SELECT reservation_xid IS NOT NULL FROM workflow_action_remote_entries WHERE id=$1",
    )
    .bind(entry.entry)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert!(current);
}
