//! Native failures through the ordinary service, with exact full owner rollback.
use super::*;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ProbeCase {
    OrphanCommand,
    ForeignCommand,
    OrphanEvidence,
    ForeignEvidence,
    EvidenceActor,
    CommandActor,
    AcceptanceControl,
    MismatchControl,
}
impl ProbeCase {
    fn key(self) -> &'static str {
        match self {
            Self::OrphanCommand => "orphan-command",
            Self::ForeignCommand => "foreign-command",
            Self::OrphanEvidence => "orphan-evidence",
            Self::ForeignEvidence => "foreign-evidence",
            Self::EvidenceActor => "evidence-actor",
            Self::CommandActor => "command-actor",
            Self::AcceptanceControl => "acceptance-control",
            Self::MismatchControl => "mismatch-control",
        }
    }
    fn target(self) -> FlushTarget {
        match self {
            Self::OrphanCommand | Self::ForeignCommand | Self::AcceptanceControl => {
                FlushTarget::EvidenceCommand
            }
            Self::OrphanEvidence | Self::ForeignEvidence => FlushTarget::CommandEvidence,
            Self::EvidenceActor => FlushTarget::EvidenceScope,
            Self::CommandActor | Self::MismatchControl => FlushTarget::CommandScope,
        }
    }
    fn field(self) -> Option<&'static str> {
        match self {
            Self::OrphanCommand | Self::ForeignCommand => Some("command_id"),
            Self::OrphanEvidence | Self::ForeignEvidence => Some("evidence_id"),
            Self::EvidenceActor | Self::CommandActor | Self::MismatchControl => Some("actor_id"),
            Self::AcceptanceControl => None,
        }
    }
    fn relation(self) -> &'static str {
        match self {
            Self::OrphanCommand | Self::ForeignCommand => "evidence",
            _ => "command",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::AcceptanceControl => "FIXTURE_COMMAND_PROBE_ACCEPTED_V1:",
            Self::MismatchControl => "FIXTURE_COMMAND_PROBE_MISMATCH_V1:",
            _ => "FIXTURE_COMMAND_PROBE_NATIVE_V1:",
        }
    }
}

