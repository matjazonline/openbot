//! A fresh refused-shape SQL linkage attack follows an actual stale service refusal.
use super::*;

struct RefusalCandidate {
    command: ReconcileActionCommand,
    id: Uuid,
    result_revision: RunRevision,
    ordinary_sequence: i64,
}

async fn insert_refusal(
    tx: &mut Transaction<'_, Postgres>,
    candidate: &RefusalCandidate,
    audit: Option<i64>,
) -> Result<Value, sqlx::Error> {
    let c = &candidate.command;
    sqlx::query_scalar("INSERT INTO workflow_action_evidence_commands(company_id,command_key,id,run_id,
        execution_id,invocation_id,argument_digest,dispatch_id,actor_id,request_digest,expected_revision,
        result_revision,outcome,evidence_id,audit_sequence,scheduled_job_id,previous_state,previous_waiting_reason)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,NULL,$14,NULL,NULL,NULL)
        RETURNING to_jsonb(workflow_action_evidence_commands)")
        .bind(c.scope.company.as_uuid()).bind(c.command_key.as_str()).bind(candidate.id)
        .bind(c.scope.run.as_uuid()).bind(c.scope.execution.as_uuid()).bind(c.subject.invocation.as_uuid())
        .bind(c.subject.argument_digest.as_str()).bind(c.marker.as_uuid()).bind(c.actor.user_id())
        .bind(c.request_digest().unwrap().as_str()).bind(i64::try_from(c.expected_revision.0).unwrap())
        .bind(i64::try_from(candidate.result_revision.0).unwrap()).bind(json!(ReconciliationOutcome::RevisionConflict))
        .bind(audit).fetch_one(&mut **tx).await
}

fn assert_refusal_row(row: &Value, candidate: &RefusalCandidate) {
    let c = &candidate.command;
    assert_eq!(row.as_object().unwrap().len(), 19);
    for (field, value) in [
        ("company_id", json!(c.scope.company.as_uuid())),
        ("command_key", json!(c.command_key.as_str())),
        ("id", json!(candidate.id)),
        ("run_id", json!(c.scope.run.as_uuid())),
        ("execution_id", json!(c.scope.execution.as_uuid())),
        ("invocation_id", json!(c.subject.invocation.as_uuid())),
        ("argument_digest", json!(c.subject.argument_digest.as_str())),
        ("dispatch_id", json!(c.marker.as_uuid())),
        ("actor_id", json!(c.actor.user_id())),
        (
            "request_digest",
            json!(c.request_digest().unwrap().as_str()),
        ),
        ("expected_revision", json!(c.expected_revision.0)),
        ("result_revision", json!(candidate.result_revision.0)),
        ("outcome", json!(ReconciliationOutcome::RevisionConflict)),
    ] {
        assert_eq!(row[field], value, "{field}");
    }
    for field in [
        "evidence_id",
        "audit_sequence",
        "scheduled_job_id",
        "previous_state",
        "previous_waiting_reason",
    ] {
        assert!(row[field].is_null(), "{field}");
    }
    let timestamp: chrono::DateTime<chrono::Utc> =
        row["created_at"].as_str().unwrap().parse().unwrap();
    assert!(timestamp.timestamp() > 0);
}

async fn actual_stale_refusal(
    f: &AdmissionFixture,
    history: &CommittedAudit,
) -> ReconciliationResult {
    let mut stale = history.command.clone();
    stale.command_key = IdempotencyKey::parse("audit-sql-genuine-stale-refusal").unwrap();
    assert_ne!(
        stale.request_digest().unwrap(),
        history.command.request_digest().unwrap()
    );
    let before = all_tables(f).await;
    let result = Box::pin(run_command(f, &stale, history.verifier.clone()))
        .await
        .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::RevisionConflict);
    assert_eq!(result.revision, history.result.revision);
    assert_eq!(result.evidence, None);
    assert!(!result.replayed);
    let mut after = all_tables(f).await;
    let rows = after["workflow_action_evidence_commands"]
        .as_array_mut()
        .unwrap();
    let index = rows
        .iter()
        .position(|row| row["command_key"] == json!(stale.command_key.as_str()))
        .unwrap();
    let id = rows[index]["id"].as_str().unwrap().parse().unwrap();
    assert_refusal_row(
        &rows[index],
        &RefusalCandidate {
            command: stale,
            id,
            result_revision: result.revision,
            ordinary_sequence: 0,
        },
    );
    rows.remove(index);
    assert_eq!(
        after, before,
        "actual stale owner COMMIT appends only its audit-free refusal"
    );
    eprintln!(
        "service_COMMIT_refusal_positive outcome=RevisionConflict evidence=NULL audit=NULL job=NULL revision={}",
        result.revision.0
    );
    result
}

