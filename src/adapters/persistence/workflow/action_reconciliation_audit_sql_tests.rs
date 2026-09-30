//! Fresh public SQL candidates test immediate scope; owner COMMIT is a separate control.
use super::*;
use sqlx::postgres::PgDatabaseError;

#[path = "action_reconciliation_refusal_sql_tests.rs"]
mod refusal_sql_tests;

const EVENT_FK: &str = "workflow_run_event_execution_fk";
const REFUSAL_CHECK: &str = "workflow_action_evidence_commands_check";

struct CommittedAudit {
    command: ReconcileActionCommand,
    result: ReconciliationResult,
    verifier: Arc<LedgerVerifier>,
}

async fn committed_unknown(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
) -> CommittedAudit {
    let provider = LedgerProvider::new(f, Delivery::Pending);
    assert!(Box::pin(invoke(f, request, &provider)).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    park(f, request).await;
    let c = command(f, request, scoped_marker(f, request).await).await;
    let result = Box::pin(run_command(f, &c, verifier.clone()))
        .await
        .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(!result.replayed);
    assert!(result.revision.0 > c.expected_revision.0);
    let state = all_tables(f).await;
    assert_owner_audit(&state, &c, &result);
    eprintln!(
        "service_COMMIT_positive company={} run={} execution={} evidence={} revision={}",
        c.scope.company.as_uuid(),
        c.scope.run.as_uuid(),
        c.scope.execution.as_uuid(),
        result.evidence.unwrap().as_uuid(),
        result.revision.0
    );
    CommittedAudit {
        command: c,
        result,
        verifier,
    }
}

fn assert_owner_audit(state: &Value, c: &ReconcileActionCommand, result: &ReconciliationResult) {
    let rows = state["workflow_action_evidence_commands"]
        .as_array()
        .unwrap();
    let matches: Vec<_> = rows
        .iter()
        .filter(|row| {
            row["company_id"] == json!(c.scope.company.as_uuid())
                && row["command_key"] == json!(c.command_key.as_str())
        })
        .collect();
    assert_eq!(matches.len(), 1);
    let receipt = matches[0];
    let evidence = state["workflow_action_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(result.evidence.unwrap().as_uuid()))
        .unwrap();
    let audits: Vec<_> = state["workflow_run_events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["company_id"] == receipt["company_id"]
                && row["run_id"] == receipt["run_id"]
                && row["sequence"] == receipt["audit_sequence"]
        })
        .collect();
    assert_eq!(audits.len(), 1);
    for (field, value) in [
        ("company_id", json!(c.scope.company.as_uuid())),
        ("run_id", json!(c.scope.run.as_uuid())),
        ("execution_id", json!(c.scope.execution.as_uuid())),
        ("actor_id", json!(c.actor.user_id())),
    ] {
        assert_eq!(receipt[field], value);
        assert_eq!(evidence[field], value);
        assert_eq!(audits[0][field], value);
    }
    assert_eq!(audits[0]["event_kind"], "action_reconciled");
    assert_eq!(receipt["evidence_id"], evidence["id"]);
    assert_eq!(receipt["id"], evidence["command_id"]);
    assert_eq!(
        receipt["request_digest"],
        json!(c.request_digest().unwrap().as_str())
    );
    assert_eq!(receipt["request_digest"], evidence["request_digest"]);
    assert_eq!(receipt["result_revision"], json!(result.revision.0));
    assert_eq!(receipt["outcome"], json!(result.outcome));
    assert_eq!(evidence["dispatch_id"], json!(c.marker.as_uuid()));
    assert_eq!(
        evidence["invocation_id"],
        json!(c.subject.invocation.as_uuid())
    );
    assert_eq!(
        evidence["argument_digest"],
        json!(c.subject.argument_digest.as_str())
    );
}

async fn native_catalog(
    f: &AdmissionFixture,
    name: &str,
    relation: &str,
    definition: &str,
) -> Value {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('oid',constraint_row.oid::bigint,'name',constraint_row.conname,
            'type',constraint_row.contype::text,'relation',constraint_row.conrelid::regclass::text,
            'valid',constraint_row.convalidated,'deferrable',constraint_row.condeferrable,
            'deferred',constraint_row.condeferred,'definition',pg_get_constraintdef(constraint_row.oid),
            'enabled',NOT EXISTS(SELECT 1 FROM pg_trigger AS trigger_row
                WHERE trigger_row.tgconstraint=constraint_row.oid AND trigger_row.tgenabled<>'O'))
         FROM pg_constraint AS constraint_row JOIN pg_namespace AS namespace ON namespace.oid=constraint_row.connamespace
         WHERE namespace.nspname='public' AND constraint_row.conname=$1")
        .bind(name).fetch_all(f.persistence().pool()).await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["relation"], relation);
    assert_eq!(row["definition"], definition);
    for field in ["valid", "enabled"] {
        assert_eq!(row[field], true);
    }
    for field in ["deferrable", "deferred"] {
        assert_eq!(row[field], false);
    }
    assert_eq!(row["type"], if name == EVENT_FK { "f" } else { "c" });
    eprintln!("native_catalog={row}");
    row.clone()
}

