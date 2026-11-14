//! Real run-tree retention must never turn an authentic private notice into ordinary fan-out.
use super::*;
use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};
#[path = "escalation_origin_guard_tests.rs"]
mod guards;
#[path = "escalation_origin_inheritance_tests.rs"]
mod inheritance;
#[path = "escalation_origin_message_tests.rs"]
mod messages;
#[path = "escalation_origin_race_tests.rs"]
mod races;
#[path = "escalation_origin_relay_tests.rs"]
mod relays;

#[derive(Clone, Copy)]
enum Stage {
    Pending,
    Claimed,
}

async fn retire(f: &AgentCase, run: RunId) {
    let finished_at = sqlx::query_scalar(
        "UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days', \
         finished_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2 RETURNING finished_at",
    ).bind(f.people.role.company().as_uuid()).bind(run.as_uuid())
        .fetch_one(f.people.role.p().pool()).await.unwrap();
    let result = f
        .people
        .role
        .p()
        .retire_tree(RetentionRoot {
            company: f.people.role.company(),
            run,
            finished_at,
        })
        .await
        .unwrap();
    assert_eq!(
        result.unwrap().runs,
        1,
        "the actual retention owner removed the source tree"
    );
}

async fn counts(f: &AgentCase) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object( \
        'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
        'cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=$1), \
        'receipts',(SELECT count(*) FROM workflow_escalation_agent_receipts WHERE company_id=$1), \
        'targets',(SELECT count(*) FROM workflow_escalation_notice_targets WHERE company_id=$1), \
        'roots',(SELECT count(*) FROM workflow_escalation_handling_roots WHERE company_id=$1), \
        'messages',(SELECT count(*) FROM workflow_message_notification_origins WHERE company_id=$1), \
        'origins',(SELECT count(*) FROM workflow_notification_origins WHERE company_id=$1), \
        'model_calls',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1))")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap()
}

async fn source_loss(stage: Stage, bindings: usize) {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    let extras = other_bindings(&f, bindings - 1).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let before = match stage {
        Stage::Pending => None,
        Stage::Claimed => Some(claims(&f).await.pop().unwrap()),
    };
    if let Some(claim) = &before {
        assert_eq!(p.candidate_bindings(claim).await.unwrap().len(), 1);
    }
    retire(&f, source).await;
    let claim = match before {
        Some(claim) => claim,
        None => claims(&f).await.pop().unwrap(),
    };
    assert!(p.candidate_bindings(&claim).await.unwrap().is_empty());
    for binding in extras {
        assert!(
            p.candidate_binding(&claim, binding)
                .await
                .unwrap()
                .is_none()
        );
    }
    process(&f, claim.clone()).await;
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM workflow_message_intents WHERE company_id=$1 AND id=$2 AND state='done'")
        .bind(claim.claim.company.as_uuid()).bind(claim.claim.intent).fetch_one(p.pool()).await.unwrap();
    assert_eq!(outcome, "notification_source_retired");
    assert_eq!(
        counts(&f).await,
        json!({"runs":0,"cases":0,"receipts":0,"targets":1,"roots":0,"messages":1,"origins":1,"model_calls":0})
    );
    let notice_live: bool = sqlx::query_scalar(
        "SELECT notice_id IS NOT NULL FROM workflow_notification_origins WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(!notice_live);
    // Exact queue replay cannot reopen a completed claim or invent a source case.
    assert!(!p.finish_intent(claim.claim, None).await.unwrap());
    assert!(claims(&f).await.is_empty());
    if bindings == 8 && matches!(stage, Stage::Pending) {
        ordinary(&f).await;
        for claim in claims(&f).await {
            process(&f, claim).await;
        }
        assert_eq!(
            counts(&f).await["runs"],
            8,
            "new authenticated work retains ordinary fan-out after source retention"
        );
    }
}

#[tokio::test]
async fn workflow_escalation_origin_actual_source_retention_closes_pending_and_claimed_private_intents_without_fanout()
 {
    for stage in [Stage::Pending, Stage::Claimed] {
        for bindings in [2, 8] {
            Box::pin(source_loss(stage, bindings)).await;
        }
    }
}

#[tokio::test]
async fn workflow_escalation_origin_former_root_retention_keeps_the_one_root_seal() {
    use crate::application::app_error::WorkflowGuardViolation;
    use crate::application::workflow::guard::{
        GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
    };
    let (f, _) = Box::pin(pending_agent_from_run()).await;
    other_bindings(&f, 7).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let claim = claims(&f).await.pop().unwrap();
    process(&f, claim.clone()).await;
    let run = RunId::new(
        sqlx::query_scalar(
            "SELECT run_id FROM workflow_escalation_handling_roots WHERE company_id=$1",
        )
        .bind(f.people.role.company().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap(),
    );
    let scope = f.people.scope(run).await;
    assert_eq!(
        p.fail_run_on_guard(GuardFailure {
            scope,
            violation: WorkflowGuardViolation::from_constraint(
                "workflow_guard:test_escalation_guard",
                "forced guard"
            )
            .unwrap(),
            site: GuardSite::Step,
            fence: None,
        })
        .await
        .unwrap(),
        GuardFailureOutcome::Failed
    );
    retire(&f, run).await;
    let historic: bool = sqlx::query_scalar("SELECT run_id=$2 AND live_run_id IS NULL FROM workflow_escalation_handling_roots WHERE company_id=$1")
        .bind(f.people.role.company().as_uuid()).bind(run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(historic);
    // Reoffer the same durable source through the real worker after its admission history
    // retired. The independent seal, rather than a surviving run/admission FK, must close it.
    sqlx::query("UPDATE workflow_message_intents SET state='pending',outcome=NULL,attempts=0,available_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(claim.claim.company.as_uuid()).bind(claim.claim.intent).execute(p.pool()).await.unwrap();
    let replay = claims(&f).await.pop().unwrap();
    assert!(p.candidate_bindings(&replay).await.unwrap().is_empty());
    process(&f, replay).await;
    assert_eq!(counts(&f).await["roots"], 1);
    assert_eq!(
        counts(&f).await["runs"],
        1,
        "only the original source run remains"
    );
    assert_eq!(counts(&f).await["cases"], 1);
}
