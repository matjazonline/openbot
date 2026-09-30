use super::*;

#[derive(Debug, PartialEq, sqlx::FromRow)]
pub(super) struct EntryAccounting {
    pub(super) entries: i64,
    pub(super) consumptions: i64,
    pub(super) requests: i64,
}

pub(super) async fn subject_counts(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
) -> EntryAccounting {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2) AS entries, (SELECT count(*) FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND invocation_id=$2) AS consumptions, (SELECT count(*) FROM fixture_evidence_ledger WHERE company_id=$1 AND invocation_id=$2) AS requests")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
        .fetch_one(f.persistence().pool()).await.unwrap()
}

pub(super) async fn available(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    excluded: Option<Uuid>,
) -> Option<Uuid> {
    sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2,$3)")
        .bind(request.scope().company.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(excluded)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

pub(super) async fn assert_subject(
    f: &AdmissionFixture,
    request: &ActionDispatchRequest,
    number: i32,
    status: &str,
) {
    assert_eq!(request.fence.attempt.0, number);
    let state: Value = sqlx::query_scalar("SELECT jsonb_build_object('job',to_jsonb(job),'run',to_jsonb(owner),'execution',to_jsonb(execution),'attempt',to_jsonb(attempt),'due',job.run_at<=clock_timestamp(),'live',job.lock_expires_at>clock_timestamp(),'episodes',(SELECT count(*) FROM workflow_action_claim_episodes WHERE company_id=job.company_id AND job_id=job.id),'attempts',(SELECT count(*) FROM task_attempts WHERE task_id=job.id),'executions',(SELECT count(*) FROM workflow_executions WHERE company_id=job.company_id AND run_id=owner.id)) FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id JOIN workflow_runs AS owner ON owner.company_id=execution.company_id AND owner.id=execution.run_id JOIN task_attempts AS attempt ON attempt.task_id=job.id AND attempt.attempt_number=$2 WHERE job.id=$1")
        .bind(request.fence.scope.job.0).bind(number).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(state["job"]["status"], status);
    assert_eq!(state["job"]["max_retries"], 132);
    assert_eq!(
        state["job"]["retry_count"],
        if status == "processing" {
            number - 1
        } else {
            number
        }
    );
    assert_eq!(state["attempts"], number);
    assert_eq!(state["episodes"], 0);
    assert_eq!(state["executions"], 1, "no successor is committed");
    assert_eq!(
        state["run"]["state"],
        if status == "failed" {
            "failed"
        } else {
            "running"
        }
    );
    for field in [
        "completed_at",
        "committed_output",
        "committed_route",
        "route_target",
        "successor_execution_id",
    ] {
        assert!(
            state["execution"]
                .get(field)
                .expect("real execution column")
                .is_null(),
            "execution {field} remains uncommitted"
        );
    }
    if status == "processing" {
        assert_eq!(state["live"], true);
        assert_eq!(state["attempt"]["status"], "processing");
        assert_eq!(state["attempt"]["worker_id"], json!(request.fence.worker.0));
        assert_eq!(
            state["attempt"]["execution_generation"],
            json!(request.fence.generation.0)
        );
        assert_eq!(state["job"]["worker_id"], json!(request.fence.worker.0));
        assert_eq!(
            state["job"]["execution_generation"],
            json!(request.fence.generation.0)
        );
    } else {
        assert_eq!(state["attempt"]["status"], "failed");
        assert_eq!(state["attempt"]["workflow_failure_class"], "terminal");
        assert_eq!(state["attempt"]["workflow_retry_safety"], "safe");
        assert_eq!(state["attempt"]["workflow_retirement"], "live");
        assert!(!state["attempt"]["finished_at"].is_null());
        for field in ["worker_id", "execution_generation", "lock_expires_at"] {
            assert!(state["job"][field].is_null());
        }
        if status == "pending" {
            assert_eq!(state["due"], true);
        }
    }
}

pub(super) fn reservation_delta(before: &Value, after: &Value) {
    assert_eq!(
        before.as_object().unwrap().keys().collect::<Vec<_>>(),
        after.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    for (relation, rows) in before.as_object().unwrap() {
        if relation == "workflow_action_remote_entries" {
            let current = after[relation].as_array().unwrap();
            assert_eq!(current.len(), rows.as_array().unwrap().len() + 1);
            for row in rows.as_array().unwrap() {
                assert!(current.contains(row));
            }
        } else {
            assert_eq!(
                rows, &after[relation],
                "single reservation preserves {relation}"
            );
        }
    }
}

pub(super) async fn clean_current(source: &HighSource) {
    let f = &source.fixture;
    let request = &source.request;
    assert_subject(f, request, 130, "processing").await;
    assert_eq!(
        subject_counts(f, request).await,
        EntryAccounting {
            entries: 129,
            consumptions: 0,
            requests: 128
        }
    );
    let valid: bool = sqlx::query_scalar("SELECT workflow_action_replay_supported($1,$2) AND NOT EXISTS(SELECT 1 FROM workflow_action_remote_entries AS entry JOIN task_attempts AS attempt ON attempt.task_id=entry.job_id AND attempt.attempt_number=entry.attempt_number WHERE entry.company_id=$1 AND entry.invocation_id=$2 AND attempt.status='processing') AND EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=$1 AND id=$3 AND grant_eligible AND valid_until>clock_timestamp()) AND (SELECT count(*) FROM workflow_action_evidence_coverage WHERE company_id=$1 AND evidence_id=$3)=128 AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence_coverage WHERE company_id=$1 AND evidence_id=$3 AND remote_entry_id=$4)")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(source.proof).bind(source.historical).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(
        valid,
        "live supported owner, all old entries retired, genuine unconsumed 128 coverage and uncovered129"
    );
}

pub(super) fn retry_delta(
    before: &Value,
    after: &Value,
    request: &ActionDispatchRequest,
    number: i32,
) {
    for table in [
        "task_attempts",
        "workflow_action_remote_entries",
        "workflow_action_dispatches",
        "workflow_action_evidence",
        "workflow_action_evidence_consumptions",
        "workflow_root_budgets",
        "workflow_root_budget_usage",
        "workflow_budget_receipts",
        "workflow_executions",
    ] {
        assert_eq!(
            before[table], after[table],
            "operator retry preserves {table}"
        );
    }
    let scoped = |state: &Value, table: &str, field: &str, id: Uuid| {
        state[table]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row[field] == json!(id))
            .unwrap()
            .clone()
    };
    let old_run = scoped(before, "workflow_runs", "id", request.scope().run.as_uuid());
    let new_run = scoped(after, "workflow_runs", "id", request.scope().run.as_uuid());
    for field in [
        "deadline",
        "max_steps",
        "max_context_bytes",
        "bundle",
        "input",
        "params",
        "resources",
    ] {
        assert_eq!(
            old_run[field], new_run[field],
            "operator retry preserves run {field}"
        );
    }
    let old_job = scoped(before, "background_tasks", "id", request.fence.scope.job.0);
    let new_job = scoped(after, "background_tasks", "id", request.fence.scope.job.0);
    for field in [
        "id",
        "workflow_execution_id",
        "retry_count",
        "max_retries",
        "payload",
    ] {
        assert_eq!(
            old_job[field], new_job[field],
            "operator retry preserves job {field}"
        );
    }
    retry_audit(before, after, request, number);
}

fn retry_audit(before: &Value, after: &Value, request: &ActionDispatchRequest, number: i32) {
    for table in ["workflow_control_commands", "workflow_run_events"] {
        assert_eq!(
            after[table].as_array().unwrap().len(),
            before[table].as_array().unwrap().len() + 1
        );
        for row in before[table].as_array().unwrap() {
            assert!(after[table].as_array().unwrap().contains(row));
        }
    }
    let key = format!("high-retry-{number}");
    let command = after["workflow_control_commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["command_key"] == key
                && row["company_id"] == json!(request.scope().company.as_uuid())
        })
        .unwrap();
    assert_eq!(command["operation"], "retry");
    assert_eq!(command["result"], "applied");
    assert_eq!(command["run_id"], json!(request.scope().run.as_uuid()));
    assert!(after["workflow_run_events"].as_array().unwrap().iter().any(
        |event| event["company_id"] == command["company_id"]
            && event["run_id"] == command["run_id"]
            && event["sequence"] == command["audit_sequence"]
            && event["event_kind"] == "control_retry"
    ));
}