fn decode_command_key<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<IdempotencyKey, D::Error> {
    IdempotencyKey::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeEnvelope {
    version: u32,
    probe_case: ProbeCase,
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
    replacement: Option<Uuid>,
    original: Value,
    returned: Value,
    changed_relation: String,
    changed_field: Option<String>,
    old_value: Value,
    new_value: Value,
    authentic_command: Uuid,
    authentic_evidence: Uuid,
    returned_command: Uuid,
    returned_evidence_reference: Uuid,
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

struct ForeignIdentity {
    company: Uuid,
    command: Uuid,
    evidence: Uuid,
    actor: Uuid,
}

async fn foreign_identity(foreign: &CompanyReceipt) -> ForeignIdentity {
    let readback: Value = sqlx::query_scalar("SELECT fixture_command_probe_pending($1,$2)")
        .bind(foreign.request.scope().company.as_uuid())
        .bind("company-applied-missing")
        .fetch_one(foreign.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(readback["evidence"]["id"], json!(foreign.evidence));
    assert_eq!(readback["command"]["evidence_id"], json!(foreign.evidence));
    assert_eq!(
        readback["evidence"]["command_id"],
        readback["command"]["id"]
    );
    ForeignIdentity {
        company: foreign.request.scope().company.as_uuid(),
        command: serde_json::from_value(readback["command"]["id"].clone()).unwrap(),
        evidence: foreign.evidence,
        actor: foreign.fixture.binding.fixture.target.actor.user_id(),
    }
}

fn replacement(case: ProbeCase, foreign: &ForeignIdentity) -> Option<Uuid> {
    match case {
        ProbeCase::OrphanCommand | ProbeCase::OrphanEvidence => Some(Uuid::new_v4()),
        ProbeCase::ForeignCommand => Some(foreign.command),
        ProbeCase::ForeignEvidence => Some(foreign.evidence),
        ProbeCase::EvidenceActor | ProbeCase::CommandActor | ProbeCase::MismatchControl => {
            Some(foreign.actor)
        }
        ProbeCase::AcceptanceControl => None,
    }
}

async fn configure_negative(
    f: &AdmissionFixture,
    candidate: &ReconcileActionCommand,
    case: ProbeCase,
    foreign: &ForeignIdentity,
    replacement: Option<Uuid>,
) {
    if matches!(case, ProbeCase::OrphanCommand | ProbeCase::OrphanEvidence) {
        let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_evidence_commands AS command WHERE command.id=$1) AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence AS evidence WHERE evidence.id=$1)")
            .bind(replacement).fetch_one(f.persistence().pool()).await.unwrap();
        assert!(
            absent,
            "globally absent reference before authentic owner invocation"
        );
    }
    sqlx::query("UPDATE fixture_command_probe_config SET probe_case=$3,replacement=$4,foreign_company=$5,foreign_command=$6,foreign_evidence=$7,foreign_actor=$8 WHERE company_id=$1 AND command_key=$2")
        .bind(candidate.scope.company.as_uuid()).bind(candidate.command_key.as_str())
        .bind(case.key()).bind(replacement).bind(foreign.company).bind(foreign.command)
        .bind(foreign.evidence).bind(foreign.actor).execute(f.persistence().pool()).await.unwrap();
}

fn transported(error: AppError, case: ProbeCase) -> ProbeEnvelope {
    let AppError::Database(message) = error else {
        panic!("ordinary Database error required: {error:?}")
    };
    eprintln!("actual_service_error case={case:?} {message}");
    let framed = message
        .strip_prefix("error returned from database: ")
        .expect("exact SQLx display wrapper");
    if matches!(
        case,
        ProbeCase::AcceptanceControl | ProbeCase::MismatchControl
    ) {
        assert!(
            !framed.starts_with("FIXTURE_COMMAND_PROBE_NATIVE_V1:"),
            "control cannot be native proof"
        );
    }
    let json = framed
        .strip_prefix(case.prefix())
        .expect("exact case-specific sentinel frame");
    assert!(json.len() <= 65536, "bounded diagnostic frame");
    serde_json::from_str(json)
        .expect("complete named envelope, no unknown fields or trailing structure")
}

fn assert_images(
    envelope: &ProbeEnvelope,
    case: ProbeCase,
    candidate: &ReconcileActionCommand,
    verifier: &LedgerVerifier,
    entry: Uuid,
    foreign: &ForeignIdentity,
    replacement: Option<Uuid>,
) {
    assert_source(&envelope.original, &expected(candidate, verifier, entry));
    assert_eq!(envelope.changed_relation, case.relation());
    assert_eq!(envelope.changed_field.as_deref(), case.field());
    assert_eq!(envelope.replacement, replacement);
    let mut canonical = envelope.returned.clone();
    if let Some(field) = case.field() {
        assert_ne!(
            envelope.original[case.relation()][field],
            json!(replacement.unwrap())
        );
        assert_eq!(
            envelope.returned[case.relation()][field],
            json!(replacement.unwrap())
        );
        assert_eq!(
            envelope.old_value,
            envelope.original[case.relation()][field]
        );
        assert_eq!(
            envelope.new_value,
            envelope.returned[case.relation()][field]
        );
        canonical[case.relation()][field] = envelope.original[case.relation()][field].clone();
    } else {
        assert!(envelope.old_value.is_null() && envelope.new_value.is_null());
    }
    assert_eq!(
        canonical, envelope.original,
        "exact one-field raw difference, unchanged complete companion and sources"
    );
    assert_eq!(
        json!(envelope.authentic_command),
        envelope.original["command"]["id"]
    );
    assert_eq!(
        json!(envelope.authentic_evidence),
        envelope.original["evidence"]["id"]
    );
    assert_eq!(envelope.returned_command, envelope.authentic_command);
    assert_eq!(
        json!(envelope.returned_evidence_reference),
        envelope.returned["command"]["evidence_id"]
    );
    assert_ne!(envelope.authentic_command, foreign.command);
    assert_ne!(envelope.authentic_evidence, foreign.evidence);
}

fn assert_diagnostic(envelope: &ProbeEnvelope, case: ProbeCase, row: &Value) {
    if case == ProbeCase::AcceptanceControl {
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
    let context = envelope.native_context.as_deref().unwrap();
    let name = row["name"].as_str().unwrap();
    let quoted = if name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        name.to_owned()
    } else {
        format!("\"{name}\"")
    };
    assert!(
        context.contains(&format!("SET CONSTRAINTS public.{quoted} IMMEDIATE")),
        "captured nested SET context: {context}"
    );
    if row["type"] == "f" {
        assert_eq!(envelope.native_state.as_deref(), Some("23503"));
        assert_eq!(envelope.native_constraint.as_deref(), Some(name));
        assert_eq!(
            envelope.native_message.as_deref(),
            Some(
                format!(
                    "insert or update on table \"{}\" violates foreign key constraint \"{name}\"",
                    row["relation"].as_str().unwrap()
                )
                .as_str()
            )
        );
        assert_eq!(envelope.native_schema.as_deref(), Some("public"));
        assert_eq!(envelope.native_table.as_deref(), row["relation"].as_str());
    } else {
        assert_eq!(envelope.native_state.as_deref(), Some("23514"));
        assert_eq!(envelope.native_constraint.as_deref(), Some(""));
        let message = if case == ProbeCase::EvidenceActor {
            "invalid workflow action evidence provenance"
        } else {
            "workflow action command evidence scope mismatch"
        };
        assert_eq!(envelope.native_message.as_deref(), Some(message));
        assert!(
            context.contains(&format!("{name}()")),
            "actual native guard context: {context}"
        );
        assert_eq!(envelope.native_schema.as_deref(), Some(""));
        assert_eq!(envelope.native_table.as_deref(), Some(""));
        if case == ProbeCase::MismatchControl {
            assert_ne!(message, "invalid workflow action evidence provenance");
        }
    }
}

async fn wait_rollback(
    observer: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    envelope: &ProbeEnvelope,
    observer_pid: i32,
) {
    assert_ne!(
        observer_pid, envelope.owner_pid,
        "hold a distinct observer throughout rollback observation"
    );
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            sqlx::query("SELECT pg_stat_clear_snapshot()").execute(&mut **observer).await.unwrap();
            let clean: bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE activity.pid=$1 AND (activity.datname IS DISTINCT FROM $2 OR activity.state IS DISTINCT FROM 'idle' OR activity.xact_start IS NOT NULL))")
                .bind(envelope.owner_pid).bind(&envelope.database).fetch_one(&mut **observer).await.unwrap();
            if clean { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("bounded actual owner rollback completion");
    eprintln!(
        "rollback_quiescent owner_pid={} observer_pid={observer_pid} database={}",
        envelope.owner_pid, envelope.database
    );
}

struct ProbeOwner {
    fixture: AdmissionFixture,
    verifier: Arc<LedgerVerifier>,
    provider: LedgerProvider,
    candidate: ReconcileActionCommand,
    entry: Uuid,
}

async fn owner(foreign: &CompanyReceipt, case: ProbeCase) -> ProbeOwner {
    // Box authentic provider and service owner seams to preserve stock 2 MiB stacks.
    let (f, request) = Box::pin(foreign_action(&foreign.fixture)).await;
    let verifier = register_ledger(&f, &request).await;
    let provider = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: false,
            lose: true,
        },
    );
    assert!(Box::pin(invoke(&f, &request, &provider)).await.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let marker = scoped_marker(&f, &request).await;
    let entry: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries WHERE company_id=$1 AND invocation_id=$2",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let mut candidate = command(&f, &request, marker).await;
    candidate.command_key = IdempotencyKey::parse(case.key()).unwrap();
    candidate.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration.id().clone(),
        reference: EvidenceRecordReference::parse(case.key()).unwrap(),
    };
    assert_distinct(foreign, &request, &candidate, entry);
    ProbeOwner {
        fixture: f,
        verifier,
        provider,
        candidate,
        entry,
    }
}

