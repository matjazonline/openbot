//! p08 D4.2 against PostgreSQL: the work list over a run waiting on reconciliation, its
//! working-set bound and its company scope. The decision, failure, refusal and delivery sources
//! are tested beside the fixtures that make them (`workflow::tests`).
use super::*;
use crate::adapters::persistence::workflow::reconciliation_fixture::{HttpForm, uncertain_http};

async fn principal(persistence: &PostgresPersistence, company: Uuid, user: Uuid) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(user)
            .fetch_one(persistence.pool())
            .await
            .unwrap(),
    )
}

fn query(
    company_id: Uuid,
    principal_id: PrincipalId,
    view: AttentionView,
    manager: bool,
) -> AttentionQuery<'static> {
    AttentionQuery {
        company_id,
        principal_id,
        visible_channel_ids: &[],
        manager,
        view,
        cursor: None,
        limit: 100,
    }
}

/// A run waiting on reconciliation is unassigned team work for a manager, nobody's own work, and
/// invisible to a member when the run has no channel.
#[tokio::test]
async fn a_run_awaiting_reconciliation_is_unassigned_team_work() {
    let f = Box::pin(uncertain_http(HttpForm::Slot)).await;
    let p = &f.persistence;
    let owner = principal(p, f.company, f.owner).await;
    let channel: Option<Uuid> =
        sqlx::query_scalar("SELECT channel_id FROM workflow_runs WHERE id = $1")
            .bind(f.run)
            .fetch_one(p.pool())
            .await
            .unwrap();

    for view in [AttentionView::Unassigned, AttentionView::TeamWork] {
        let items = p
            .list_attention(query(f.company, owner, view, true))
            .await
            .unwrap()
            .items;
        assert_eq!(items.len(), 1, "{view:?}");
        let item = &items[0];
        assert_eq!(item.source_kind, AttentionSourceKind::RunReconciliation);
        assert_eq!((item.source_id, item.run_id), (f.run, Some(f.run)));
        assert_eq!(item.channel_id, channel);
        assert_eq!(item.responsibility, AttentionResponsibility::ChannelTeam);
        assert_eq!(item.state, "waiting");
        assert!(item.due_at.is_some(), "the run deadline is when it is due");
        assert_eq!(
            item.href,
            Some(format!(
                "/ui/workflow-runs?company_id={}&run_id={}",
                f.company, f.run
            ))
        );
    }
    assert!(
        p.list_attention(query(f.company, owner, AttentionView::MyWork, true))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    if channel.is_none() {
        assert!(
            p.list_attention(query(f.company, owner, AttentionView::Unassigned, false))
                .await
                .unwrap()
                .items
                .is_empty(),
            "a company-associated run is only for owners and admins"
        );
    }

    let summary = p.operational_summary(f.company, &[]).await.unwrap();
    assert_eq!(summary.runs_awaiting_reconciliation, 1);
    assert_eq!(summary.open_decision_count, 0);
}

/// The working set is bounded and reported: a page past the bound says so, and another
/// company's id lists nothing of this one.
#[tokio::test]
async fn the_working_set_is_bounded_and_the_company_scoped() {
    let f = Box::pin(uncertain_http(HttpForm::Tool)).await;
    let p = &f.persistence;
    let owner = principal(p, f.company, f.owner).await;
    let team = query(f.company, owner, AttentionView::TeamWork, true);

    let within = p.attention_page(team, 1).await.unwrap();
    assert_eq!((within.items.len(), within.working_set_size), (1, 1));
    assert!(!within.truncated);
    let past = p.attention_page(team, 0).await.unwrap();
    assert_eq!(past.working_set_size, 1);
    assert!(past.truncated);

    let elsewhere = query(Uuid::new_v4(), owner, AttentionView::TeamWork, true);
    assert!(p.list_attention(elsewhere).await.unwrap().items.is_empty());
}