struct NativeFunction {
    name: &'static str,
    migration: &'static str,
}

fn native_body(definition: &NativeFunction) -> &'static str {
    let start = definition
        .migration
        .find(&format!("FUNCTION {}(", definition.name))
        .unwrap();
    definition.migration[start..]
        .split_once("$$")
        .unwrap()
        .1
        .split_once("$$")
        .unwrap()
        .0
}

pub(super) async fn native_catalog(f: &AdmissionFixture) -> Value {
    let binding =
        include_str!("../../../../migrations/20260930181000_workflow_action_evidence_binding.sql");
    let bounds =
        include_str!("../../../../migrations/20261001090000_workflow_action_proof_bounds.sql");
    let evidence =
        include_str!("../../../../migrations/20260930180000_workflow_action_evidence.sql");
    for definition in [
        NativeFunction {
            name: "workflow_action_reservation_identity",
            migration: binding,
        },
        NativeFunction {
            name: "workflow_action_consumption_commit_guard",
            migration: binding,
        },
        NativeFunction {
            name: "workflow_action_not_applied_available",
            migration: bounds,
        },
        NativeFunction {
            name: "workflow_action_remote_entry_commit_guard",
            migration: evidence,
        },
    ] {
        let body: String = sqlx::query_scalar(
            "SELECT prosrc FROM pg_proc WHERE pronamespace='public'::regnamespace AND proname=$1",
        )
        .bind(definition.name)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(body, native_body(&definition), "native {}", definition.name);
    }
    let catalog: Value = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('name',tgname,'relation',tgrelid::regclass::text,'enabled',tgenabled,'type',tgtype,'deferred',tgdeferrable,'initially_deferred',tginitdeferred,'function',tgfoid::regproc::text,'definition',pg_get_triggerdef(oid)) ORDER BY tgname) FROM pg_trigger WHERE NOT tgisinternal AND tgname=ANY($1)")
        .bind(vec!["workflow_action_consumption_commit_guard", "workflow_action_consumption_reservation_identity", "workflow_action_entry_reservation_identity", "workflow_action_remote_entry_commit_guard"])
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(catalog.as_array().unwrap().len(), 4);
    for trigger in catalog.as_array().unwrap() {
        assert_eq!(trigger["enabled"], "O");
        let deferred = trigger["name"].as_str().unwrap().ends_with("commit_guard");
        assert_eq!(trigger["type"], if deferred { 5 } else { 7 });
        assert_eq!(trigger["deferred"], deferred);
        assert_eq!(trigger["initially_deferred"], deferred);
        assert_eq!(
            trigger["relation"],
            if trigger["name"].as_str().unwrap().contains("consumption") {
                "workflow_action_evidence_consumptions"
            } else {
                "workflow_action_remote_entries"
            }
        );
        assert_eq!(
            trigger["function"],
            if deferred {
                trigger["name"].as_str().unwrap()
            } else {
                "workflow_action_reservation_identity"
            }
        );
    }
    catalog
}