async fn negative(foreign: &CompanyReceipt, case: ProbeCase, catalog: &[Value]) {
    let ProbeOwner {
        fixture: f,
        verifier,
        provider,
        candidate,
        entry,
    } = Box::pin(owner(foreign, case)).await;
    let identities = foreign_identity(foreign).await;
    let replacement = replacement(case, &identities);
    let row = selected(catalog, case.target());
    configure(&f, &candidate, &verifier, entry, case.target(), row).await;
    configure_negative(&f, &candidate, case, &identities, replacement).await;
    let mut observer = f.persistence().pool().acquire().await.unwrap();
    let observer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut *observer)
        .await
        .unwrap();
    let before = all_tables(&f).await;
    let observed = Arc::new(ObservedVerifier::new(verifier.clone()));
    let error = Box::pin(run_command(&f, &candidate, observed.clone()))
        .await
        .expect_err("negative/control aborts ordinary owner");
    assert_eq!(observed.calls.load(Ordering::SeqCst), 1);
    let envelope = transported(error, case);
    assert_eq!(envelope.version, 1);
    assert_eq!(envelope.probe_case, case);
    assert_eq!(envelope.company_id, candidate.scope.company.as_uuid());
    assert_eq!(envelope.command_key, candidate.command_key);
    assert_eq!(envelope.target_oid, row["oid"].as_i64().unwrap());
    assert_eq!(envelope.target_name, row["name"].as_str().unwrap());
    assert_eq!(envelope.before_hits, 2);
    assert_eq!(envelope.after_hits, 1);
    assert_eq!(envelope.depth, 1);
    assert!(
        envelope.images_checked
            && envelope.source_checked
            && envelope.selected_executed
            && envelope.owner_pid > 0
    );
    assert_eq!(envelope.database, database);
    assert_images(
        &envelope,
        case,
        &candidate,
        &verifier,
        entry,
        &identities,
        replacement,
    );
    assert_diagnostic(&envelope, case, row);
    wait_rollback(&mut observer, &envelope, observer_pid).await;
    assert_eq!(
        all_tables(&f).await,
        before,
        "all public rows equal after actual owner rollback; no exclusions or cleanup"
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    eprintln!(
        "negative_complete case={case:?} full_public_equality=true verifier_calls=1 provider_calls=1"
    );
}

