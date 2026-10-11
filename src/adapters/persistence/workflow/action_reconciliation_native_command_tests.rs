//! Immediate command FK/CHECK owners, separate from ordinary COMMIT controls.
use super::super::super::super::super::commit_tests::{observer as commit_observer, snapshot};
use super::super::super::super::super::sibling_tests::{
    commit_final_tests as final_source,
    receipt_sql_tests::company_tests::{CompanyReceipt, applied_history, foreign_action},
};
use super::*;

#[path = "action_reconciliation_native_command_assertions.rs"]
mod assertions;
use assertions::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Dispatch,
    Audit,
    Job,
    State,
    Reason,
    Kind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Orphan,
    Foreign,
    Accepted,
    Positive,
    Wrong,
    Null,
    Missing,
}
impl Target {
    fn field(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch_id",
            Self::Audit => "audit_sequence",
            Self::Job => "scheduled_job_id",
            Self::State => "previous_state",
            Self::Reason => "previous_waiting_reason",
            Self::Kind => "kind",
        }
    }
    fn fk(self) -> bool {
        matches!(self, Self::Dispatch | Self::Audit | Self::Job)
    }
}
impl Cell {
    fn mode(self) -> &'static str {
        match self {
            Self::Orphan => "orphan",
            Self::Foreign => "foreign",
            Self::Accepted => "accepted",
            Self::Positive => "positive",
            Self::Wrong => "wrong",
            Self::Null => "null",
            Self::Missing => "missing",
        }
    }
    fn accepted(self) -> bool {
        matches!(self, Self::Accepted | Self::Null | Self::Missing)
    }
}

async fn foreign(source: &final_source::Final) -> (CompanyReceipt, Arc<LedgerVerifier>) {
    let (fixture, request) = Box::pin(foreign_action(&source.fixture)).await;
    let verifier = register_ledger(&fixture, &request).await;
    (
        Box::pin(applied_history(fixture, request, verifier.clone())).await,
        verifier,
    )
}

async fn foreign_audit(
    source: &final_source::Final,
    foreign: &CompanyReceipt,
    verifier: Arc<LedgerVerifier>,
) -> i64 {
    let next: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0)+1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2")
        .bind(source.command.scope.company.as_uuid()).bind(source.command.scope.run.as_uuid())
        .fetch_one(source.fixture.persistence().pool()).await.unwrap();
    assert!(next < 16, "small fixture audit preparation bound");
    for index in 0..16 {
        let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2")
            .bind(foreign.request.scope().company.as_uuid()).bind(foreign.request.scope().run.as_uuid())
            .fetch_one(source.fixture.persistence().pool()).await.unwrap();
        if sequence > next {
            return sequence;
        }
        let key = format!("command-foreign-audit-{index}");
        let result = Box::pin(reconcile(
            &foreign.fixture,
            &foreign.request,
            foreign.marker,
            verifier.clone(),
            &key,
        ))
        .await;
        assert!(!result.replayed);
        assert!(!matches!(
            result.outcome,
            ReconciliationOutcome::Scheduled { .. }
        ));
        let current: i64 =
            sqlx::query_scalar("SELECT revision FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(foreign.request.scope().company.as_uuid())
                .bind(foreign.request.scope().run.as_uuid())
                .fetch_one(source.fixture.persistence().pool())
                .await
                .unwrap();
        assert_eq!(result.revision.0, current as u64);
    }
    panic!("foreign audit fixture failed its bounded preparation");
}

async fn replacement(
    source: &final_source::Final,
    foreign: &CompanyReceipt,
    target: Target,
    cell: Cell,
    verifier: Arc<LedgerVerifier>,
) -> Option<Value> {
    match cell {
        Cell::Accepted | Cell::Positive | Cell::Missing => None,
        Cell::Null => Some(Value::Null),
        Cell::Wrong => Some(json!(match target {
            Target::State => "running",
            Target::Reason => "decision",
            Target::Kind => "unknown",
            _ => panic!("CHECK field"),
        })),
        Cell::Foreign => Some(match target {
            Target::Dispatch => json!(foreign.marker),
            Target::Job => json!(foreign.request.fence.scope.job.0),
            Target::Audit => json!(foreign_audit(source, foreign, verifier).await),
            _ => panic!("FK field"),
        }),
        Cell::Orphan => {
            if target == Target::Audit {
                let absent: bool = sqlx::query_scalar(
                    "SELECT NOT EXISTS(SELECT 1 FROM workflow_run_events WHERE sequence=$1)",
                )
                .bind(i64::MAX)
                .fetch_one(source.fixture.persistence().pool())
                .await
                .unwrap();
                assert!(absent);
                Some(json!(i64::MAX))
            } else {
                let id = Uuid::new_v4();
                let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE id=$1) AND NOT EXISTS(SELECT 1 FROM background_tasks WHERE id=$1)")
                    .bind(id).fetch_one(source.fixture.persistence().pool()).await.unwrap();
                assert!(absent);
                Some(json!(id))
            }
        }
    }
}

