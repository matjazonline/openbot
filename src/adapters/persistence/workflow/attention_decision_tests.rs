//! p08 D4.2 against PostgreSQL: the work list's decisions and failed runs, over decisions the
//! production park writes.
use super::super::message_tests::support::member;
use super::decision_tests::DecisionFixture;
use super::*;
use crate::application::attention::AttentionPersistence;
use crate::application::workflow::decisions::WorkflowDecisions;
use crate::entities::attention::{
    AttentionItem, AttentionQuery, AttentionResponsibility, AttentionSourceKind, AttentionView,
};
use crate::entities::transport::PrincipalId;

async fn principal(p: &PostgresPersistence, company: Uuid, user: Uuid) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(user)
            .fetch_one(p.pool())
            .await
            .unwrap(),
    )
}

async fn listed(
    p: &PostgresPersistence,
    company: Uuid,
    viewer: PrincipalId,
    view: AttentionView,
    manager: bool,
) -> Vec<AttentionItem> {
    p.list_attention(AttentionQuery {
        company_id: company,
        principal_id: viewer,
        visible_channel_ids: &[],
        manager,
        view,
        cursor: None,
        limit: 100,
    })
    .await
    .unwrap()
    .items
}

/// A decision is in each reviewer's My work and in no other member's; Team work has it once with
/// its reviewer count; it is never unassigned; a company decision needs its reviewer to still
/// manage the company (the route passes `manager` from the current membership); a submission
/// takes it off every view.
#[tokio::test]
async fn attention_lists_a_decision_for_its_reviewers_and_once_in_team_work() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let view = f.park().await;
    let p = f.persistence();
    let company = f.scope.company.as_uuid();
    let owner = principal(p, company, f.admission.binding.target.actor.user_id()).await;
    let reviewers = [
        principal(p, company, f.users[0]).await,
        principal(p, company, f.users[1]).await,
    ];
    let bystander = principal(
        p,
        company,
        member(&f.admission.binding.fixture, "admin").await,
    )
    .await;

    for reviewer in reviewers {
        let mine = listed(p, company, reviewer, AttentionView::MyWork, true).await;
        assert_eq!(mine.len(), 1);
        let item = &mine[0];
        assert_eq!(
            (item.source_kind, item.source_id),
            (AttentionSourceKind::Decision, view.decision.0)
        );
        assert_eq!(item.run_id, Some(f.scope.run.as_uuid()));
        assert_eq!(item.channel_id, None);
        assert_eq!(
            item.responsibility,
            AttentionResponsibility::Principal(reviewer)
        );
        assert_eq!(item.due_at, Some(view.deadline));
        assert_eq!(
            item.href.as_deref(),
            Some(format!("/ui/workflow-decisions/{}", view.decision.0).as_str())
        );
        assert!(
            listed(p, company, reviewer, AttentionView::MyWork, false)
                .await
                .is_empty(),
            "a reviewer who no longer manages the company loses a company decision"
        );
    }
    for viewer in [owner, bystander] {
        assert!(
            listed(p, company, viewer, AttentionView::MyWork, true)
                .await
                .is_empty()
        );
        assert!(
            listed(p, company, viewer, AttentionView::Unassigned, true)
                .await
                .is_empty(),
            "a decision is never unassigned"
        );
    }
    let team = listed(p, company, owner, AttentionView::TeamWork, true).await;
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].source_id, view.decision.0);
    assert_eq!(team[0].responsibility_label, "2 reviewers");
    assert_eq!(team[0].responsibility, AttentionResponsibility::ChannelTeam);
    let summary = p.operational_summary(company, &[]).await.unwrap();
    assert_eq!(summary.open_decision_count, 1);
    assert!(summary.oldest_open_decision_age_seconds.is_some());

    p.submit_decision(f.submit(&view, 0, "accept"))
        .await
        .unwrap();
    assert!(
        listed(p, company, reviewers[1], AttentionView::MyWork, true)
            .await
            .is_empty()
    );
    assert!(
        listed(p, company, owner, AttentionView::TeamWork, true)
            .await
            .is_empty()
    );
    assert_eq!(
        p.operational_summary(company, &[])
            .await
            .unwrap()
            .open_decision_count,
        0
    );
}

/// A failed run is its actor's work while they are a member, and leaves every view once it is
/// rerun.
#[tokio::test]
async fn attention_lists_a_failed_run_for_its_actor_until_it_is_rerun() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    // An unrenderable notice fails the park, and the run with it.
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices.clone().unrenderable())
            .await
            .unwrap()
            .is_none()
    );
    let p = f.persistence();
    let company = f.scope.company.as_uuid();
    let run = f.scope.run.as_uuid();
    let actor = principal(p, company, f.admission.binding.target.actor.user_id()).await;
    let reviewer = principal(p, company, f.users[0]).await;

    let mine = listed(p, company, actor, AttentionView::MyWork, true).await;
    assert_eq!(mine.len(), 1);
    assert_eq!(
        (mine[0].source_kind, mine[0].source_id, mine[0].run_id),
        (AttentionSourceKind::RunFailure, run, Some(run))
    );
    assert_eq!(
        mine[0].responsibility,
        AttentionResponsibility::Principal(actor)
    );
    assert_eq!(
        mine[0].href,
        Some(format!(
            "/ui/workflow-runs?company_id={company}&run_id={run}"
        ))
    );
    assert!(
        mine[0].next_action.contains("in the last 7 days"),
        "{}",
        mine[0].next_action
    );
    assert!(
        listed(p, company, reviewer, AttentionView::MyWork, true)
            .await
            .is_empty()
    );
    assert!(
        listed(p, company, actor, AttentionView::Unassigned, true)
            .await
            .is_empty(),
        "a member actor's failure is theirs, not unassigned"
    );
    assert_eq!(
        listed(p, company, reviewer, AttentionView::TeamWork, true)
            .await
            .len(),
        1
    );
    assert_eq!(
        p.operational_summary(company, &[])
            .await
            .unwrap()
            .failed_run_count,
        1
    );

    let mut rerun = f.admission.request("rerun", f.admission.manual());
    rerun.rerun_of = Some(f.scope.run);
    let command = f.admission.prepare(rerun).await;
    assert!(matches!(
        p.admit(&command).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    for view in [AttentionView::MyWork, AttentionView::TeamWork] {
        assert!(
            listed(p, company, actor, view, true)
                .await
                .iter()
                .all(|item| item.source_id != run),
            "a rerun supersedes the failure in {view:?}"
        );
    }
    assert_eq!(
        p.operational_summary(company, &[])
            .await
            .unwrap()
            .failed_run_count,
        0
    );
}
