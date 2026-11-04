use super::*;
use crate::application::workflow::storage::WorkflowStorageMaintenance;

#[tokio::test]
async fn witness_sweep_all_three_tables_compete_without_removing_permanent_refusal() {
    let s = Box::pin(scheduled()).await;
    exhaust(&s).await;
    assert!(
        s.fixture
            .persistence()
            .settled()
            .claim_io(s.request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let tables = [
        "workflow_action_state_witnesses",
        "workflow_action_schedule_witnesses",
        "workflow_action_claim_retirement_witnesses",
    ];
    for table in tables {
        assert!(witness_count(&s, table).await > 0, "populated {table}");
    }
    let before = all_tables(&s.fixture).await;
    let p = s.fixture.persistence();
    let (left, right) = tokio::join!(p.sweep_obsolete_witnesses(1), p.sweep_obsolete_witnesses(1));
    assert!(left.unwrap() + right.unwrap() > 0);
    for _ in 0..10 {
        if p.sweep_obsolete_witnesses(1).await.unwrap() == 0 {
            break;
        }
    }
    for table in tables {
        assert_eq!(witness_count(&s, table).await, 0, "drained {table}");
    }
    let after = all_tables(&s.fixture).await;
    for table in [
        "workflow_action_claim_budget_refusals",
        "workflow_action_evidence",
        "workflow_action_evidence_commands",
        "workflow_action_remote_entries",
        "workflow_run_events",
    ] {
        assert_eq!(before[table], after[table], "retained permanent {table}");
    }
    assert_eq!(
        after["workflow_action_claim_budget_refusals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

async fn witness_count(s: &Scheduled, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(s.fixture.persistence().pool())
        .await
        .unwrap()
}
