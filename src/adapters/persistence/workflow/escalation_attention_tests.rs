//! Open administrative decisions share the actual work feed and operational count.
use super::*;
use crate::application::attention::AttentionPersistence;
use crate::entities::attention::{
    AttentionItem, AttentionQuery, AttentionSourceKind, AttentionView,
};

async fn listed(
    f: &GuardFixture,
    viewer: PrincipalId,
    view: AttentionView,
    channels: &[Uuid],
) -> Vec<AttentionItem> {
    f.p()
        .list_attention(AttentionQuery {
            company_id: f.scope.company.as_uuid(),
            principal_id: viewer,
            visible_channel_ids: channels,
            manager: false,
            company_items: false,
            view,
            cursor: None,
            limit: 100,
        })
        .await
        .unwrap()
        .items
        .into_iter()
        .filter(|item| item.source_kind == AttentionSourceKind::EscalationDecision)
        .collect()
}

async fn owner(f: &GuardFixture) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar(
            "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.people.role.admission.binding.target.actor.user_id())
        .fetch_one(f.p().pool())
        .await
        .unwrap(),
    )
}

#[tokio::test]
async fn workflow_escalation_attention_lists_only_current_assignments_and_removes_settled_work() {
    let (f, decision) = Box::pin(pending(2)).await;
    let channels = [f.people.role.channel];
    let owner = owner(&f).await;
    for reviewer in &f.people.principals {
        let mine = listed(&f, *reviewer, AttentionView::MyWork, &channels).await;
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].source_id, decision.as_uuid());
        assert_eq!(
            mine[0].href.as_deref(),
            Some(format!("/ui/workflow-decisions/escalations/{decision}").as_str())
        );
    }
    assert!(
        listed(&f, owner, AttentionView::MyWork, &channels)
            .await
            .is_empty()
    );
    assert_eq!(
        listed(&f, owner, AttentionView::TeamWork, &channels)
            .await
            .len(),
        1
    );
    assert!(
        listed(&f, owner, AttentionView::Unassigned, &channels)
            .await
            .is_empty()
    );
    assert!(
        listed(&f, owner, AttentionView::TeamWork, &[])
            .await
            .is_empty()
    );
    let foreign = PrincipalId::new(Uuid::new_v4());
    assert!(
        listed(&f, foreign, AttentionView::TeamWork, &channels)
            .await
            .is_empty()
    );
    assert_eq!(
        f.p()
            .operational_summary(f.scope.company.as_uuid(), &channels)
            .await
            .unwrap()
            .open_decision_count,
        1
    );
    let (target_user, target) = eligible_member(&f).await;
    f.p()
        .submit_escalation(command(
            &f,
            decision,
            f.people.users[0],
            "feed-reassign",
            EscalationChoice::Reassign { principal: target },
        ))
        .await
        .unwrap();
    assert!(
        listed(&f, f.people.principals[0], AttentionView::MyWork, &channels)
            .await
            .is_empty()
    );
    let next = listed(&f, target, AttentionView::MyWork, &channels).await;
    assert_eq!(next.len(), 1);
    assert_ne!(next[0].source_id, decision.as_uuid());
    f.p()
        .submit_escalation(command(
            &f,
            EscalationDecisionId::new(next[0].source_id),
            target_user,
            "feed-ack",
            EscalationChoice::Acknowledge,
        ))
        .await
        .unwrap();
    assert!(
        listed(&f, target, AttentionView::MyWork, &channels)
            .await
            .is_empty()
    );
    assert!(
        listed(&f, owner, AttentionView::TeamWork, &channels)
            .await
            .is_empty()
    );
    assert_eq!(
        f.p()
            .operational_summary(f.scope.company.as_uuid(), &channels)
            .await
            .unwrap()
            .open_decision_count,
        0
    );
}

#[tokio::test]
async fn workflow_escalation_attention_actual_retirement_hands_waiting_work_to_the_company_owner() {
    let (f, _) = Box::pin(pending(2)).await;
    let channels = [f.people.role.channel];
    let owner_principal = owner(&f).await;
    for user in &f.people.users {
        f.p()
            .remove_member(f.scope.company.as_uuid(), *user)
            .await
            .unwrap();
    }
    for _ in 0..4 {
        if f.worker().drain_once().await.unwrap() == 0 {
            break;
        }
    }
    let waiting = listed(&f, owner_principal, AttentionView::MyWork, &channels).await;
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].state, "owner_waiting");
    let team = listed(&f, owner_principal, AttentionView::TeamWork, &channels).await;
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].responsibility_label, "Company owner");
    let (other_user, other) = eligible_member(&f).await;
    assert!(
        listed(&f, other, AttentionView::MyWork, &channels)
            .await
            .is_empty()
    );
    let owner_user = f.people.role.admission.binding.target.actor.user_id();
    assert_ne!(owner_user, other_user);
    f.p()
        .submit_escalation(command(
            &f,
            EscalationDecisionId::new(waiting[0].source_id),
            owner_user,
            "owner-feed-ack",
            EscalationChoice::Acknowledge,
        ))
        .await
        .unwrap();
    assert!(
        listed(&f, owner_principal, AttentionView::MyWork, &channels)
            .await
            .is_empty()
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
}
