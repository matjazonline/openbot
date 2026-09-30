//! Bounded native diagnostic transport and distinct-backend rollback observation.
use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticEnvelope {
    version: u32,
    probe_case: ReopenCase,
    company_id: Uuid,
    #[serde(deserialize_with = "decode_command_key")]
    command_key: IdempotencyKey,
    target_oid: i64,
    target_name: String,
    before_hits: u32,
    after_hits: u32,
    depth: u32,
    owner_pid: i32,
    database: String,
    first_execution: Uuid,
    first_job: Uuid,
    other_actor: Uuid,
    predicates: Value,
    original: Value,
    returned: Value,
    images_checked: bool,
    source_checked: bool,
    selected_executed: bool,
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

pub(super) fn decode(error: AppError, case: ReopenCase) -> DiagnosticEnvelope {
    let AppError::Database(message) = error else {
        panic!("ordinary Database error required: {error:?}")
    };
    eprintln!("actual_service_error case={case:?} {message}");
    let framed = message
        .strip_prefix("error returned from database: ")
        .expect("exact SQLx display wrapper");
    let json = framed
        .strip_prefix(case.prefix())
        .expect("case-specific native/accepted/mismatch frame");
    assert!(json.len() <= 65536, "bounded envelope");
    serde_json::from_str(json).expect("full parser consumption and deny unknown fields")
}

pub(super) fn assert_envelope(
    envelope: &DiagnosticEnvelope,
    owner: &FinalOwner,
    case: ReopenCase,
    row: &Value,
    database: &str,
) {
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.probe_case, case);
    assert_eq!(envelope.company_id, owner.candidate.scope.company.as_uuid());
    assert_eq!(envelope.command_key, owner.candidate.command_key);
    assert_eq!(envelope.target_oid, row["oid"].as_i64().unwrap());
    assert_eq!(envelope.target_name, row["name"].as_str().unwrap());
    assert_eq!(envelope.before_hits, 3);
    assert_eq!(envelope.after_hits, 1);
    assert_eq!(envelope.depth, 1);
    assert!(
        envelope.owner_pid > 0
            && envelope.images_checked
            && envelope.source_checked
            && envelope.selected_executed
    );
    assert_eq!(envelope.database, database);
    assert_eq!(envelope.first_execution, owner.first.execution.as_uuid());
    assert_eq!(envelope.first_job, owner.first.job.0);
    assert_eq!(envelope.other_actor, owner.other_actor);
    assert_final(&envelope.original, owner);
    assert_images(envelope, owner, case);
    assert_predicates(envelope, owner, case);
    assert_diagnostic(envelope, case);
}

struct ImageChange {
    relation: &'static str,
    field: &'static str,
    replacement: Value,
}
fn assert_images(envelope: &DiagnosticEnvelope, owner: &FinalOwner, case: ReopenCase) {
    let mut normalized = envelope.returned.clone();
    let change = match case {
        ReopenCase::Actor | ReopenCase::MismatchControl => Some(ImageChange {
            relation: "audit",
            field: "actor_id",
            replacement: json!(owner.other_actor),
        }),
        ReopenCase::Execution => Some(ImageChange {
            relation: "audit",
            field: "execution_id",
            replacement: json!(owner.first.execution.as_uuid()),
        }),
        ReopenCase::HistoricalRevision => Some(ImageChange {
            relation: "command",
            field: "result_revision",
            replacement: owner.history["command"]["result_revision"].clone(),
        }),
        ReopenCase::AcceptanceControl => None,
        ReopenCase::Positive => panic!("no positive envelope"),
    };
    if let Some(ImageChange {
        relation,
        field,
        replacement,
    }) = change
    {
        assert_eq!(normalized[relation][field], replacement);
        assert_ne!(
            normalized[relation][field],
            envelope.original[relation][field]
        );
        normalized[relation][field] = envelope.original[relation][field].clone();
    }
    assert_eq!(
        normalized, envelope.original,
        "single declared actual NEW field only"
    );
}
fn assert_predicates(envelope: &DiagnosticEnvelope, owner: &FinalOwner, case: ReopenCase) {
    let filters = envelope.predicates.as_array().unwrap();
    assert_eq!(filters.len(), if owner.history.is_null() { 1 } else { 2 });
    for filter in filters {
        assert_eq!(filter["other_filters"], true);
        assert_eq!(
            filter["actor_equal"],
            !matches!(case, ReopenCase::Actor | ReopenCase::MismatchControl)
        );
        assert_eq!(filter["execution_equal"], case != ReopenCase::Execution);
        assert_eq!(
            filter["revision_equal"],
            case != ReopenCase::HistoricalRevision
        );
    }
}

fn assert_diagnostic(envelope: &DiagnosticEnvelope, case: ReopenCase) {
    if case == ReopenCase::AcceptanceControl {
        assert!(
            envelope.native_state.is_none()
                && envelope.native_constraint.is_none()
                && envelope.native_message.is_none()
                && envelope.native_context.is_none()
                && envelope.native_schema.is_none()
                && envelope.native_table.is_none()
        );
        return;
    }
    assert_eq!(envelope.native_state.as_deref(), Some("23514"));
    assert_eq!(envelope.native_constraint.as_deref(), Some(""));
    assert_eq!(envelope.native_schema.as_deref(), Some(""));
    assert_eq!(envelope.native_table.as_deref(), Some(""));
    assert_eq!(
        envelope.native_message.as_deref(),
        Some("invalid explicit workflow retry")
    );
    let context = envelope.native_context.as_deref().unwrap();
    assert!(context.contains("check_workflow_control_retry()"));
    assert!(context.contains("SET CONSTRAINTS public.workflow_control_retry_guard IMMEDIATE"));
    if case == ReopenCase::MismatchControl {
        assert_ne!(
            envelope.native_message.as_deref(),
            Some("deliberately wrong reopen diagnostic")
        );
    }
}

pub(super) async fn wait_rollback(
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    envelope: &DiagnosticEnvelope,
    observer_pid: i32,
) {
    assert_ne!(observer_pid, envelope.owner_pid, "distinct held observer");
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut **observer).await.unwrap();
            let clean: bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=$1 AND (activity.datname IS DISTINCT FROM $2 OR activity.state IS DISTINCT FROM 'idle' OR activity.xact_start IS NOT NULL))")
                .bind(envelope.owner_pid).bind(&envelope.database).fetch_one(&mut **observer).await.unwrap();
            if clean { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("actual owner rollback quiescence");
    eprintln!(
        "rollback_quiescent owner_pid={} observer_pid={observer_pid} database={}",
        envelope.owner_pid, envelope.database
    );
}
