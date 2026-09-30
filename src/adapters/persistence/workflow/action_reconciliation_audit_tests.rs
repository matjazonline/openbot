//! Per-command actor audits retain exact immutable provenance under contention.
use super::*;

async fn execute(f: &AdmissionFixture, command: &ReconcileActionCommand) -> ReconciliationResult {
    let persistence = f.persistence().clone();
    let service = ActionReconciliationService::new(
        PostgresActionReconciliation::new(persistence.clone(), Resources),
        LifecycleAuthorizer::new(
            persistence.clone(),
            persistence.clone(),
            persistence.clone(),
        ),
        Directory(persistence),
        None,
    );
    Box::pin(service.reconcile(command, &CancellationToken::new(), Duration::from_secs(5)))
        .await
        .unwrap()
}

async fn contender(
    f: &AdmissionFixture,
    command: &ReconcileActionCommand,
    barrier: &tokio::sync::Barrier,
) -> ReconciliationResult {
    barrier.wait().await;
    execute(f, command).await
}

async fn audited(f: &AdmissionFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_action_evidence_commands AS command JOIN workflow_action_evidence AS evidence ON evidence.company_id=command.company_id AND evidence.id=command.evidence_id AND evidence.command_key=command.command_key AND evidence.command_id=command.id AND evidence.request_digest=command.request_digest JOIN workflow_run_events AS audit ON audit.company_id=command.company_id AND audit.run_id=command.run_id AND audit.sequence=command.audit_sequence AND audit.execution_id=command.execution_id AND audit.actor_id=command.actor_id AND audit.event_kind='action_reconciled'")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_action_audit_competing_commands_and_exact_replay() {
    let (f, request) = setup().await;
    let RemoteReservationResult::Reserved(reserved) =
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker");
    };
    park(&f, &request).await;
    let first = command(&f, &request, reserved.marker).await;
    let barrier = tokio::sync::Barrier::new(2);
    let (left, right) = tokio::join!(
        contender(&f, &first, &barrier),
        contender(&f, &first, &barrier)
    );
    assert_ne!(left.replayed, right.replayed);
    assert_eq!(left.evidence, right.evidence);
    assert_eq!(left.revision, right.revision);
    assert_eq!(audited(&f).await, 1);

    let mut second = command(&f, &request, reserved.marker).await;
    second.command_key = IdempotencyKey::parse("contender-two").unwrap();
    let mut third = second.clone();
    third.command_key = IdempotencyKey::parse("contender-three").unwrap();
    let (left, right) = tokio::join!(
        contender(&f, &second, &barrier),
        contender(&f, &third, &barrier)
    );
    assert_ne!(left.evidence.is_some(), right.evidence.is_some());
    let refused = if left.evidence.is_none() { left } else { right };
    assert!(matches!(
        refused.outcome,
        ReconciliationOutcome::RevisionConflict
            | ReconciliationOutcome::Blocked {
                reason: ReconciliationBlockedReason::StaleSnapshot
            }
    ));
    assert_eq!(audited(&f).await, 2);

    let mut later = command(&f, &request, reserved.marker).await;
    later.command_key = IdempotencyKey::parse("later-fresh-command").unwrap();
    assert!(matches!(
        execute(&f, &later).await.outcome,
        ReconciliationOutcome::UnknownRecorded
    ));
    assert_eq!(audited(&f).await, 3);
    let sequences: i64 = sqlx::query_scalar("SELECT count(DISTINCT audit_sequence) FROM workflow_action_evidence_commands WHERE evidence_id IS NOT NULL")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(sequences, 3);
    let before: Value = sqlx::query_scalar("SELECT jsonb_build_object('run',(SELECT to_jsonb(run) FROM workflow_runs AS run),'evidence',(SELECT jsonb_agg(to_jsonb(evidence)) FROM workflow_action_evidence AS evidence),'commands',(SELECT jsonb_agg(to_jsonb(command)) FROM workflow_action_evidence_commands AS command),'audit',(SELECT jsonb_agg(to_jsonb(audit)) FROM workflow_run_events AS audit))")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert!(execute(&f, &later).await.replayed);
    let after: Value = sqlx::query_scalar("SELECT jsonb_build_object('run',(SELECT to_jsonb(run) FROM workflow_runs AS run),'evidence',(SELECT jsonb_agg(to_jsonb(evidence)) FROM workflow_action_evidence AS evidence),'commands',(SELECT jsonb_agg(to_jsonb(command)) FROM workflow_action_evidence_commands AS command),'audit',(SELECT jsonb_agg(to_jsonb(audit)) FROM workflow_run_events AS audit))")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(before, after);
}

fn check_error(error: &sqlx::Error, message: &str) {
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some("23514"));
    assert!(database.message().contains(message), "{database}");
}