struct SqlOwner {
    pid: i32,
    database: String,
}

async fn sql_owner(tx: &mut Transaction<'_, Postgres>) -> SqlOwner {
    let (pid, database): (i32, String) =
        sqlx::query_as("SELECT pg_backend_pid(),current_database()::text")
            .fetch_one(&mut **tx)
            .await
            .unwrap();
    SqlOwner { pid, database }
}

async fn rolled_back(
    tx: Transaction<'_, Postgres>,
    observer: &mut sqlx::pool::PoolConnection<Postgres>,
    owner: &SqlOwner,
) {
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **observer)
        .await
        .unwrap();
    assert_ne!(observer_pid, owner.pid);
    tx.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()")
                .execute(&mut **observer)
                .await
                .unwrap();
            let clean: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity
                WHERE activity.pid=$1 AND (activity.datname IS DISTINCT FROM $2
                    OR activity.state IS DISTINCT FROM 'idle' OR activity.xact_start IS NOT NULL))",
            )
            .bind(owner.pid)
            .bind(&owner.database)
            .fetch_one(&mut **observer)
            .await
            .unwrap();
            if clean {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("explicit SQL rollback must finish on its actual backend");
    eprintln!(
        "rollback_quiescent owner_pid={} observer_pid={observer_pid} database={}",
        owner.pid, owner.database
    );
}

fn native_error(error: &sqlx::Error, code: &str, constraint: &str, table: &str) {
    let native = error
        .as_database_error()
        .unwrap()
        .downcast_ref::<PgDatabaseError>();
    assert_eq!(native.code(), code);
    assert_eq!(native.constraint(), Some(constraint));
    assert_eq!(native.schema(), Some("public"));
    assert_eq!(native.table(), Some(table));
    let expected = if code == "23503" {
        format!(
            "insert or update on table \"{table}\" violates foreign key constraint \"{constraint}\""
        )
    } else {
        format!("new row for relation \"{table}\" violates check constraint \"{constraint}\"")
    };
    assert_eq!(native.message(), expected);
    eprintln!(
        "native_SQL_negative code={} constraint={:?} schema={:?} table={:?} message={} detail={:?}",
        native.code(),
        native.constraint(),
        native.schema(),
        native.table(),
        native.message(),
        native.detail()
    );
}

struct AuditCandidate {
    scope: ActionScope,
    actor: Uuid,
    sequence: i64,
}

async fn insert_audit(
    tx: &mut Transaction<'_, Postgres>,
    candidate: &AuditCandidate,
    execution: Uuid,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id)
        VALUES($1,$2,$3,'action_reconciled',$4,$5) RETURNING to_jsonb(workflow_run_events)")
        .bind(candidate.scope.company.as_uuid()).bind(candidate.scope.run.as_uuid()).bind(candidate.sequence)
        .bind(candidate.actor).bind(execution).fetch_one(&mut **tx).await
}