async fn refusal_candidate(f: &AdmissionFixture, history: &CommittedAudit) -> RefusalCandidate {
    let mut c = history.command.clone();
    c.command_key = IdempotencyKey::parse("audit-sql-explicit-fresh-stale-candidate").unwrap();
    c.input = EvidenceInput::UnknownNote {
        note: "fresh explicit SQL refused-shape candidate".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    assert_ne!(
        c.request_digest().unwrap(),
        history.command.request_digest().unwrap()
    );
    let sequence: i64 = sqlx::query_scalar("SELECT event.sequence FROM workflow_run_events AS event
        WHERE event.company_id=$1 AND event.run_id=$2 AND event.event_kind='admitted'
            AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command
                WHERE command.company_id=event.company_id AND command.run_id=event.run_id AND command.audit_sequence=event.sequence)")
        .bind(c.scope.company.as_uuid()).bind(c.scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    let revision = f
        .persistence()
        .head(c.scope.company, c.scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    assert!(revision.0 > c.expected_revision.0);
    let fresh: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands WHERE company_id=$1 AND command_key=$2)")
        .bind(c.scope.company.as_uuid()).bind(c.command_key.as_str()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(fresh);
    RefusalCandidate {
        command: c,
        id: Uuid::new_v4(),
        result_revision: revision,
        ordinary_sequence: sequence,
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_audit_sql_actual_refusal_linkage() {
    // Box real owner seams for stock 2 MiB stacks.
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let history = Box::pin(committed_unknown(&f, &request, verifier)).await;
    Box::pin(actual_stale_refusal(&f, &history)).await;
    let candidate = Box::pin(refusal_candidate(&f, &history)).await;
    let definition = "CHECK ((((evidence_id IS NULL) AND (audit_sequence IS NULL) AND (scheduled_job_id IS NULL)) OR ((evidence_id IS NOT NULL) AND (audit_sequence IS NOT NULL))))";
    let catalog = native_catalog(
        &f,
        REFUSAL_CHECK,
        "workflow_action_evidence_commands",
        definition,
    )
    .await;
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let baseline = all_tables(&f).await;
    for audit in [None, Some(candidate.ordinary_sequence)] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let owner = sql_owner(&mut tx).await;
        let started: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SAVEPOINT native_refusal_candidate")
            .execute(&mut *tx)
            .await
            .unwrap();
        if audit.is_none() {
            let row = insert_refusal(&mut tx, &candidate, audit).await.unwrap();
            assert_refusal_row(&row, &candidate);
            let finished: chrono::DateTime<chrono::Utc> =
                sqlx::query_scalar("SELECT clock_timestamp()")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            assert_clock_default(&row, started, finished);
            eprintln!(
                "RefusalAudit selected_native_SQL_savepoint_positive={row} no_owner_COMMIT_claim=true"
            );
        } else {
            let error = insert_refusal(&mut tx, &candidate, audit)
                .await
                .unwrap_err();
            native_error(
                &error,
                "23514",
                REFUSAL_CHECK,
                "workflow_action_evidence_commands",
            );
            eprintln!(
                "RefusalAudit ordinary_unowned_event_sequence={} candidate_id={} digest={}",
                candidate.ordinary_sequence,
                candidate.id,
                candidate.command.request_digest().unwrap().as_str()
            );
        }
        rolled_back(tx, &mut observer, &owner).await;
        assert_eq!(
            all_tables(&f).await,
            baseline,
            "refusal SQL rollback preserves every public row, provider and genuine refusal"
        );
    }
    assert_eq!(
        native_catalog(
            &f,
            REFUSAL_CHECK,
            "workflow_action_evidence_commands",
            definition
        )
        .await,
        catalog
    );
    assert_eq!(effects(&f).await, 0);
    drop(observer);
    f.persistence().pool().close().await;
}

fn assert_clock_default(
    row: &Value,
    started: chrono::DateTime<chrono::Utc>,
    finished: chrono::DateTime<chrono::Utc>,
) {
    let created: chrono::DateTime<chrono::Utc> =
        row["created_at"].as_str().unwrap().parse().unwrap();
    assert!(created >= started && created <= finished);
}
