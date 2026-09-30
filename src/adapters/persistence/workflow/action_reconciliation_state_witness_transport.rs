//! Typed bounded transport of a native diagnostic from the actual owner transaction.
use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    version: u32,
    source_case: Source,
    mode: String,
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
    #[serde(deserialize_with = "decode_command_key")]
    command_key: IdempotencyKey,
    target_oid: i64,
    target_name: String,
    before_hits: u32,
    after_hits: u32,
    depth: u32,
    owner_pid: i32,
    database: String,
    xid: String,
    images: Value,
    pending: Value,
    predicates: Value,
    native_state: Option<String>,
    native_constraint: Option<String>,
    native_message: Option<String>,
    native_context: Option<String>,
    native_schema: Option<String>,
    native_table: Option<String>,
}
fn decode_command_key<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<IdempotencyKey, D::Error> {
    IdempotencyKey::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
}
pub(super) fn decode(error: AppError, mode: &str) -> Envelope {
    let AppError::Database(message) = error else {
        panic!("ordinary database transport required: {error}")
    };
    eprintln!("actual_state_service_error mode={mode} {message}");
    let prefix = match mode {
        "native" => "FIXTURE_STATE_NATIVE_V1:",
        "acceptance-control" => "FIXTURE_STATE_ACCEPTED_V1:",
        "mismatch-control" => "FIXTURE_STATE_MISMATCH_V1:",
        _ => panic!("unknown transport mode"),
    };
    let json = message
        .strip_prefix("error returned from database: ")
        .unwrap()
        .strip_prefix(prefix)
        .expect("native/accepted/mismatch are distinct classifications");
    assert!(json.len() <= 65536);
    serde_json::from_str(json).expect("bounded versioned deny-unknown-fields transport")
}
pub(super) fn assert_envelope(
    e: &Envelope,
    owner: &WitnessOwner,
    mode: &str,
    target: &Value,
    database: &str,
) {
    assert_eq!(e.version, 1);
    assert_eq!(e.source_case, owner.source);
    assert_eq!(e.mode, mode);
    assert_eq!(e.company_id, owner.candidate.scope.company.as_uuid());
    assert_eq!(e.run_id, owner.candidate.scope.run.as_uuid());
    assert_eq!(e.execution_id, owner.candidate.scope.execution.as_uuid());
    assert_eq!(e.job_id, owner.request.fence.scope.job.0);
    assert_eq!(e.command_key, owner.candidate.command_key);
    assert_eq!(e.target_oid, target["oid"].as_i64().unwrap());
    assert_eq!(e.target_name, target["name"].as_str().unwrap());
    assert_eq!(e.database, database);
    assert_eq!((e.before_hits, e.after_hits, e.depth), (3, 1, 1));
    assert_eq!(e.pending["xid"], e.xid);
    assert!(e.owner_pid > 0 && e.xid.parse::<u64>().unwrap() > 0);
    assert_images(e, owner);
    assert_predicates(e, owner);
    assert_diagnostic(e, mode);
}
fn assert_images(e: &Envelope, owner: &WitnessOwner) {
    for relation in ["command", "evidence"] {
        for (field, value) in owner.expected[relation].as_object().unwrap() {
            assert_eq!(&e.images[relation]["original"][field], value);
        }
    }
    for relation in ["evidence", "audit", "command"] {
        assert_eq!(e.images[relation]["returned"], e.pending[relation]);
        if relation != "command" || owner.source == Source::Waiting {
            assert_eq!(
                e.images[relation]["original"],
                e.images[relation]["returned"]
            );
        }
    }
    if owner.source != Source::Waiting {
        let before = &e.pending["steps"]["before"];
        let first = &e.pending["steps"]["first"];
        let final_step = &e.pending["steps"]["final"];
        assert_eq!(before["states"], json!([]));
        assert_eq!(before["schedules"], json!([]));
        assert_eq!(before["run"]["state"], owner.source.state());
        assert_eq!(
            before["run"]["waiting_reason"],
            json!(owner.source.reason())
        );
        assert_eq!(first["states"][0]["initial_state"], owner.source.state());
        assert_eq!(
            first["states"][0]["initial_waiting_reason"],
            json!(owner.source.reason())
        );
        assert_eq!(first["states"], final_step["states"]);
        assert_ne!(before["run"]["revision"], final_step["run"]["revision"]);
        assert_eq!(
            e.images["command"]["original"]["result_revision"],
            before["run"]["revision"]
        );
        assert_eq!(
            e.pending["command"]["result_revision"],
            final_step["run"]["revision"]
        );
    }
}
fn assert_predicates(e: &Envelope, owner: &WitnessOwner) {
    for (name, value) in e.predicates["reopen"].as_object().unwrap() {
        assert_eq!(value, &json!(true), "{name}");
    }
    assert_eq!(e.predicates["ordinary_receipt"], false);
    assert!(e.predicates["ordinary_helper"].is_boolean());
    assert_eq!(
        e.predicates["witness_eligible"],
        owner.source == Source::Waiting
    );
    let candidates = e.predicates["candidates"].as_array().unwrap();
    assert_eq!(
        candidates.len(),
        if owner.source == Source::Failed { 2 } else { 1 }
    );
    for candidate in candidates {
        for field in ["scope", "identity", "audit", "labels"] {
            assert_eq!(candidate[field], true);
        }
        assert_eq!(
            candidate["revision_equal"],
            candidate["command_key"] == json!(owner.candidate.command_key.as_str())
        );
    }
    assert_eq!(
        e.pending["evidence"]["grant_eligible"],
        owner.source != Source::Failed
    );
    assert_eq!(e.pending["available_proof"]["grant_eligible"], true);
    if owner.source == Source::Failed {
        assert_ne!(
            e.pending["available_proof"]["id"],
            e.pending["evidence"]["id"]
        );
    }
}
fn assert_diagnostic(e: &Envelope, mode: &str) {
    if mode == "acceptance-control" {
        assert!(
            e.native_state.is_none()
                && e.native_constraint.is_none()
                && e.native_message.is_none()
                && e.native_context.is_none()
                && e.native_schema.is_none()
                && e.native_table.is_none()
        );
        return;
    }
    assert_eq!(e.native_state.as_deref(), Some("23514"));
    assert_eq!(e.native_constraint.as_deref(), Some(""));
    assert_eq!(e.native_schema.as_deref(), Some(""));
    assert_eq!(e.native_table.as_deref(), Some(""));
    assert_eq!(
        e.native_message.as_deref(),
        Some("invalid explicit workflow retry")
    );
    let context = e.native_context.as_deref().unwrap();
    assert!(context.contains("check_workflow_control_retry()"));
    assert!(context.contains("SET CONSTRAINTS public.workflow_control_retry_guard IMMEDIATE"));
    if mode == "mismatch-control" {
        assert_ne!(
            e.native_message.as_deref(),
            Some("deliberately wrong state witness diagnostic")
        );
    }
}
pub(super) async fn wait_rollback(
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    e: &Envelope,
    observer_pid: i32,
) {
    assert_ne!(observer_pid, e.owner_pid, "distinct held observer backend");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut **observer).await.unwrap();
            let clean: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND (datname IS DISTINCT FROM $2 OR state IS DISTINCT FROM 'idle' OR xact_start IS NOT NULL))")
                .bind(e.owner_pid).bind(&e.database).fetch_one(&mut **observer).await.unwrap();
            if clean { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("actual service owner full rollback and quiescence");
    eprintln!(
        "state_rollback_quiescent owner={} observer={observer_pid} database={}",
        e.owner_pid, e.database
    );
}
