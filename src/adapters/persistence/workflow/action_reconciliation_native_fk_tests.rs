//! Three immediate composite FK owners, each with orphan/foreign/commit/probe controls.
use super::super::super::super::super::sibling_tests::receipt_sql_tests::company_tests::{
    CompanyReceipt, applied_history, foreign_action,
};
use super::*;

#[path = "action_reconciliation_native_fk_assertions.rs"]
mod assertions;
use assertions::*;

#[derive(Clone, Copy, Debug)]
enum Target {
    Dispatch,
    Evidence,
    Entry,
}
#[derive(Clone, Copy, Debug)]
enum Cell {
    Orphan,
    Foreign,
    Positive,
    Accepted,
}
impl Cell {
    fn mode(self) -> &'static str {
        match self {
            Self::Orphan => "orphan",
            Self::Foreign => "foreign",
            Self::Positive => "positive",
            Self::Accepted => "accepted",
        }
    }
}
impl Target {
    fn mode(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Evidence => "evidence",
            Self::Entry => "entry",
        }
    }
    fn relation(self) -> &'static str {
        match self {
            Self::Dispatch => "workflow_action_evidence",
            _ => "workflow_action_evidence_coverage",
        }
    }
    fn reference(self) -> &'static str {
        match self {
            Self::Dispatch => "workflow_action_dispatches",
            Self::Evidence => "workflow_action_evidence",
            Self::Entry => "workflow_action_remote_entries",
        }
    }
    fn field(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch_id",
            Self::Evidence => "evidence_id",
            Self::Entry => "remote_entry_id",
        }
    }
    fn foreign(self, source: &CompanyReceipt) -> Uuid {
        match self {
            Self::Dispatch => source.marker,
            Self::Evidence => source.evidence,
            Self::Entry => source.entry,
        }
    }
}

async fn fk_catalog(source: &UnknownSource) -> Vec<Value> {
    sqlx::query_scalar(include_str!("action_reconciliation_native_fk_catalog.sql"))
        .fetch_all(source.fixture.persistence().pool())
        .await
        .unwrap()
}

async fn configure(
    source: &UnknownSource,
    command: &ReconcileActionCommand,
    target: Target,
    cell: Cell,
    replacement: Option<Uuid>,
) {
    for sql in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_fk_fixture.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(source.fixture.persistence().pool())
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO fixture_native_fk_config(company_id,command_key,target,mode,replacement) VALUES($1,$2,$3,$4,$5)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str()).bind(target.mode())
        .bind(cell.mode()).bind(replacement).execute(source.fixture.persistence().pool()).await.unwrap();
}

async fn replacement(
    source: &UnknownSource,
    foreign: &CompanyReceipt,
    target: Target,
    cell: Cell,
) -> Option<Uuid> {
    match cell {
        Cell::Foreign => Some(target.foreign(foreign)),
        Cell::Orphan => {
            let id = Uuid::new_v4();
            let absent: bool = sqlx::query_scalar(&format!(
                "SELECT NOT EXISTS(SELECT 1 FROM {} WHERE id=$1)",
                target.reference()
            ))
            .bind(id)
            .fetch_one(source.fixture.persistence().pool())
            .await
            .unwrap();
            assert!(absent, "orphan absent from entire target table");
            Some(id)
        }
        _ => None,
    }
}

async fn negative(
    source: &UnknownSource,
    foreign: &CompanyReceipt,
    command: &ReconcileActionCommand,
    target: Target,
    cell: Cell,
    before: &Value,
) {
    let catalog = fk_catalog(source).await;
    let owner = target_owner(&catalog, target);
    let mut observer = source.fixture.persistence().pool().acquire().await.unwrap();
    let error = Box::pin(run_command(
        &source.fixture,
        command,
        source.verifier.clone(),
    ))
    .await
    .unwrap_err();
    let (envelope, evidence) = transport(error, cell, target, owner);
    original_images(
        source,
        command,
        &owner_view(before, foreign),
        target,
        &envelope.original,
        &evidence,
    );
    assert_ne!(evidence["id"], json!(foreign.evidence));
    let mut expected = envelope.original.clone();
    if !matches!(cell, Cell::Accepted) {
        expected[target.field()] = before["fixture_native_fk_config"][0]["replacement"].clone();
    }
    assert_eq!(
        envelope.candidate, expected,
        "single-field substitution only"
    );
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        all_tables(&source.fixture).await,
        *before,
        "complete rollback including B effect and capture rows"
    );
}