#[tokio::test]
async fn workflow_action_audit_sql_orphan_immutability_and_other_kind_policy() {
    let (f, request) = setup().await;
    let RemoteReservationResult::Reserved(reserved) =
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker");
    };
    park(&f, &request).await;
    execute(&f, &command(&f, &request, reserved.marker).await).await;
    for sql in [
        "UPDATE workflow_run_events SET actor_id=actor_id WHERE event_kind='action_reconciled'",
        "DELETE FROM workflow_run_events WHERE event_kind='action_reconciled'",
    ] {
        let error = sqlx::query(sql)
            .execute(f.persistence().pool())
            .await
            .unwrap_err();
        check_error(&error, "actor audit is append-only");
    }
    let error = sqlx::query("UPDATE workflow_run_events SET event_kind='action_reconciled',execution_id=$1 WHERE event_kind='admitted'")
        .bind(request.scope().execution.as_uuid()).execute(f.persistence().pool()).await.unwrap_err();
    check_error(&error, "cannot become a reconciliation actor audit");
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,run_id,MAX(sequence)+1,'action_reconciled',$1,$2 FROM workflow_run_events GROUP BY company_id,run_id")
        .bind(f.binding.target.actor.user_id()).bind(request.scope().execution.as_uuid())
        .execute(&mut *tx).await.unwrap();
    check_error(
        &tx.commit().await.unwrap_err(),
        "requires one exact evidence command",
    );
    let error = sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id) SELECT company_id,run_id,MAX(sequence)+1,'action_reconciled',$1 FROM workflow_run_events GROUP BY company_id,run_id")
        .bind(f.binding.target.actor.user_id()).execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_action_audit_execution")
    );

    let mut tx = f.persistence().pool().begin().await.unwrap();
    for _ in 0..2 {
        sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id) SELECT company_id,run_id,MAX(sequence)+1,'audit_nullable_test',$1 FROM workflow_run_events GROUP BY company_id,run_id")
            .bind(f.binding.target.actor.user_id()).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,run_id,MAX(sequence)+1,'audit_nonnull_test',$1,$2 FROM workflow_run_events GROUP BY company_id,run_id")
        .bind(f.binding.target.actor.user_id()).bind(request.scope().execution.as_uuid()).execute(&mut *tx).await.unwrap();
    let error=sqlx::query("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,run_id,MAX(sequence)+1,'audit_nonnull_test',$1,$2 FROM workflow_run_events GROUP BY company_id,run_id")
        .bind(f.binding.target.actor.user_id()).bind(request.scope().execution.as_uuid()).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_run_event_execution_kind")
    );
    tx.rollback().await.unwrap();
    assert_eq!(audited(&f).await, 1);
}

async fn clone_facts(tx: &mut Transaction<'_, Postgres>, audit_sequence: i64) {
    let command_id = Uuid::new_v4();
    let evidence_id = Uuid::new_v4();
    sqlx::query("INSERT INTO workflow_action_evidence SELECT (jsonb_populate_record(NULL::workflow_action_evidence,to_jsonb(evidence)||$1::jsonb)).* FROM workflow_action_evidence AS evidence LIMIT 1")
        .bind(json!({"id":evidence_id,"command_id":command_id,"command_key":"sql-cloned-command"}))
        .execute(&mut **tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_action_evidence_commands SELECT (jsonb_populate_record(NULL::workflow_action_evidence_commands,to_jsonb(command)||$1::jsonb)).* FROM workflow_action_evidence_commands AS command LIMIT 1")
        .bind(json!({"id":command_id,"evidence_id":evidence_id,"command_key":"sql-cloned-command","audit_sequence":audit_sequence}))
        .execute(&mut **tx).await.unwrap();
}

#[tokio::test]
async fn workflow_action_audit_sql_exact_actor_kind_and_unique_command_reference() {
    let (f, request) = setup().await;
    let RemoteReservationResult::Reserved(reserved) =
        adapter(&f, 0).reserve_remote(&request, None).await.unwrap()
    else {
        panic!("marker");
    };
    park(&f, &request).await;
    execute(&f, &command(&f, &request, reserved.marker).await).await;
    for (kind, actor) in [
        ("action_reconciled", Uuid::new_v4()),
        ("different_kind", f.binding.target.actor.user_id()),
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let sequence: i64 = sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT company_id,run_id,MAX(sequence)+1,$1,$2,$3 FROM workflow_run_events GROUP BY company_id,run_id RETURNING sequence")
            .bind(kind).bind(actor).bind(request.scope().execution.as_uuid()).fetch_one(&mut *tx).await.unwrap();
        clone_facts(&mut tx, sequence).await;
        let error = sqlx::query("SET CONSTRAINTS workflow_action_command_scope_guard IMMEDIATE")
            .execute(&mut *tx)
            .await
            .unwrap_err();
        check_error(&error, "command audit scope mismatch");
        tx.rollback().await.unwrap();
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let sequence: i64 =
        sqlx::query_scalar("SELECT audit_sequence FROM workflow_action_evidence_commands")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let command_id = Uuid::new_v4();
    let error=sqlx::query("INSERT INTO workflow_action_evidence_commands SELECT (jsonb_populate_record(NULL::workflow_action_evidence_commands,to_jsonb(command)||$1::jsonb)).* FROM workflow_action_evidence_commands AS command LIMIT 1")
        .bind(json!({"id":command_id,"command_key":"duplicate-audit","audit_sequence":sequence}))
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_action_command_audit_unique")
    );
    tx.rollback().await.unwrap();
    assert_eq!(audited(&f).await, 1);
}
