use super::*;
use crate::{
    adapters::persistence::test_support::test_pool,
    entities::{
        attention::{AttentionView, BusinessPriority},
        creation::CreationProvenance,
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

async fn fixture(persistence: &PostgresPersistence) -> (Uuid, Uuid, PrincipalId) {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("attention-{suffix}@example.com");
    persistence
        .create_user(&format!("attention-{suffix}"), &email, "hash")
        .await
        .unwrap();
    let owner = UserPersistence::get_by_email(persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        persistence,
        owner.id,
        CompanyWrite {
            name: "Attention Test".into(),
            slug: format!("attention-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let agent = AgentPersistence::create(
        persistence,
        company.id,
        AgentWrite {
            name: "Attention Agent".into(),
            slug: format!("attention-agent-{suffix}"),
            created_by: Some(CreationProvenance::system()),
            ..AgentWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Attention".into(),
            slug: "attention".into(),
            agent_ids: Some(vec![agent.id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let principal = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
    )
    .bind(company.id)
    .bind(owner.id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    (company.id, channel.id, PrincipalId::new(principal))
}

fn query<'a>(
    company_id: Uuid,
    channel_ids: &'a [Uuid],
    principal_id: PrincipalId,
    view: AttentionView,
) -> AttentionQuery<'a> {
    AttentionQuery {
        company_id,
        principal_id,
        visible_channel_ids: channel_ids,
        view,
        cursor: None,
        limit: 50,
    }
}

#[tokio::test]
async fn competing_handoff_edits_are_fenced_and_resolution_reconciles() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, actor) = fixture(&persistence).await;
    let handoff_id = Uuid::new_v4();
    let create = NewManualHandoff {
        id: handoff_id,
        company_id,
        channel_id,
        thread_id: None,
        correlation_id: None,
        title: "Call the customer".into(),
        next_action: "Confirm the replacement address".into(),
        responsible_principal_id: None,
        priority: BusinessPriority::Normal,
        due_at: None,
        command_id: Uuid::new_v4(),
        actor_principal_id: actor,
    };
    let (create_first, create_retry) = tokio::join!(
        persistence.create_handoff(create.clone()),
        persistence.create_handoff(create)
    );
    assert_eq!(create_first.unwrap(), 1);
    assert_eq!(create_retry.unwrap(), 1);
    let create_event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attention_source_events WHERE source_id = $1 AND operation = 'created'",
    )
    .bind(handoff_id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert_eq!(create_event_count, 1);

    let visible = vec![channel_id];
    let base = AttentionSourceCommand {
        company_id,
        source_kind: AttentionSourceKind::Handoff,
        source_id: handoff_id,
        command_id: Uuid::new_v4(),
        expected_version: 1,
        actor_principal_id: actor,
        visible_channel_ids: visible.clone(),
        priority: BusinessPriority::High,
        due_at: Some(Utc::now() + chrono::Duration::hours(2)),
        responsible_principal_id: Some(actor),
    };
    let competing = AttentionSourceCommand {
        command_id: Uuid::new_v4(),
        priority: BusinessPriority::Urgent,
        ..base.clone()
    };
    let (first, second) = tokio::join!(
        persistence.change_source_attributes(base.clone()),
        persistence.change_source_attributes(competing.clone())
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    assert!(matches!(
        first.as_ref().err().or(second.as_ref().err()),
        Some(AppError::Conflict(_))
    ));

    let winning = if first.is_ok() { base } else { competing };
    assert_eq!(
        persistence.change_source_attributes(winning).await.unwrap(),
        2
    );
    let page = persistence
        .list_attention(query(company_id, &visible, actor, AttentionView::MyWork))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].source_id, handoff_id);
    assert_eq!(page.items[0].version, 2);

    persistence
        .resolve_handoff(ResolveHandoffCommand {
            company_id,
            handoff_id,
            command_id: Uuid::new_v4(),
            expected_version: 2,
            actor_principal_id: actor,
            visible_channel_ids: visible.clone(),
            resolution: HandoffResolution::Resolved,
        })
        .await
        .unwrap();
    assert!(
        persistence
            .list_attention(query(company_id, &visible, actor, AttentionView::MyWork))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn due_groups_precede_priority_and_cross_company_scope_is_empty() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, actor) = fixture(&persistence).await;
    for (title, priority, due_at) in [
        ("Urgent later", BusinessPriority::Urgent, None),
        (
            "Overdue normal",
            BusinessPriority::Normal,
            Some(Utc::now() - chrono::Duration::minutes(1)),
        ),
    ] {
        persistence
            .create_handoff(NewManualHandoff {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id: None,
                correlation_id: None,
                title: title.into(),
                next_action: "Act".into(),
                responsible_principal_id: None,
                priority,
                due_at,
                command_id: Uuid::new_v4(),
                actor_principal_id: actor,
            })
            .await
            .unwrap();
    }
    let visible = [channel_id];
    let page = persistence
        .list_attention(query(
            company_id,
            &visible,
            actor,
            AttentionView::Unassigned,
        ))
        .await
        .unwrap();
    assert_eq!(page.items[0].title, "Overdue normal");
    let summary = persistence
        .operational_summary(company_id, &visible)
        .await
        .unwrap();
    assert_eq!(summary.unassigned_count, 2);
    assert!(summary.oldest_actionable_age_seconds.is_some());
    assert!(
        persistence
            .list_attention(query(
                Uuid::new_v4(),
                &visible,
                actor,
                AttentionView::TeamWork,
            ))
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn cursor_freezes_due_buckets_and_removed_principals_release_handoffs() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, actor) = fixture(&persistence).await;
    let visible = [channel_id];
    for title in ["First", "Second"] {
        persistence
            .create_handoff(NewManualHandoff {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id: None,
                correlation_id: None,
                title: title.into(),
                next_action: "Act".into(),
                responsible_principal_id: Some(actor),
                priority: BusinessPriority::Normal,
                due_at: None,
                command_id: Uuid::new_v4(),
                actor_principal_id: actor,
            })
            .await
            .unwrap();
    }

    let first = persistence
        .list_attention(AttentionQuery {
            limit: 1,
            ..query(company_id, &visible, actor, AttentionView::MyWork)
        })
        .await
        .unwrap();
    let cursor: AttentionCursor = first.next_cursor.as_deref().unwrap().parse().unwrap();
    let second = persistence
        .list_attention(AttentionQuery {
            cursor: Some(&cursor),
            limit: 1,
            ..query(company_id, &visible, actor, AttentionView::MyWork)
        })
        .await
        .unwrap();
    assert_eq!(second.as_of, first.as_of);
    assert_eq!(second.items.len(), 1);
    assert_ne!(second.items[0].source_id, first.items[0].source_id);

    sqlx::query("DELETE FROM principals WHERE company_id = $1 AND id = $2")
        .bind(company_id)
        .bind(actor.as_uuid())
        .execute(persistence.pool())
        .await
        .unwrap();
    let released = persistence
        .list_attention(query(
            company_id,
            &visible,
            actor,
            AttentionView::Unassigned,
        ))
        .await
        .unwrap();
    assert_eq!(released.items.len(), 2);
    assert!(released.items.iter().all(|item| {
        matches!(item.responsibility, AttentionResponsibility::ChannelTeam) && item.version == 2
    }));
}

#[tokio::test]
async fn agent_work_stays_out_until_a_human_decision_is_required() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, actor) = fixture(&persistence).await;
    let task_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO background_tasks (
               id, company_id, channel_id, correlation_id, task_type, status, payload
           ) VALUES ($1, $2, $3, $4, 'reply', 'pending', '{}')"#,
    )
    .bind(task_id)
    .bind(company_id)
    .bind(channel_id)
    .bind(Uuid::new_v4())
    .execute(persistence.pool())
    .await
    .unwrap();
    let visible = [channel_id];
    assert!(
        persistence
            .list_attention(query(company_id, &visible, actor, AttentionView::TeamWork,))
            .await
            .unwrap()
            .items
            .is_empty()
    );

    let business_due = Utc::now() + chrono::Duration::hours(4);
    persistence
        .change_source_attributes(AttentionSourceCommand {
            company_id,
            source_kind: AttentionSourceKind::Task,
            source_id: task_id,
            command_id: Uuid::new_v4(),
            expected_version: 1,
            actor_principal_id: actor,
            visible_channel_ids: visible.to_vec(),
            priority: BusinessPriority::Urgent,
            due_at: Some(business_due),
            responsible_principal_id: None,
        })
        .await
        .unwrap();
    let outreach_id = Uuid::new_v4();
    let expiry = Utc::now() + chrono::Duration::hours(2);
    sqlx::query(
        r#"INSERT INTO task_outreaches (
               id, company_id, task_id, status, required_threshold_percent, expires_at,
               outreach_key, subject, body
           ) VALUES ($1, $2, $3, 'timeout_pending_approval', 100, $4, $5, 'Need input', 'Body')"#,
    )
    .bind(outreach_id)
    .bind(company_id)
    .bind(task_id)
    .bind(expiry)
    .bind(Uuid::new_v4().to_string())
    .execute(persistence.pool())
    .await
    .unwrap();

    let page = persistence
        .list_attention(query(
            company_id,
            &visible,
            actor,
            AttentionView::Unassigned,
        ))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let decision = &page.items[0];
    assert_eq!(
        decision.source_kind,
        AttentionSourceKind::DelegationDecision
    );
    assert_eq!(decision.source_id, outreach_id);
    assert_eq!(decision.priority, BusinessPriority::Urgent);
    assert_eq!(decision.due_at, Some(expiry));
    assert!(matches!(
        decision.responsibility,
        AttentionResponsibility::ChannelTeam
    ));
}
