//! `goal_rollup_` (Plan08 03D.1, gate 08G-P03-D2 r2): a goal's rollup counts its runs with an
//! open escalation, as the attention queue defines one (waiting, owner waiting or manual).
use super::*;
use crate::adapters::persistence::department::goal_rollup::goal_rollup_on;
use crate::application::ports::department::GOAL_ROLLUP_RUN_LIMIT;

#[tokio::test]
async fn goal_rollup_counts_open_escalations() {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let goal = people.role.department;
    sqlx::query("UPDATE departments SET kind='goal' WHERE id=$1")
        .bind(goal.as_uuid())
        .execute(people.role.p().pool())
        .await
        .unwrap();
    let f = Box::pin(GuardFixture::admit(people, Some(policy()), None)).await;
    let company = f.scope.company;
    let rollup = || async {
        let mut db = f.p().pool().acquire().await.unwrap();
        goal_rollup_on(&mut db, company, goal, GOAL_ROLLUP_RUN_LIMIT)
            .await
            .unwrap()
    };
    assert_eq!(rollup().await.escalations_open, 0);
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    // The case is pending until routed: not yet open, as in the attention queue.
    assert_eq!(rollup().await.escalations_open, 0);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    let state: String = sqlx::query_scalar(
        "SELECT progress.state FROM workflow_escalation_progress AS progress \
           JOIN workflow_escalation_cases AS escalation ON escalation.company_id=progress.company_id \
            AND escalation.id=progress.case_id WHERE escalation.run_id=$1",
    )
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(state, "waiting");
    let open = rollup().await;
    assert_eq!(open.escalations_open, 1);
    assert_eq!(open.units.len(), 1);
    assert_eq!(
        (open.units[0].unit, open.units[0].failed),
        (goal.as_uuid(), 1)
    );
}
