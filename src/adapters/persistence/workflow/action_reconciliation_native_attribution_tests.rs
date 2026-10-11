//! Native immediate attribution owners. Grammar is redundant with the earlier shape CHECK.
use super::super::super::super::super::sibling_tests::receipt_sql_tests::company_tests::{
    CompanyReceipt, applied_history, foreign_action,
};
use super::*;

#[path = "action_reconciliation_native_attribution_assertions.rs"]
mod assertions;
use assertions::*;

#[derive(Clone, Copy, Debug)]
enum Cell {
    Invalid,
    MissingEntry,
    MarkerEntry,
    UnattributedEntry,
    UnknownAttributed,
    FinalAttributed,
    Orphan,
    Foreign,
    MissingAttribution,
    NullWithEntry,
    RemoteAccepted,
    MarkerAccepted,
    UnattributedAccepted,
    UnknownAccepted,
    FinalAccepted,
}
impl Cell {
    fn mode(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::MissingEntry => "missing_entry",
            Self::MarkerEntry => "marker_entry",
            Self::UnattributedEntry => "unattributed_entry",
            Self::UnknownAttributed => "unknown_attributed",
            Self::FinalAttributed => "final_attributed",
            Self::Orphan => "orphan",
            Self::Foreign => "foreign",
            Self::MissingAttribution => "missing_attribution",
            Self::NullWithEntry => "null_with_entry",
            Self::RemoteAccepted => "remote_accepted",
            Self::MarkerAccepted => "marker_accepted",
            Self::UnattributedAccepted => "unattributed_accepted",
            Self::UnknownAccepted => "unknown_accepted",
            Self::FinalAccepted => "final_accepted",
        }
    }
    fn unknown(self) -> bool {
        matches!(self, Self::UnknownAttributed | Self::UnknownAccepted)
    }
    fn accepted(self) -> bool {
        matches!(
            self,
            Self::RemoteAccepted
                | Self::MarkerAccepted
                | Self::UnattributedAccepted
                | Self::UnknownAccepted
                | Self::FinalAccepted
        )
    }
}
struct AttributionVerifier {
    inner: Arc<LedgerVerifier>,
    calls: AtomicUsize,
    observation: Mutex<Option<Value>>,
}
#[async_trait]
impl ActionEvidenceVerifier for AttributionVerifier {
    fn registration(&self) -> &EvidenceVerifierRegistration {
        self.inner.registration()
    }
    async fn verify(
        &self,
        snapshot: &ReconciliationSnapshot,
        reference: &EvidenceRecordReference,
        cancellation: &CancellationToken,
    ) -> AppResult<EvidenceAttestation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let value = self.inner.verify(snapshot, reference, cancellation).await?;
        assert_eq!(snapshot.entries.len(), 1);
        let applied = match &value.disposition {
            VerifiedDisposition::Applied { request, .. } => {
                assert_eq!(
                    *request,
                    AppliedEvidenceRequest::RemoteEntry(snapshot.entries[0].id)
                );
                true
            }
            VerifiedDisposition::Unknown => false,
            _ => panic!("unexpected genuine disposition"),
        };
        assert!(self.observation.lock().unwrap().replace(json!({
            "applied": applied, "entry": snapshot.entries[0].id.as_uuid(),
            "coverage_digest": snapshot.coverage_digest.as_str(), "reference": value.authoritative_reference.as_str(),
            "observed_at": value.observed_at, "valid_until": value.valid_until, "diagnostic": value.diagnostic,
        })).is_none());
        Ok(value)
    }
}
struct Source {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    provider: LedgerProvider,
    verifier: Arc<AttributionVerifier>,
}
async fn source(cell: Cell) -> Source {
    // Box real provider/fixture seams so the matrix retains stock 2 MiB test stacks.
    let (fixture, request) = Box::pin(setup()).await;
    resources(&fixture).await;
    let inner = ledger(&fixture, &request).await;
    let delivery = if cell.unknown() {
        Delivery::Pending
    } else {
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        }
    };
    let provider = LedgerProvider::new(&fixture, delivery);
    let error = Box::pin(invoke(&fixture, &request, &provider))
        .await
        .err()
        .expect("actual provider timeout");
    let expected = if cell.unknown() {
        "provider request remains active"
    } else {
        "response lost after durable effect"
    };
    assert!(matches!(error, AppError::Timeout(ref message) if message == expected));
    park(&fixture, &request).await;
    Source {
        fixture,
        request,
        provider,
        verifier: Arc::new(AttributionVerifier {
            inner,
            calls: AtomicUsize::new(0),
            observation: Mutex::new(None),
        }),
    }
}
async fn catalog(source: &Source) -> Vec<Value> {
    sqlx::query_scalar(include_str!("action_reconciliation_native_fk_catalog.sql"))
        .fetch_all(source.fixture.persistence().pool())
        .await
        .unwrap()
}
async fn foreign(source: &Source) -> CompanyReceipt {
    let (fixture, request) = Box::pin(foreign_action(&source.fixture)).await;
    let verifier = register_ledger(&fixture, &request).await;
    Box::pin(applied_history(fixture, request, verifier)).await
}
async fn assert_database(source: &Source, foreign: &CompanyReceipt) {
    let mut names = Vec::new();
    for fixture in [&source.fixture, &foreign.fixture] {
        names.push(
            sqlx::query_scalar::<_, String>("SELECT current_database()")
                .fetch_one(fixture.persistence().pool())
                .await
                .unwrap(),
        );
    }
    assert_eq!(names[0], names[1]);
    let validated: bool = sqlx::query_scalar("SELECT count(*)=3 AND bool_and(convalidated) FROM pg_constraint WHERE conrelid='workflow_action_evidence'::regclass AND conname IN ('workflow_action_applied_request_shape','workflow_action_applied_request_scope','workflow_action_evidence_applied_request_check')")
        .fetch_one(source.fixture.persistence().pool()).await.unwrap();
    assert!(validated);
}
async fn install(source: &Source, command: &ReconcileActionCommand, cell: Cell, replacement: Uuid) {
    for sql in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_attribution_fixture.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(source.fixture.persistence().pool())
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO fixture_native_attribution_config(company_id,command_key,mode,replacement) VALUES($1,$2,$3,$4)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(cell.mode()).bind(replacement)
        .execute(source.fixture.persistence().pool()).await.unwrap();
}
async fn exercise(cell: Cell) {
    let source = Box::pin(source(cell)).await;
    let foreign = Box::pin(foreign(&source)).await;
    assert_database(&source, &foreign).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.inner,
        cell.mode(),
    )
    .await;
    let replacement = if matches!(cell, Cell::Foreign) {
        foreign.entry
    } else {
        Uuid::new_v4()
    };
    let initial = all_tables(&source.fixture).await;
    if !matches!(cell, Cell::Foreign) {
        assert!(
            initial["workflow_action_remote_entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["id"] != json!(replacement))
        );
    }
    let owners = catalog(&source).await;
    let native = native_catalog(&source.fixture).await;
    assert_owners(&owners, &native);
    install(&source, &command, cell, replacement).await;
    assert_eq!(production(native_catalog(&source.fixture).await), native);
    let before = all_tables(&source.fixture).await;
    assert_eq!(before["fixture_native_diagnostic_config"], json!([]));
    let mut observer = source.fixture.persistence().pool().acquire().await.unwrap();
    let error = Box::pin(run_command(
        &source.fixture,
        &command,
        source.verifier.clone(),
    ))
    .await
    .unwrap_err();
    let envelope = transport(error, cell);
    original(&source, &command, cell, &envelope.original, &before);
    candidate(cell, replacement, &envelope, &before, &foreign);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        before,
        "complete public and fixture rollback"
    );
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        effects(&source.fixture).await,
        1 + i64::from(!cell.unknown())
    );
    assert_eq!(catalog(&source).await, owners);
    assert_eq!(production(native_catalog(&source.fixture).await), native);
    eprintln!(
        "native_attribution cell={} immediate_accepted={} ordinary_commit=false",
        cell.mode(),
        cell.accepted()
    );
    drop(observer);
    source.fixture.persistence().pool().close().await;
}
#[tokio::test]
async fn workflow_action_reconciliation_native_attribution_shape() {
    for cell in [
        Cell::Invalid,
        Cell::MissingEntry,
        Cell::MarkerEntry,
        Cell::UnattributedEntry,
        Cell::UnknownAttributed,
        Cell::FinalAttributed,
    ] {
        Box::pin(exercise(cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_attribution_scope_and_before() {
    for cell in [
        Cell::Orphan,
        Cell::Foreign,
        Cell::MissingAttribution,
        Cell::NullWithEntry,
    ] {
        Box::pin(exercise(cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_attribution_accepted_calibrations() {
    for cell in [
        Cell::RemoteAccepted,
        Cell::MarkerAccepted,
        Cell::UnattributedAccepted,
        Cell::UnknownAccepted,
        Cell::FinalAccepted,
    ] {
        Box::pin(exercise(cell)).await;
    }
}

#[path = "action_reconciliation_native_coverage_tests.rs"]
mod native_coverage_tests;
