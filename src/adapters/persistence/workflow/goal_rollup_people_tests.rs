//! `goal_rollup_` (Plan08 03D.1, gate 08G-P03-D2 r2): a goal's rollup lists its decisions waiting
//! on people, oldest first with the reviewers' names, and counts the time people kept its work
//! waiting, from the decision rows.
use super::decision_tests::DecisionFixture;
use super::*;
use crate::adapters::persistence::department::goal_rollup::goal_rollup_on;
use crate::application::ports::department::GOAL_ROLLUP_RUN_LIMIT;
use crate::application::workflow::{activation::*, decisions::*};
use crate::entities::department::DepartmentId;

#[tokio::test]
async fn goal_rollup_lists_decisions_waiting_on_people() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let p = f.persistence();
    let company = f.scope.company;
    let goal = p.test_unit(company.as_uuid(), "launch").await;
    sqlx::query("UPDATE departments SET kind='goal' WHERE id=$1")
        .bind(goal)
        .execute(p.pool())
        .await
        .unwrap();
    let desk = crate::adapters::persistence::test_support::create_channel(
        p,
        company.as_uuid(),
        crate::application::ports::channel::ChannelWrite {
            name: "Launch desk".into(),
            slug: "launch-desk".into(),
            department_id: Some(DepartmentId::new(goal)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Public, so the fixture's reviewers may see the desk's work.
    sqlx::query("UPDATE channels SET enabled=true, access_mode='public' WHERE id=$1")
        .bind(desk.id)
        .execute(p.pool())
        .await
        .unwrap();
    let mut request = f.admission.request("goal-wait", f.admission.manual());
    request.association =
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(desk.id));
    let command = f.admission.prepare(request).await;
    p.admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company,
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    p.park_decision(scope, &f.notices).await.unwrap().unwrap();
    // The fixture's own decision serves no goal and is not listed.
    p.park_decision(f.scope, &f.notices).await.unwrap().unwrap();
    let rollup = || async {
        let mut db = p.pool().acquire().await.unwrap();
        goal_rollup_on(
            &mut db,
            company,
            DepartmentId::new(goal),
            GOAL_ROLLUP_RUN_LIMIT,
        )
        .await
        .unwrap()
    };
    let (decision, since): (Uuid, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT id, created_at FROM workflow_decisions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let labels: Vec<String> = sqlx::query_scalar(
        "SELECT display_label FROM principals WHERE id = ANY($1) ORDER BY display_label",
    )
    .bind(&f.principals)
    .fetch_all(p.pool())
    .await
    .unwrap();
    let waiting = rollup().await;
    assert_eq!(waiting.waiting_total, 1);
    assert_eq!(waiting.waiting.len(), 1);
    let wait = &waiting.waiting[0];
    assert_eq!(
        (
            wait.run_id,
            wait.unit,
            wait.unit_name.as_deref(),
            wait.since
        ),
        (scope.run.as_uuid(), goal, Some("launch"), since)
    );
    assert_eq!(wait.reviewers, labels, "both people, by name");
    assert_eq!(waiting.units.len(), 1);
    assert_eq!(waiting.units[0].waiting, 1);
    let view = p
        .read_decision(
            company,
            WorkflowActor::authenticated(f.users[0]).unwrap(),
            DecisionId(decision),
        )
        .await
        .unwrap();
    p.submit_decision(f.submit(&view, 0, "goal-wait"))
        .await
        .unwrap();
    let submitted: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT submitted_at FROM workflow_decisions WHERE id=$1")
            .bind(decision)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let answered = rollup().await;
    assert!(answered.waiting.is_empty() && answered.waiting_total == 0);
    let kept = (submitted - since).num_milliseconds() as f64 / 1000.0;
    assert!(
        (answered.waiting_on_people_seconds as f64 - kept).abs() <= 1.0,
        "{} vs {kept}",
        answered.waiting_on_people_seconds
    );
    assert_eq!(
        answered.last_activity_at.map(|at| at >= submitted),
        Some(true)
    );

    // A decision whose run is cancelled unanswered stops counting when its wait ends
    // (08G-P03-B2-F3): two reads a second apart agree.
    let mut request = f
        .admission
        .request("goal-wait-cancelled", f.admission.manual());
    request.association =
        RelatedAssociation::Channel(crate::application::workflow::RelatedChannelId::new(desk.id));
    let command = f.admission.prepare(request).await;
    p.admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    let cancelled = ActivationRequest {
        company,
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    p.park_decision(cancelled, &f.notices)
        .await
        .unwrap()
        .unwrap();
    let revision = p
        .head(company, cancelled.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    assert!(matches!(
        p.cancel(crate::application::workflow::CancelCommand {
            company_id: company,
            actor: WorkflowActor::authenticated(f.users[0]).unwrap(),
            run_id: cancelled.run,
            command_key: crate::application::workflow::IdempotencyKey::parse("goal-wait-cancel")
                .unwrap(),
            expected_revision: revision,
        })
        .await
        .unwrap(),
        crate::application::workflow::CancelResult::Applied { .. }
    ));
    let settled: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT wait.settled_at FROM workflow_decisions AS decision JOIN workflow_waits AS wait \
           ON wait.company_id=decision.company_id AND wait.id=decision.wait_id \
          WHERE decision.run_id=$1",
    )
    .bind(cancelled.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(settled.is_some(), "cancelling ends the wait");
    let first = rollup().await;
    assert!(first.waiting.is_empty(), "an ended wait is not listed");
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let later = rollup().await;
    assert_eq!(
        first.waiting_on_people_seconds,
        later.waiting_on_people_seconds
    );
}