async fn assert_foreign_source(foreign: &CompanyReceipt, verifier: &LedgerVerifier) {
    let readback: Value = sqlx::query_scalar("SELECT fixture_command_probe_pending($1,$2)")
        .bind(foreign.request.scope().company.as_uuid())
        .bind("company-applied-missing")
        .fetch_one(foreign.fixture.persistence().pool())
        .await
        .unwrap();
    let mut candidate = command(&foreign.fixture, &foreign.request, foreign.marker).await;
    candidate.command_key = IdempotencyKey::parse("company-applied-missing").unwrap();
    candidate.expected_revision.0 = readback["command"]["expected_revision"].as_u64().unwrap();
    candidate.input = EvidenceInput::VerifiedReference {
        registration: verifier.registration.id().clone(),
        reference: EvidenceRecordReference::parse("company-applied-missing").unwrap(),
    };
    assert_source(&readback, &expected(&candidate, verifier, foreign.entry));
    assert_eq!(readback["evidence"]["id"], json!(foreign.evidence));
}

#[tokio::test]
async fn workflow_action_reconciliation_command_probe_native_negatives_and_controls() {
    // Existing authentic B owner/provider setup owns this disposable migrated database.
    let (f, request) = Box::pin(setup()).await;
    let verifier = ledger(&f, &request).await;
    let foreign = Box::pin(applied_history(f, request, verifier.clone())).await;
    let original = catalog(&foreign.fixture).await;
    sqlx::raw_sql(include_str!(
        "action_reconciliation_command_probe_fixture.sql"
    ))
    .execute(foreign.fixture.persistence().pool())
    .await
    .unwrap();
    assert_foreign_source(&foreign, &verifier).await;
    for case in [
        ProbeCase::OrphanCommand,
        ProbeCase::ForeignCommand,
        ProbeCase::OrphanEvidence,
        ProbeCase::ForeignEvidence,
        ProbeCase::EvidenceActor,
        ProbeCase::CommandActor,
        ProbeCase::AcceptanceControl,
        ProbeCase::MismatchControl,
    ] {
        Box::pin(negative(&foreign, case, &original)).await;
    }
    assert_eq!(effects(&foreign.fixture).await, 9);
    assert_eq!(
        catalog(&foreign.fixture).await,
        original,
        "native catalog/function bytes preserved"
    );
    foreign.fixture.persistence().pool().close().await;
}
