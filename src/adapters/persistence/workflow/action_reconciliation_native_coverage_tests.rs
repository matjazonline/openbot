//! Selected deferred provenance owner: missing coverage, Applied membership, marker coverage.
use super::*;
#[path = "action_reconciliation_native_coverage_assertions.rs"]
mod coverage_assertions;
use coverage_assertions::*;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Missing,
    Membership,
    Marker,
}
impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Membership => "membership",
            Self::Marker => "marker",
        }
    }
    fn source(self) -> Cell {
        if self == Self::Missing {
            Cell::UnknownAccepted
        } else {
            Cell::RemoteAccepted
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Rejected,
    Accepted,
    Positive,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Accepted => "accepted",
            Self::Positive => "positive",
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Case {
    target: Target,
    mode: Mode,
}
impl Case {
    fn omitted(self) -> bool {
        self.mode == Mode::Rejected && self.target != Target::Marker
    }
}
async fn install_coverage(source: &Source, command: &ReconcileActionCommand, case: Case) {
    for sql in [
        include_str!("action_reconciliation_native_diagnostic_fixture.sql"),
        include_str!("action_reconciliation_native_coverage_fixture.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(source.fixture.persistence().pool())
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO fixture_native_coverage_config(company_id,command_key,mode,target) VALUES($1,$2,$3,$4)")
        .bind(command.scope.company.as_uuid()).bind(command.command_key.as_str())
        .bind(case.mode.name()).bind(case.target.name()).execute(source.fixture.persistence().pool()).await.unwrap();
}
async fn exercise_coverage(case: Case) {
    // Box the real provider/service seams to retain the stock 2 MiB libtest stack.
    let source = Box::pin(source(case.target.source())).await;
    let foreign = Box::pin(foreign(&source)).await;
    assert_database(&source, &foreign).await;
    let command = proof_command(
        &source.fixture,
        &source.request,
        &source.verifier.inner,
        case.target.name(),
    )
    .await;
    let native = native_catalog(&source.fixture).await;
    guard(&native);
    install_coverage(&source, &command, case).await;
    assert_eq!(
        coverage_catalog(native_catalog(&source.fixture).await),
        native
    );
    let before = all_tables(&source.fixture).await;
    assert_eq!(before["fixture_native_diagnostic_config"], json!([]));
    if case.mode == Mode::Positive {
        Box::pin(positive_coverage(&source, &command, case, &before)).await;
    } else {
        let mut observer = source.fixture.persistence().pool().acquire().await.unwrap();
        let error = Box::pin(run_command(
            &source.fixture,
            &command,
            source.verifier.clone(),
        ))
        .await
        .unwrap_err();
        let (envelope, staged) = coverage_transport(error, case);
        assertions::original(
            &source,
            &command,
            case.target.source(),
            &envelope.original,
            &before,
        );
        candidate_coverage(&envelope.original, &envelope.candidate, case, &staged);
        assert_staged(&command, case, &envelope.candidate, &before, &staged);
        wait_rollback(&mut observer, &envelope).await;
        assert_eq!(
            all_tables(&source.fixture).await,
            before,
            "complete public and fixture rollback"
        );
    }
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        effects(&source.fixture).await,
        1 + i64::from(case.target != Target::Missing)
    );
    assert_eq!(
        coverage_catalog(native_catalog(&source.fixture).await),
        native
    );
    eprintln!(
        "native_coverage target={} mode={} selected_flush=true ordinary_commit={}",
        case.target.name(),
        case.mode.name(),
        case.mode == Mode::Positive
    );
    source.fixture.persistence().pool().close().await;
}
async fn positive_coverage(
    source: &Source,
    command: &ReconcileActionCommand,
    case: Case,
    before: &Value,
) {
    let result = Box::pin(run_command(
        &source.fixture,
        command,
        source.verifier.clone(),
    ))
    .await
    .unwrap();
    assert!(!result.replayed);
    assert_eq!(
        result.outcome,
        if case.target == Target::Missing {
            ReconciliationOutcome::UnknownRecorded
        } else {
            ReconciliationOutcome::Scheduled { receipt_only: true }
        }
    );
    let after = all_tables(&source.fixture).await;
    let images = one(&after, "fixture_native_diagnostic_images");
    assert_eq!(images["original"], images["candidate"]);
    assert_eq!(images["depth"], 1);
    assertions::original(
        source,
        command,
        case.target.source(),
        &images["original"],
        before,
    );
    let staged = &one(&after, "fixture_native_coverage_staged")["proposal"];
    candidate_coverage(&images["original"], &images["candidate"], case, staged);
    assert_staged(command, case, &images["candidate"], before, staged);
    assert_committed(command, &result, case, before, &after, staged);
}
#[tokio::test]
async fn workflow_action_reconciliation_native_coverage_unknown_missing() {
    for mode in [Mode::Rejected, Mode::Accepted, Mode::Positive] {
        Box::pin(exercise_coverage(Case {
            target: Target::Missing,
            mode,
        }))
        .await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_coverage_applied_membership() {
    for mode in [Mode::Rejected, Mode::Accepted, Mode::Positive] {
        Box::pin(exercise_coverage(Case {
            target: Target::Membership,
            mode,
        }))
        .await;
    }
}
#[tokio::test]
async fn workflow_action_reconciliation_native_coverage_marker_nonempty() {
    for mode in [Mode::Rejected, Mode::Accepted] {
        Box::pin(exercise_coverage(Case {
            target: Target::Marker,
            mode,
        }))
        .await;
    }
}
