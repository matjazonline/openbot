//! Five distinct ordinary-COMMIT failures after a complete valid Applied proposal.
use super::*;

#[derive(Clone, Copy, Debug)]
pub(in super::super) enum Position {
    Evidence,
    Receipt,
    Reopen,
    Audit,
    Command,
}
impl Position {
    pub(in super::super) fn name(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Receipt => "receipt",
            Self::Reopen => "reopen",
            Self::Audit => "audit",
            Self::Command => "command",
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Evidence => "workflow_action_evidence",
            Self::Receipt => "workflow_action_receipts",
            Self::Reopen => "background_tasks",
            Self::Audit => "workflow_run_events",
            Self::Command => "workflow_action_evidence_commands",
        }
    }

    fn operation(self) -> &'static str {
        match self {
            Self::Reopen => "UPDATE",
            _ => "INSERT",
        }
    }

    pub(in super::super) fn trigger(self) -> String {
        // Only enum-owned identifiers are interpolated; candidate values stay bound.
        format!(
            "CREATE CONSTRAINT TRIGGER fixture_commit_injected AFTER {} ON public.{} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fixture_commit_injected('{}')",
            self.operation(),
            self.table(),
            self.name()
        )
    }
}

pub(in super::super) fn assert_injection_catalog(
    before: &Value,
    after: &Value,
    position: Position,
) {
    let mut remaining = after.as_object().unwrap().clone();
    let functions: Vec<_> = remaining
        .iter()
        .filter(|(key, row)| {
            key.starts_with("function:") && row["name"] == "fixture_commit_injected"
        })
        .map(|(key, _)| key.clone())
        .collect();
    assert_eq!(functions.len(), 1);
    let function = remaining.remove(&functions[0]).unwrap();
    assert!(
        function["definition"]
            .as_str()
            .unwrap()
            .contains("RAISE EXCEPTION 'row3e injected Applied %'")
    );
    assert!(
        function["definition"]
            .as_str()
            .unwrap()
            .contains("RAISE EXCEPTION 'row3e injected Final %'")
    );
    let trigger_key = format!("trigger:{}.fixture_commit_injected", position.table());
    let trigger = remaining
        .remove(&trigger_key)
        .expect("one injection trigger");
    assert_eq!(trigger["enabled"], "O");
    assert_eq!(trigger["deferrable"], true);
    assert_eq!(trigger["deferred"], true);
    assert_eq!(trigger["definition"], position.trigger());
    let constraint_key = format!("constraint:{}.fixture_commit_injected", position.table());
    let constraint = remaining
        .remove(&constraint_key)
        .expect("one injection constraint");
    assert_eq!(constraint["deferrable"], true);
    assert_eq!(constraint["deferred"], true);
    assert_eq!(
        Value::Object(remaining),
        *before,
        "all native and observer definitions unchanged"
    );
}

async fn exercise_injected(position: Position) {
    // Box the genuine fixture seam to retain stock 2 MiB test stacks.
    let source = Box::pin(source()).await;
    let native = observer::catalog(&source.fixture).await;
    sqlx::raw_sql(include_str!("action_reconciliation_commit_fixture.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let observed = observer::catalog(&source.fixture).await;
    observer::assert_installation(&native, &observed);
    sqlx::raw_sql(include_str!("action_reconciliation_commit_injected.sql"))
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    sqlx::raw_sql(&position.trigger())
        .execute(source.fixture.persistence().pool())
        .await
        .unwrap();
    let installed = observer::catalog(&source.fixture).await;
    assert_injection_catalog(&observed, &installed, position);
    let mut gate = observer::Gate::hold(&source.fixture).await;
    let before = configure(&source, Case::Positive, &gate).await;
    let (result, observation) = gate
        .run(&source.fixture, &source.command, source.verifier.clone())
        .await;
    let after = snapshot(&source.fixture).await;
    assert!(
        observation.unwrap().is_some(),
        "complete proposal reached final gate: {result:?}"
    );
    let expected = format!(
        "error returned from database: row3e injected Applied {}",
        position.name()
    );
    assert!(
        matches!(result, Err(AppError::Database(ref error)) if error==&expected),
        "{position:?}: {result:?}"
    );
    assert_eq!(
        after, before,
        "{position:?}: every public row rolls back after exact backend quiescence"
    );
    assert_eq!(source.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(source.verifier.calls.load(Ordering::SeqCst), 1);
    assert!(source.verifier.issued.lock().unwrap().is_some());
    assert_eq!(effects(&source.fixture).await, 1);
    assert_eq!(observer::catalog(&source.fixture).await, installed);
    drop(gate);
    source.fixture.persistence().pool().close().await;
}

#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_injected_evidence() {
    Box::pin(exercise_injected(Position::Evidence)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_injected_receipt() {
    Box::pin(exercise_injected(Position::Receipt)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_injected_reopen() {
    Box::pin(exercise_injected(Position::Reopen)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_injected_audit() {
    Box::pin(exercise_injected(Position::Audit)).await;
}
#[tokio::test]
async fn workflow_action_reconciliation_commit_applied_injected_command() {
    Box::pin(exercise_injected(Position::Command)).await;
}
