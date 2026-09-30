//! Immediate native controls only: these adversarial candidates never claim deferred validity.
use super::*;

struct SqlCandidate {
    command: ReconcileActionCommand,
    id: Uuid,
    evidence: Uuid,
    result_revision: i64,
}

async fn insert_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    candidate: &SqlCandidate,
    audit: i64,
) -> Result<Value, sqlx::Error> {
    let command = &candidate.command;
    sqlx::query_scalar("INSERT INTO workflow_action_evidence_commands(company_id,command_key,id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,actor_id,request_digest,expected_revision,result_revision,outcome,evidence_id,audit_sequence,scheduled_job_id,previous_state,previous_waiting_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NULL,NULL,NULL) RETURNING to_jsonb(workflow_action_evidence_commands)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(candidate.id)
        .bind(command.scope.run.as_uuid()).bind(command.scope.execution.as_uuid()).bind(command.subject.invocation.as_uuid())
        .bind(command.subject.argument_digest.as_str()).bind(command.marker.as_uuid()).bind(command.actor.user_id())
        .bind(command.request_digest().unwrap().as_str()).bind(i64::try_from(command.expected_revision.0).unwrap())
        .bind(candidate.result_revision).bind(json!({"kind":"unknown_recorded"})).bind(candidate.evidence).bind(audit)
        .fetch_one(&mut **tx).await
}

fn structured_native(error: &sqlx::Error) -> String {
    let native = error
        .as_database_error()
        .unwrap()
        .downcast_ref::<PgDatabaseError>();
    assert_eq!(native.code(), "23505");
    assert_eq!(native.constraint(), Some(OWNERSHIP));
    assert_eq!(native.schema(), Some("public"));
    assert_eq!(native.table(), Some("workflow_action_evidence_commands"));
    assert_eq!(
        native.message(),
        format!("duplicate key value violates unique constraint \"{OWNERSHIP}\"")
    );
    eprintln!(
        "native_SQL_control_only code={} constraint={:?} schema={:?} table={:?} message={} detail={:?}",
        native.code(),
        native.constraint(),
        native.schema(),
        native.table(),
        native.message(),
        native.detail()
    );
    error.to_string()
}

fn assert_candidate(row: &Value, candidate: &SqlCandidate, audit: i64) {
    let command = &candidate.command;
    assert_eq!(
        row["company_id"],
        command.scope.company.as_uuid().to_string()
    );
    assert_eq!(row["command_key"], command.command_key.as_str());
    assert_eq!(row["id"], candidate.id.to_string());
    assert_eq!(row["run_id"], command.scope.run.as_uuid().to_string());
    assert_eq!(
        row["execution_id"],
        command.scope.execution.as_uuid().to_string()
    );
    assert_eq!(
        row["invocation_id"],
        command.subject.invocation.as_uuid().to_string()
    );
    assert_eq!(
        row["argument_digest"],
        command.subject.argument_digest.as_str()
    );
    assert_eq!(row["dispatch_id"], command.marker.as_uuid().to_string());
    assert_eq!(row["actor_id"], command.actor.user_id().to_string());
    assert_eq!(
        row["request_digest"],
        command.request_digest().unwrap().as_str()
    );
    assert_eq!(row["expected_revision"], command.expected_revision.0);
    assert_eq!(row["result_revision"], candidate.result_revision);
    assert_eq!(row["outcome"], json!({"kind":"unknown_recorded"}));
    assert_eq!(row["evidence_id"], candidate.evidence.to_string());
    assert_eq!(row["audit_sequence"], audit);
    for field in [
        "scheduled_job_id",
        "previous_state",
        "previous_waiting_reason",
    ] {
        assert!(row[field].is_null());
    }
    assert!(row["created_at"].as_str().is_some());
}

pub(super) async fn pair(
    owner: &UnknownOwner,
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
) -> String {
    let f = &owner.fixture;
    let mut command = owner.candidate.clone();
    command.command_key =
        IdempotencyKey::parse(format!("sql-candidate-{}", Uuid::new_v4().simple())).unwrap();
    let candidate = SqlCandidate {
        command,
        id: Uuid::new_v4(),
        evidence: Uuid::parse_str(owner.history["evidence"]["id"].as_str().unwrap()).unwrap(),
        result_revision: i64::try_from(owner.candidate.expected_revision.0).unwrap(),
    };
    assert_ne!(candidate.command.command_key, owner.candidate.command_key);
    assert_ne!(candidate.id.to_string(), owner.history["command"]["id"]);
    let baseline = all_tables(f).await;
    let retained = owner.history["audit"]["sequence"].as_i64().unwrap();
    let mut observed = None;
    // Identical explicit candidate bytes: only audit_sequence differs.
    for positive in [true, false] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let actual = backend(&mut tx, observer).await;
        let fresh: i64 = sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) SELECT $1,$2,COALESCE(MAX(sequence),0)+1,'action_reconciled',$3,$4 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 RETURNING sequence")
            .bind(candidate.command.scope.company.as_uuid()).bind(candidate.command.scope.run.as_uuid())
            .bind(candidate.command.actor.user_id()).bind(candidate.command.scope.execution.as_uuid())
            .fetch_one(&mut *tx).await.unwrap();
        assert_ne!(fresh, retained);
        sqlx::query("SAVEPOINT native_ownership_control")
            .execute(&mut *tx)
            .await
            .unwrap();
        let audit = if positive { fresh } else { retained };
        let result = insert_candidate(&mut tx, &candidate, audit).await;
        if positive {
            let row = result.expect("other immediate constraints must pass fresh ownership");
            assert_candidate(&row, &candidate, fresh);
            let readback: Value = sqlx::query_scalar("SELECT to_jsonb(command) FROM workflow_action_evidence_commands AS command WHERE company_id=$1 AND command_key=$2")
                .bind(candidate.command.scope.company.as_uuid()).bind(candidate.command.command_key.as_str()).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(row, readback);
        } else {
            observed = Some(structured_native(
                &result.expect_err("retained audit native UNIQUE"),
            ));
        }
        // Deferred evidence-command binding is intentionally unproved; no flush/COMMIT.
        tx.rollback().await.unwrap();
        quiescent(observer, &actual).await;
        assert_eq!(
            all_tables(f).await,
            baseline,
            "native SQL branch explicit rollback preserves every public row"
        );
    }
    observed.unwrap()
}