async fn positive(
    source: &UnknownSource,
    foreign: &CompanyReceipt,
    command: &ReconcileActionCommand,
    target: Target,
    before: &Value,
) {
    let result = Box::pin(run_command(
        &source.fixture,
        command,
        source.verifier.clone(),
    ))
    .await
    .unwrap();
    assert_eq!(result.outcome, ReconciliationOutcome::UnknownRecorded);
    assert!(!result.replayed);
    let after = all_tables(&source.fixture).await;
    let image = one(&after, "fixture_native_fk_images");
    assert_eq!(image["target"], target.mode());
    assert_eq!(image["original"], image["candidate"]);
    let before_a = owner_view(before, foreign);
    original_images(
        source,
        command,
        &before_a,
        target,
        &image["original"],
        &image["evidence"],
    );
    assert_ne!(image["evidence"]["id"], json!(foreign.evidence));
    let mut after_a = owner_view(&after, foreign);
    after_a["fixture_native_fk_images"] = before_a["fixture_native_fk_images"].clone();
    assert_evidence(source, command, &result, &after_a);
    assert_commit(command, &result, &before_a, &after_a);
    assert!(one(&after_a, "workflow_action_evidence_commands")["scheduled_job_id"].is_null());
    // Restore only rows belonging to A and its capture; every B/global row must survive byte-for-byte.
    let mut restored = after.clone();
    for relation in [
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_coverage",
        "workflow_run_events",
        "workflow_runs",
        "fixture_native_fk_images",
    ] {
        let rows = restored[relation].as_array_mut().unwrap();
        rows.retain(|row| row["company_id"] != json!(command.scope.company.as_uuid()));
        rows.extend(
            before[relation]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["company_id"] == json!(command.scope.company.as_uuid()))
                .cloned(),
        );
        rows.sort_by_key(|row| row.to_string());
    }
    // all_tables orders jsonb text in PostgreSQL; compare each table as a set after exact row assertions.
    for (name, rows) in before.as_object().unwrap() {
        let mut expected = rows.as_array().unwrap().clone();
        expected.sort_by_key(|row| row.to_string());
        let mut actual = restored[name].as_array().unwrap().clone();
        actual.sort_by_key(|row| row.to_string());
        assert_eq!(actual, expected, "full preserved {name}");
    }
}

async fn exercise(target: Target, cell: Cell) {
    // Box real service/provider/fixture seams to retain stock 2 MiB stacks.
    let source = Box::pin(unknown_source()).await;
    let (foreign_fixture, request) = Box::pin(foreign_action(&source.fixture)).await;
    let verifier = register_ledger(&foreign_fixture, &request).await;
    let foreign = Box::pin(applied_history(foreign_fixture, request, verifier)).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.ledger,
        "native-fk",
    )
    .await;
    let initial = all_tables(&source.fixture).await;
    assert_sources(&source, &foreign, &initial);
    let db_a: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(source.fixture.persistence().pool())
        .await
        .unwrap();
    let db_b: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(foreign.fixture.persistence().pool())
        .await
        .unwrap();
    assert_eq!(db_a, db_b);
    let catalog = fk_catalog(&source).await;
    target_owner(&catalog, target);
    let native = native_catalog(&source.fixture).await;
    assert_native_owners(&catalog, &native);
    let replacement = replacement(&source, &foreign, target, cell).await;
    configure(&source, &command, target, cell, replacement).await;
    assert_eq!(production(native_catalog(&source.fixture).await), native);
    let before = all_tables(&source.fixture).await;
    assert_eq!(before["fixture_native_diagnostic_config"], json!([]));
    match cell {
        Cell::Positive => Box::pin(positive(&source, &foreign, &command, target, &before)).await,
        _ => Box::pin(negative(&source, &foreign, &command, target, cell, &before)).await,
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 1);
    assert_eq!(fk_catalog(&source).await, catalog);
    assert_eq!(production(native_catalog(&source.fixture).await), native);
    eprintln!(
        "native_fk target={target:?} cell={cell:?} genuine_foreign=true provider_calls=1 effects=1 ordinary_commit={}",
        matches!(cell, Cell::Positive)
    );
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_fk_dispatch() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Positive, Cell::Accepted] {
        Box::pin(exercise(Target::Dispatch, cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_fk_coverage_evidence() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Positive, Cell::Accepted] {
        Box::pin(exercise(Target::Evidence, cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_fk_coverage_entry() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Positive, Cell::Accepted] {
        Box::pin(exercise(Target::Entry, cell)).await;
    }
}

pub(super) async fn null_job_positive() {
    Box::pin(exercise(Target::Dispatch, Cell::Positive)).await;
}