async fn install_probe(
    source: &final_source::Final,
    target: Target,
    cell: Cell,
    replacement: Option<Value>,
) -> Value {
    final_source::install(source).await;
    let native = commit_observer::catalog(&source.fixture).await;
    sqlx::raw_sql(include_str!(
        "action_reconciliation_native_command_fixture.sql"
    ))
    .execute(source.fixture.persistence().pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO fixture_native_command_config(company_id,command_key,mode,target,replacement) VALUES($1,$2,$3,$4,$5)")
        .bind(source.command.scope.company.as_uuid()).bind(source.command.command_key.as_str())
        .bind(cell.mode()).bind(target.field()).bind(replacement).execute(source.fixture.persistence().pool()).await.unwrap();
    let installed = commit_observer::catalog(&source.fixture).await;
    assert_installation(&native, &installed);
    installed
}

async fn rollback(
    source: &final_source::Final,
    foreign: &CompanyReceipt,
    target: Target,
    cell: Cell,
    owner: &Value,
) {
    let before = snapshot(&source.fixture).await;
    let mut observer = source.fixture.persistence().pool().acquire().await.unwrap();
    let error = Box::pin(run_command(
        &source.fixture,
        &source.command,
        source.verifier.clone(),
    ))
    .await
    .unwrap_err();
    let (envelope, linked) = transport(error, target, cell, owner);
    original(source, foreign, &before, &envelope, &linked, target, cell);
    wait_rollback(&mut observer, &envelope).await;
    assert_eq!(
        snapshot(&source.fixture).await,
        before,
        "every public row rolls back"
    );
}

async fn committed(source: &final_source::Final) {
    let mut gate = commit_observer::Gate::hold(&source.fixture).await;
    final_source::configure(source, final_source::Case::Positive, &gate).await;
    let (result, observation) = gate
        .run(&source.fixture, &source.command, source.verifier.clone())
        .await;
    final_source::positive(
        source,
        &snapshot(&source.fixture).await,
        result.unwrap(),
        &observation.unwrap().expect("complete scheduled proposal"),
    );
}

async fn exercise(target: Target, cell: Cell) {
    // Box the real sibling/provider/service seams for stock 2 MiB libtest threads.
    let source = Box::pin(final_source::source()).await;
    let (foreign, verifier) = Box::pin(foreign(&source)).await;
    let replacement = replacement(&source, &foreign, target, cell, verifier).await;
    let catalog: Vec<Value> =
        sqlx::query_scalar(include_str!("action_reconciliation_native_fk_catalog.sql"))
            .fetch_all(source.fixture.persistence().pool())
            .await
            .unwrap();
    let owner = owner(&catalog, target);
    let native = install_probe(&source, target, cell, replacement).await;
    assert_native_order(&native, &catalog);
    if cell == Cell::Positive {
        Box::pin(committed(&source)).await;
    } else {
        Box::pin(rollback(&source, &foreign, target, cell, owner)).await;
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&source.fixture).await, 2);
    assert_eq!(commit_observer::catalog(&source.fixture).await, native);
    let after_catalog: Vec<Value> =
        sqlx::query_scalar(include_str!("action_reconciliation_native_fk_catalog.sql"))
            .fetch_all(source.fixture.persistence().pool())
            .await
            .unwrap();
    assert_eq!(after_catalog, catalog);
    eprintln!(
        "native_command target={target:?} cell={cell:?} ordinary_commit={}",
        cell == Cell::Positive
    );
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_native_command_dispatch() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Accepted, Cell::Positive] {
        Box::pin(exercise(Target::Dispatch, cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_command_audit() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Accepted, Cell::Positive] {
        Box::pin(exercise(Target::Audit, cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_command_job() {
    for cell in [Cell::Orphan, Cell::Foreign, Cell::Accepted, Cell::Positive] {
        Box::pin(exercise(Target::Job, cell)).await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_command_scheduled_check() {
    for target in [Target::State, Target::Reason, Target::Kind] {
        Box::pin(exercise(target, Cell::Wrong)).await;
    }
    for cell in [Cell::Accepted, Cell::Positive] {
        Box::pin(exercise(Target::Kind, cell)).await;
    }
    Box::pin(super::native_fk_tests::null_job_positive()).await;
    eprintln!("native_command target=NullJob cell=Positive ordinary_commit=true");
}
#[tokio::test]
async fn workflow_action_reconciliation_native_command_nullable_calibrations() {
    for target in [Target::State, Target::Reason, Target::Kind] {
        Box::pin(exercise(target, Cell::Null)).await;
    }
    Box::pin(exercise(Target::Kind, Cell::Missing)).await;
}