async fn audit_sql_pair(f: &AdmissionFixture, own: ActionScope, foreign: ActionScope, label: &str) {
    assert_ne!(own.execution, foreign.execution);
    assert_ne!(own.run, foreign.run);
    assert_eq!(own.company == foreign.company, label == "AuditRun");
    let definition = "FOREIGN KEY (company_id, run_id, execution_id) REFERENCES workflow_executions(company_id, run_id, id)";
    let catalog = native_catalog(f, EVENT_FK, "workflow_run_events", definition).await;
    let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0)+1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2")
        .bind(own.company.as_uuid()).bind(own.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    let candidate = AuditCandidate {
        scope: own,
        actor: f.binding.target.actor.user_id(),
        sequence,
    };
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let baseline = all_tables(f).await;
    for execution in [own.execution.as_uuid(), foreign.execution.as_uuid()] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let owner = sql_owner(&mut tx).await;
        sqlx::query("SAVEPOINT native_audit_candidate")
            .execute(&mut *tx)
            .await
            .unwrap();
        if execution == own.execution.as_uuid() {
            let row = insert_audit(&mut tx, &candidate, execution).await.unwrap();
            assert_eq!(row.as_object().unwrap().len(), 9);
            assert!(row["terminal_state"].is_null());
            assert!(row["terminal_execution_id"].is_null());
            for (field, expected) in [
                ("company_id", json!(own.company.as_uuid())),
                ("run_id", json!(own.run.as_uuid())),
                ("execution_id", json!(execution)),
                ("actor_id", json!(candidate.actor)),
                ("sequence", json!(sequence)),
                ("event_kind", json!("action_reconciled")),
            ] {
                assert_eq!(row[field], expected);
            }
            let now: String = sqlx::query_scalar("SELECT to_jsonb(CURRENT_TIMESTAMP)#>>'{}'")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(row["created_at"], now);
            eprintln!(
                "{label} selected_native_SQL_savepoint_positive={row} reverse_orphan_left_deferred=true"
            );
        } else {
            let error = insert_audit(&mut tx, &candidate, execution)
                .await
                .unwrap_err();
            native_error(&error, "23503", EVENT_FK, "workflow_run_events");
            let native = error
                .as_database_error()
                .unwrap()
                .downcast_ref::<PgDatabaseError>();
            let expected = format!(
                "Key (company_id, run_id, execution_id)=({}, {}, {}) is not present in table \"workflow_executions\".",
                own.company.as_uuid(),
                own.run.as_uuid(),
                execution
            );
            assert_eq!(native.detail(), Some(expected.as_str()));
            eprintln!(
                "{label} genuine_foreign_source company={} run={} execution={execution}",
                foreign.company.as_uuid(),
                foreign.run.as_uuid()
            );
        }
        rolled_back(tx, &mut observer, &owner).await;
        assert_eq!(
            all_tables(f).await,
            baseline,
            "{label}: every public table unchanged"
        );
    }
    assert_eq!(
        native_catalog(f, EVENT_FK, "workflow_run_events", definition).await,
        catalog
    );
}

async fn admitted_root(f: &AdmissionFixture) -> ActionScope {
    let c = f
        .prepare(f.request("audit-sql-other-root", f.manual()))
        .await;
    assert!(matches!(
        f.persistence().admit(&c).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    ActionScope {
        company: c.company_id(),
        run: c.proposed_run_id(),
        execution: c.first_execution_id(),
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_audit_sql_genuine_foreign_company() {
    // Box real admission/provider/service seams to retain stock 2 MiB stacks.
    let (a, a_request) = Box::pin(setup()).await;
    let a_verifier = ledger(&a, &a_request).await;
    let a_history = Box::pin(committed_unknown(&a, &a_request, a_verifier)).await;
    let (b, b_request) = Box::pin(foreign_action(&a)).await;
    let b_verifier = register_ledger(&b, &b_request).await;
    let b_history = Box::pin(committed_unknown(&b, &b_request, b_verifier)).await;
    let a_db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(a.persistence().pool())
        .await
        .unwrap();
    let b_db: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(b.persistence().pool())
        .await
        .unwrap();
    assert_eq!(a_db, b_db);
    assert_owner_audit(&all_tables(&a).await, &a_history.command, &a_history.result);
    assert_owner_audit(&all_tables(&a).await, &b_history.command, &b_history.result);
    Box::pin(audit_sql_pair(
        &b,
        b_request.scope(),
        a_request.scope(),
        "AuditCompany",
    ))
    .await;
    assert_eq!(effects(&a).await, 0);
    a.persistence().pool().close().await;
    b.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_audit_sql_genuine_foreign_run() {
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let history = Box::pin(committed_unknown(&f, &request, verifier)).await;
    let foreign = Box::pin(admitted_root(&f)).await;
    assert_owner_audit(&all_tables(&f).await, &history.command, &history.result);
    Box::pin(audit_sql_pair(&f, request.scope(), foreign, "AuditRun")).await;
    assert_eq!(effects(&f).await, 0);
    f.persistence().pool().close().await;
}
