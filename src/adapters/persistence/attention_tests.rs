use super::*;
use crate::{
    adapters::persistence::test_support::{DeliveryFixtureRequest, delivery_fixture, test_pool},
    application::task_queue::TaskPersistence,
    entities::{
        attention::{AttentionView, BusinessPriority},
        creation::CreationProvenance,
        task::{ResumeActor, TaskOwner, TaskStatus},
    },
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
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
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
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

#[tokio::test]
async fn all_owned_includes_completed_tasks_but_preserves_scope() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, owner) = fixture(&persistence).await;
    let thread = persistence
        .create_thread(channel_id, "Owned tasks", &[])
        .await
        .unwrap();
    let task_id =
        insert_approval_task(&persistence, company_id, channel_id, thread.id, owner).await;
    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE id = $1")
        .bind(task_id)
        .execute(persistence.pool())
        .await
        .unwrap();
    let channels = [channel_id];
    let mut request = query(company_id, &channels, owner, AttentionView::MyWork);
    assert!(
        persistence
            .list_attention(request)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    request.all_owned = true;
    let page = persistence.list_attention(request).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].source_id, task_id);
    assert_eq!(page.items[0].state, "completed");

    request.principal_id = PrincipalId::new(Uuid::new_v4());
    assert!(
        persistence
            .list_attention(request)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    request.principal_id = owner;
    request.visible_channel_ids = &[];
    assert!(
        persistence
            .list_attention(request)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    request.visible_channel_ids = &channels;
    request.company_id = Uuid::new_v4();
    assert!(
        persistence
            .list_attention(request)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    request.company_id = company_id;
    request.view = AttentionView::TeamWork;
    assert!(
        persistence
            .list_attention(request)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn pending_approvals_belong_to_the_approver_without_duplicating_the_task() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, task_owner) = fixture(&persistence).await;
    let reviewer_principal = add_teammate(&persistence, company_id, "Approval Reviewer").await;

    let thread = persistence
        .create_thread(channel_id, "Approval responsibility", &[])
        .await
        .unwrap();
    let internal_task =
        insert_approval_task(&persistence, company_id, channel_id, thread.id, task_owner).await;
    let external_task =
        insert_approval_task(&persistence, company_id, channel_id, thread.id, task_owner).await;
    let internal_approval = insert_pending_approval(
        &persistence,
        company_id,
        channel_id,
        thread.id,
        internal_task,
        "internal@example.com",
        Some(reviewer_principal),
    )
    .await;
    let external_approval = insert_pending_approval(
        &persistence,
        company_id,
        channel_id,
        thread.id,
        external_task,
        "external@example.net",
        None,
    )
    .await;
    let visible = [channel_id];

    let reviewer_work = persistence
        .list_attention(query(
            company_id,
            &visible,
            reviewer_principal,
            AttentionView::MyWork,
        ))
        .await
        .unwrap();
    assert_eq!(reviewer_work.items.len(), 1);
    assert_eq!(reviewer_work.items[0].source_id, internal_approval);
    assert_eq!(
        reviewer_work.items[0].source_kind,
        AttentionSourceKind::Approval
    );
    assert_eq!(
        reviewer_work.items[0].responsibility,
        AttentionResponsibility::Principal(reviewer_principal)
    );

    assert!(
        persistence
            .list_attention(query(
                company_id,
                &visible,
                task_owner,
                AttentionView::MyWork,
            ))
            .await
            .unwrap()
            .items
            .is_empty(),
        "the task owner must not receive a duplicate approval card"
    );
    assert!(
        persistence
            .list_attention(query(
                company_id,
                &visible,
                task_owner,
                AttentionView::Unassigned,
            ))
            .await
            .unwrap()
            .items
            .is_empty(),
        "an email-only approver is external, not unassigned channel work"
    );

    let team_work = persistence
        .list_attention(query(
            company_id,
            &visible,
            task_owner,
            AttentionView::TeamWork,
        ))
        .await
        .unwrap();
    assert_eq!(team_work.items.len(), 2);
    let external = team_work
        .items
        .iter()
        .find(|item| item.source_id == external_approval)
        .unwrap();
    assert_eq!(external.responsibility, AttentionResponsibility::External);
    assert_eq!(
        persistence
            .operational_summary(company_id, &visible)
            .await
            .unwrap()
            .unassigned_count,
        0,
        "external approvals must not inflate the unassigned channel-team count"
    );

    sqlx::query("UPDATE human_approvals SET status = 'approved' WHERE company_id = $1 AND id = $2")
        .bind(company_id)
        .bind(internal_approval)
        .execute(persistence.pool())
        .await
        .unwrap();
    let resumed = persistence
        .resume_task(internal_task, ResumeActor::Approval(internal_approval))
        .await
        .unwrap();
    assert_eq!(resumed.status, TaskStatus::Pending);
    assert_eq!(
        resumed.ownership.owner,
        TaskOwner::Human(task_owner),
        "approval resolution must not transfer the underlying task"
    );
    let owner_work = persistence
        .list_attention(query(
            company_id,
            &visible,
            task_owner,
            AttentionView::MyWork,
        ))
        .await
        .unwrap();
    assert_eq!(owner_work.items.len(), 1);
    assert_eq!(owner_work.items[0].source_kind, AttentionSourceKind::Task);
    assert_eq!(owner_work.items[0].source_id, internal_task);
}

#[tokio::test]
async fn approval_assignee_cannot_cross_the_company_boundary() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let (company_id, channel_id, _) = fixture(&persistence).await;
    let (_, _, foreign_principal) = fixture(&persistence).await;
    let thread = persistence
        .create_thread(channel_id, "Tenant-scoped approver", &[])
        .await
        .unwrap();

    let result = sqlx::query(
        r#"INSERT INTO human_approvals (
               id, company_id, channel_id, thread_id, step_key, approver_email,
               approver_principal_id, action_type, action_title, action_summary, payload, token,
               status, expires_at
           ) VALUES ($1, $2, $3, $4, $5, 'reviewer@example.com', $6, 'tool', 'Approve',
                     'Confirm', '{}', $7, 'pending', $8)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(channel_id)
    .bind(thread.id)
    .bind(format!("cross-tenant-{}", Uuid::new_v4()))
    .bind(foreign_principal.as_uuid())
    .bind(Uuid::new_v4())
    .bind(Utc::now() + chrono::Duration::hours(1))
    .execute(persistence.pool())
    .await;

    assert!(
        result.is_err(),
        "the composite foreign key must reject a foreign approver"
    );
}

/// A second person in `company_id`, a member rather than the owner.
async fn add_teammate(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    display_label: &str,
) -> PrincipalId {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(
            &format!("attention-teammate-{suffix}"),
            &format!("attention-teammate-{suffix}@example.com"),
            "hash",
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(user.id)
    .execute(persistence.pool())
    .await
    .unwrap();
    let principal = PrincipalId::random();
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
           VALUES ($1, $2, 'person', $3, $4)"#,
    )
    .bind(principal.as_uuid())
    .bind(company_id)
    .bind(user.id)
    .bind(display_label)
    .execute(persistence.pool())
    .await
    .unwrap();
    principal
}

async fn insert_approval_task(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    owner: PrincipalId,
) -> Uuid {
    let scope = FeedScope {
        company_id,
        channel_id,
        thread_id,
    };
    insert_task(persistence, scope, Some(owner), "pending_approval").await
}

/// Where a fixture item lives: its company, the channel it is visible through, and a thread to
/// hang messages, approvals and drafts on.
#[derive(Clone, Copy)]
struct FeedScope {
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
}

/// A task in `status`, owned by `owner` — or, given `None`, by whatever the ownership trigger
/// assigns, which on the fixture channel is its agent.
async fn insert_task(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    owner: Option<PrincipalId>,
    status: &str,
) -> Uuid {
    let task_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO background_tasks (
               id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
               owner_principal_id, owner_principal_kind
           ) VALUES ($1, $2, $3, $4, $5, 'attention-test', $6, '{}', $7, $8)"#,
    )
    .bind(task_id)
    .bind(scope.company_id)
    .bind(scope.channel_id)
    .bind(scope.thread_id)
    .bind(Uuid::new_v4())
    .bind(status)
    .bind(owner.map(PrincipalId::as_uuid))
    .bind(owner.map(|_| "person"))
    .execute(persistence.pool())
    .await
    .unwrap();
    task_id
}

async fn insert_pending_approval(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    task_id: Uuid,
    approver_email: &str,
    approver_principal_id: Option<PrincipalId>,
) -> Uuid {
    let approval_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO human_approvals (
               id, company_id, channel_id, thread_id, task_id, step_key, approver_email,
               approver_principal_id, action_type, action_title, action_summary, payload, token,
               status, expires_at
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'tool', 'Approve deployment',
                     'Confirm the deployment', '{}', $9, 'pending', $10)"#,
    )
    .bind(approval_id)
    .bind(company_id)
    .bind(channel_id)
    .bind(thread_id)
    .bind(task_id)
    .bind(format!("approval-{approval_id}"))
    .bind(approver_email)
    .bind(approver_principal_id.map(PrincipalId::as_uuid))
    .bind(Uuid::new_v4())
    .bind(Utc::now() + chrono::Duration::hours(1))
    .execute(persistence.pool())
    .await
    .unwrap();
    approval_id
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
        all_owned: false,
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

/// Each union branch filters on the responsibility its own columns imply, ahead of `ranked`.
/// That may change how much work a narrowed view does, never a row it returns.
///
/// Agreement alone would also be satisfied by both statements being wrong;
/// `every_spread_item_lands_with_the_holder_it_was_built_for` pins what they agree on.
#[tokio::test]
async fn branch_responsibility_filters_return_what_filtering_after_the_union_returned() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let spread = spread(&persistence).await;
    let visible = [spread.scope.channel_id];
    let request = |principal, view, all_owned| AttentionQuery {
        all_owned,
        ..query(spread.scope.company_id, &visible, principal, view)
    };
    for request in [
        request(spread.viewer, AttentionView::MyWork, false),
        request(spread.viewer, AttentionView::MyWork, true),
        request(spread.teammate, AttentionView::MyWork, false),
        request(PrincipalId::random(), AttentionView::MyWork, false),
        request(spread.viewer, AttentionView::Unassigned, false),
        request(spread.viewer, AttentionView::TeamWork, false),
    ] {
        assert_eq!(
            feed(&persistence, ATTENTION_SQL, request).await,
            feed(&persistence, ATTENTION_SQL_BEFORE_PUSHDOWN, request).await,
            "{:?} for {:?} (all_owned: {}) must return what it did before the push-down",
            request.view,
            request.principal_id,
            request.all_owned,
        );
    }
}

#[tokio::test]
async fn every_spread_item_lands_with_the_holder_it_was_built_for() {
    use AttentionView::{MyWork, TeamWork, Unassigned};

    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let spread = spread(&persistence).await;
    let (viewer, teammate) = (spread.viewer, spread.teammate);
    let visible = [spread.scope.channel_id];
    let request = |principal, view| query(spread.scope.company_id, &visible, principal, view);

    let viewer_work = listed(&persistence, request(viewer, MyWork)).await;
    assert_eq!(viewer_work, sorted(spread.viewer_items.clone()));
    assert_eq!(
        listed(&persistence, request(teammate, MyWork)).await,
        sorted(spread.teammate_items.clone())
    );
    assert!(
        listed(&persistence, request(PrincipalId::random(), MyWork))
            .await
            .is_empty()
    );
    assert_eq!(
        listed(&persistence, request(viewer, Unassigned)).await,
        sorted(spread.team_items.clone())
    );
    let mut everything = [
        spread.viewer_items.clone(),
        spread.teammate_items.clone(),
        spread.team_items.clone(),
    ]
    .concat();
    everything.push((AttentionSourceKind::Approval, spread.external_approval));
    assert_eq!(
        listed(&persistence, request(viewer, TeamWork)).await,
        sorted(everything)
    );

    // `all_owned` adds the viewer's finished tasks, and only the viewer's.
    let owned = AttentionQuery {
        all_owned: true,
        ..request(viewer, MyWork)
    };
    let owned = listed(&persistence, owned).await;
    assert!(owned.contains(&(AttentionSourceKind::Task, spread.viewer_completed_task)));
    assert!(!owned.contains(&(AttentionSourceKind::Task, spread.teammate_completed_task)));
    assert!(viewer_work.iter().all(|item| owned.contains(item)));
}

/// `ranked` cannot put back a row a branch never produced, so a branch filter narrower than
/// `ranked`'s would hide work without failing anything. Every narrowed view must therefore be
/// exactly the team view's rows with that responsibility — whatever the branches emit.
#[tokio::test]
async fn narrowed_views_are_the_team_view_split_by_responsibility() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let persistence = PostgresPersistence::new(pool);
    let spread = spread(&persistence).await;
    let visible = [spread.scope.channel_id];
    let request = |principal, view| query(spread.scope.company_id, &visible, principal, view);
    let team = persistence
        .list_attention(request(spread.viewer, AttentionView::TeamWork))
        .await
        .unwrap()
        .items;
    let held_by = |holder: AttentionResponsibility| {
        sorted(identities(
            team.iter().filter(|item| item.responsibility == holder),
        ))
    };

    assert_eq!(
        listed(&persistence, request(spread.viewer, AttentionView::MyWork)).await,
        held_by(AttentionResponsibility::Principal(spread.viewer))
    );
    assert_eq!(
        listed(
            &persistence,
            request(spread.teammate, AttentionView::MyWork)
        )
        .await,
        held_by(AttentionResponsibility::Principal(spread.teammate))
    );
    assert_eq!(
        listed(
            &persistence,
            request(spread.viewer, AttentionView::Unassigned)
        )
        .await,
        held_by(AttentionResponsibility::ChannelTeam)
    );
    assert_eq!(
        held_by(AttentionResponsibility::External),
        vec![(AttentionSourceKind::Approval, spread.external_approval)],
        "an email-only approver's item is nobody's work here and nobody's team work"
    );
}

type Identity = (AttentionSourceKind, Uuid);

/// The rows two feed statements must agree on: every column a page is built from except
/// `as_of`, which is each statement's own clock.
type FeedRow = (String, Uuid, String, Option<Uuid>, String, i64, i64);

async fn feed(
    persistence: &PostgresPersistence,
    sql: &str,
    request: AttentionQuery<'_>,
) -> Vec<FeedRow> {
    bind_attention(sql, request)
        .fetch_all(persistence.pool())
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            (
                row.source_kind,
                row.source_id,
                row.responsibility_kind,
                row.responsible_principal_id,
                row.state,
                row.version,
                row.bounded_count,
            )
        })
        .collect()
}

/// The set of items a view lists.
async fn listed(persistence: &PostgresPersistence, request: AttentionQuery<'_>) -> Vec<Identity> {
    sorted(identities(
        &persistence.list_attention(request).await.unwrap().items,
    ))
}

fn identities<'a>(items: impl IntoIterator<Item = &'a AttentionItem>) -> Vec<Identity> {
    items
        .into_iter()
        .map(|item| (item.source_kind, item.source_id))
        .collect()
}

fn sorted(mut items: Vec<Identity>) -> Vec<Identity> {
    items.sort();
    items
}

/// Every source kind the feed has, spread across two people and the channel team.
struct Spread {
    scope: FeedScope,
    viewer: PrincipalId,
    teammate: PrincipalId,
    viewer_items: Vec<Identity>,
    teammate_items: Vec<Identity>,
    team_items: Vec<Identity>,
    external_approval: Uuid,
    viewer_completed_task: Uuid,
    teammate_completed_task: Uuid,
}

async fn spread(persistence: &PostgresPersistence) -> Spread {
    let (company_id, channel_id, viewer) = fixture(persistence).await;
    let teammate = add_teammate(persistence, company_id, "Teammate").await;
    let thread = persistence
        .create_thread(channel_id, "Responsibility spread", &[])
        .await
        .unwrap();
    let scope = FeedScope {
        company_id,
        channel_id,
        thread_id: thread.id,
    };
    let externally_approved =
        insert_task(persistence, scope, Some(viewer), "pending_approval").await;
    let external_approval = insert_pending_approval(
        persistence,
        company_id,
        channel_id,
        thread.id,
        externally_approved,
        "external@example.net",
        None,
    )
    .await;
    // Agent work in progress, which no view shows.
    insert_task(persistence, scope, None, "pending").await;
    Spread {
        scope,
        viewer,
        teammate,
        viewer_items: person_items(persistence, scope, viewer, teammate).await,
        teammate_items: person_items(persistence, scope, teammate, viewer).await,
        team_items: team_items(persistence, scope).await,
        external_approval,
        viewer_completed_task: insert_task(persistence, scope, Some(viewer), "completed").await,
        teammate_completed_task: insert_task(persistence, scope, Some(teammate), "completed").await,
    }
}

/// One item of every kind a person can be responsible for, all of them `person`'s.
///
/// Approvals and reviews belong to whoever decides them, so each hangs on `colleague`'s task: the
/// task drops out of the feed and the decision still lands with `person`. Delegation decisions and
/// delivery failures inherit their task's owner, so those tasks are `person`'s own.
async fn person_items(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    person: PrincipalId,
    colleague: PrincipalId,
) -> Vec<Identity> {
    let approved = insert_task(persistence, scope, Some(colleague), "pending_approval").await;
    let reviewed = insert_task(persistence, scope, Some(colleague), "pending").await;
    let delegated = insert_task(
        persistence,
        scope,
        Some(person),
        "waiting_for_third_party_reply",
    )
    .await;
    let undelivered = insert_task(persistence, scope, Some(person), "failed").await;
    vec![
        (
            AttentionSourceKind::Task,
            insert_task(persistence, scope, Some(person), "pending").await,
        ),
        (
            AttentionSourceKind::Handoff,
            insert_handoff(persistence, scope, Some(person)).await,
        ),
        (
            AttentionSourceKind::Approval,
            insert_pending_approval(
                persistence,
                scope.company_id,
                scope.channel_id,
                scope.thread_id,
                approved,
                "approver@example.com",
                Some(person),
            )
            .await,
        ),
        (
            AttentionSourceKind::ResponseReview,
            insert_pending_review(persistence, scope, reviewed, person).await,
        ),
        (
            AttentionSourceKind::DelegationDecision,
            insert_delegation_decision(persistence, scope, delegated).await,
        ),
        (
            AttentionSourceKind::DeliveryFailure,
            insert_delivery_failure(persistence, scope, Some(undelivered), "dead_letter").await,
        ),
    ]
}

/// Channel-team work: every kind that can be unassigned, by each route there is to it.
async fn team_items(persistence: &PostgresPersistence, scope: FeedScope) -> Vec<Identity> {
    let ownerless = insert_task(persistence, scope, None, "pending").await;
    sqlx::query(
        r#"UPDATE background_tasks SET owner_principal_id = NULL, owner_principal_kind = NULL
           WHERE company_id = $1 AND id = $2"#,
    )
    .bind(scope.company_id)
    .bind(ownerless)
    .execute(persistence.pool())
    .await
    .unwrap();
    let delegated = insert_task(persistence, scope, None, "waiting_for_third_party_reply").await;
    let undelivered = insert_task(persistence, scope, None, "failed").await;
    vec![
        // An agent's task surfaces once a human has to decide; an ownerless one surfaces at once.
        (
            AttentionSourceKind::Task,
            insert_task(persistence, scope, None, "dead_letter").await,
        ),
        (AttentionSourceKind::Task, ownerless),
        (
            AttentionSourceKind::Handoff,
            insert_handoff(persistence, scope, None).await,
        ),
        (
            AttentionSourceKind::DelegationDecision,
            insert_delegation_decision(persistence, scope, delegated).await,
        ),
        (
            AttentionSourceKind::DeliveryFailure,
            insert_delivery_failure(persistence, scope, Some(undelivered), "outcome_unknown").await,
        ),
        // No task at all leaves the owner columns NULL, which is still channel-team work.
        (
            AttentionSourceKind::DeliveryFailure,
            insert_delivery_failure(persistence, scope, None, "dead_letter").await,
        ),
    ]
}

async fn insert_handoff(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    responsible: Option<PrincipalId>,
) -> Uuid {
    let handoff_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO manual_handoffs (
               id, company_id, channel_id, thread_id, title, next_action, responsible_principal_id
           ) VALUES ($1, $2, $3, $4, 'Call the customer', 'Confirm the address', $5)"#,
    )
    .bind(handoff_id)
    .bind(scope.company_id)
    .bind(scope.channel_id)
    .bind(scope.thread_id)
    .bind(responsible.map(PrincipalId::as_uuid))
    .execute(persistence.pool())
    .await
    .unwrap();
    handoff_id
}

/// A draft for `task_id` waiting on `reviewer`, which is what the feed lists by draft id.
async fn insert_pending_review(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    task_id: Uuid,
    reviewer: PrincipalId,
) -> Uuid {
    let draft_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO response_drafts (
               id, version, company_id, channel_id, thread_id, task_id, author_principal_id,
               reviewer_principal_id, proposed_message_id, subject, body, attachment_snapshot,
               recipient_snapshot, transport_snapshot, publication_snapshot,
               created_by_principal_id, updated_by_principal_id
           ) VALUES ($1, 1, $2, $3, $4, $5, $6, $6, $7, 'Proposed reply', 'Proposed body',
                     '{"version": "1", "items": []}', '{"version": "1", "to": [], "cc": []}',
                     '{"version": "1"}', '{"version": "1"}', $6, $6)"#,
    )
    .bind(draft_id)
    .bind(scope.company_id)
    .bind(scope.channel_id)
    .bind(scope.thread_id)
    .bind(task_id)
    .bind(reviewer.as_uuid())
    .bind(Uuid::new_v4())
    .execute(persistence.pool())
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO response_reviews (
               company_id, draft_id, draft_version, reviewer_principal_id, expires_at
           ) VALUES ($1, $2, 1, $3, CURRENT_TIMESTAMP + interval '1 hour')"#,
    )
    .bind(scope.company_id)
    .bind(draft_id)
    .bind(reviewer.as_uuid())
    .execute(persistence.pool())
    .await
    .unwrap();
    draft_id
}

/// A delegation on `task_id` that timed out and now waits for a human to decide.
async fn insert_delegation_decision(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    task_id: Uuid,
) -> Uuid {
    let outreach_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_outreaches (
               id, company_id, task_id, status, required_threshold_percent, expires_at,
               outreach_key, subject, body
           ) VALUES ($1, $2, $3, 'timeout_pending_approval', 100,
                     CURRENT_TIMESTAMP + interval '2 hours', $4, 'Need input', 'Body')"#,
    )
    .bind(outreach_id)
    .bind(scope.company_id)
    .bind(task_id)
    .bind(Uuid::new_v4().to_string())
    .execute(persistence.pool())
    .await
    .unwrap();
    outreach_id
}

/// A delivery for `task_id` that ended in `status`, one of the two the feed treats as failed.
async fn insert_delivery_failure(
    persistence: &PostgresPersistence,
    scope: FeedScope,
    task_id: Option<Uuid>,
    status: &str,
) -> Uuid {
    let queued = delivery_fixture(
        persistence,
        DeliveryFixtureRequest {
            task_id,
            ..DeliveryFixtureRequest::new(
                scope.company_id,
                scope.channel_id,
                scope.thread_id,
                &format!("attention-{}", Uuid::new_v4()),
            )
        },
    )
    .await;
    let delivery_id = queued.delivery.id.as_uuid();
    persistence.enqueue_delivery(queued.delivery).await.unwrap();
    sqlx::query("UPDATE message_deliveries SET status = $3 WHERE company_id = $1 AND id = $2")
        .bind(scope.company_id)
        .bind(delivery_id)
        .bind(status)
        .execute(persistence.pool())
        .await
        .unwrap();
    delivery_id
}

/// `ATTENTION_SQL` exactly as it stood before the responsibility filter was pushed into the union
/// branches: `raw` builds the rows of every responsibility and `ranked` alone discards them.
///
/// Frozen as the oracle for that push-down, which had to change how much work a narrowed view
/// does without changing a row it returns. It is not a second specification of the feed: when
/// the feed changes what it returns on purpose, retire the comparison that reads this rather
/// than editing it to match. `narrowed_views_are_the_team_view_split_by_responsibility` is the
/// guard that outlives it.
const ATTENTION_SQL_BEFORE_PUSHDOWN: &str = r#"
WITH params AS (
    SELECT COALESCE($5::timestamptz, CURRENT_TIMESTAMP) AS as_of
), raw AS (
    SELECT 'task'::text AS source_kind, task.id AS source_id, task.company_id,
           task.channel_id, task.thread_id, task.id AS task_id, task.correlation_id,
           task.status AS state,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END
               AS responsible_principal_id,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN 'principal' ELSE 'channel_team' END AS responsibility_kind,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END
               AS responsibility_label,
           task.task_type AS title,
           CASE task.status
             WHEN 'completed' THEN 'View the completed task'
             WHEN 'stopped' THEN 'View the stopped task'
             WHEN 'pending_approval' THEN 'Review the pending approval'
             WHEN 'dead_letter' THEN 'Decide how to recover the failed task'
             WHEN 'failed' THEN 'Decide how to recover the failed task'
             ELSE 'Complete or reassign the task'
           END AS next_action,
           task.business_priority, task.business_due_at AS due_at,
           task.wait_expires_at AS expires_at,
           task.attention_version AS version,
           task.created_at, task.updated_at
    FROM background_tasks AS task
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE task.company_id = $1 AND task.channel_id = ANY($2)
      AND (
        ($14 AND $4 = 'my_work' AND task.owner_principal_id = $3
         AND task.owner_principal_kind = 'person')
        OR (task.status IN ('pending', 'processing', 'pending_approval',
                          'waiting_for_third_party_reply', 'failed', 'dead_letter')
        AND (task.owner_principal_kind = 'person' OR task.owner_principal_id IS NULL
           OR task.status IN ('pending_approval', 'failed', 'dead_letter'))
        AND NOT EXISTS (
          SELECT 1 FROM human_approvals AS approval
          WHERE approval.company_id = task.company_id AND approval.task_id = task.id
            AND approval.status = 'pending'
        )
        AND NOT EXISTS (
          SELECT 1 FROM response_reviews AS review
          WHERE review.company_id = task.company_id AND review.status = 'pending'
            AND EXISTS (
                SELECT 1 FROM response_drafts AS draft
                WHERE draft.company_id = task.company_id AND draft.id = review.draft_id
                  AND draft.version = review.draft_version AND draft.task_id = task.id
                  AND draft.status = 'pending_review'
            )
        )
        AND NOT EXISTS (
          SELECT 1 FROM task_outreaches AS outreach
          WHERE outreach.company_id = task.company_id AND outreach.task_id = task.id
            AND outreach.status = 'timeout_pending_approval'
        )
        AND NOT EXISTS (
          SELECT 1 FROM message_deliveries AS delivery
          WHERE delivery.company_id = task.company_id AND delivery.task_id = task.id
            AND delivery.status IN ('outcome_unknown', 'dead_letter')
            AND delivery.last_error_class IS DISTINCT FROM 'superseded'
        )
        )
      )

    UNION ALL

    SELECT 'handoff', handoff.id, handoff.company_id, handoff.channel_id, handoff.thread_id,
           NULL::uuid, handoff.correlation_id, handoff.status,
           handoff.responsible_principal_id,
           CASE WHEN handoff.responsible_principal_id IS NULL
                THEN 'channel_team' ELSE 'principal' END,
           COALESCE(responsible.display_label, 'Channel team'), handoff.title,
           handoff.next_action, handoff.business_priority, handoff.business_due_at,
           NULL::timestamptz, handoff.version, handoff.created_at, handoff.updated_at
    FROM manual_handoffs AS handoff
    LEFT JOIN principals AS responsible
      ON responsible.company_id = handoff.company_id
     AND responsible.id = handoff.responsible_principal_id
    WHERE handoff.company_id = $1 AND handoff.channel_id = ANY($2)
      AND handoff.status = 'open'

    UNION ALL

    SELECT 'approval', approval.id, approval.company_id, approval.channel_id,
           approval.thread_id, approval.task_id, task.correlation_id, approval.status,
           approval.approver_principal_id,
           CASE WHEN approval.approver_principal_id IS NULL
                THEN 'external' ELSE 'principal' END,
           CASE WHEN approval.approver_principal_id IS NULL
                THEN 'External approver'
                ELSE COALESCE(approver.display_label, 'Assigned approver') END,
           approval.action_title, 'Approve or reject the requested action',
           COALESCE(task.business_priority, 'normal'),
           COALESCE(task.business_due_at, approval.expires_at), approval.expires_at,
           1::bigint, approval.created_at, approval.updated_at
    FROM human_approvals AS approval
    LEFT JOIN background_tasks AS task
      ON task.company_id = approval.company_id AND task.id = approval.task_id
    LEFT JOIN principals AS approver
      ON approver.company_id = approval.company_id
     AND approver.id = approval.approver_principal_id
    WHERE approval.company_id = $1 AND approval.channel_id = ANY($2)
      AND approval.status = 'pending'

    UNION ALL

    SELECT 'response_review', review.draft_id, review.company_id, draft.channel_id,
           draft.thread_id, draft.task_id, task.correlation_id, review.status,
           review.reviewer_principal_id, 'principal', reviewer.display_label,
           draft.subject, 'Review the proposed external response',
           COALESCE(task.business_priority, 'normal'), task.business_due_at,
           review.expires_at, draft.version::bigint, review.created_at, review.updated_at
    FROM response_reviews AS review
    JOIN response_drafts AS draft
      ON draft.company_id = review.company_id AND draft.id = review.draft_id
     AND draft.version = review.draft_version AND draft.status = 'pending_review'
    JOIN principals AS reviewer
      ON reviewer.company_id = review.company_id AND reviewer.id = review.reviewer_principal_id
    LEFT JOIN background_tasks AS task
      ON task.company_id = draft.company_id AND task.id = draft.task_id
    WHERE review.company_id = $1 AND draft.channel_id = ANY($2) AND review.status = 'pending'

    UNION ALL

    SELECT 'delegation_decision', outreach.id, task.company_id, task.channel_id,
           task.thread_id, task.id, task.correlation_id, outreach.status,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN 'principal' ELSE 'channel_team' END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END,
           outreach.subject, 'Review the delegation timeout', task.business_priority,
           CASE WHEN task.business_due_at IS NULL THEN outreach.expires_at
                ELSE LEAST(task.business_due_at, outreach.expires_at) END,
           outreach.expires_at, outreach.version, outreach.created_at, outreach.updated_at
    FROM task_outreaches AS outreach
    JOIN background_tasks AS task
      ON task.company_id = outreach.company_id AND task.id = outreach.task_id
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE task.company_id = $1 AND task.channel_id = ANY($2)
      AND outreach.status = 'timeout_pending_approval'

    UNION ALL

    SELECT 'delivery_failure', delivery.id, delivery.company_id, delivery.channel_id,
           task.thread_id, delivery.task_id, delivery.correlation_id, delivery.status,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN 'principal' ELSE 'channel_team' END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END,
           message.subject,
           CASE delivery.status WHEN 'outcome_unknown' THEN 'Resolve the unknown delivery outcome'
                ELSE 'Repair or dismiss the permanent delivery failure' END,
           COALESCE(task.business_priority, 'normal'), task.business_due_at,
           NULL::timestamptz, GREATEST(delivery.attempt_count, 1)::bigint,
           delivery.created_at, delivery.updated_at
    FROM message_deliveries AS delivery
    JOIN messages AS message
      ON message.company_id = delivery.company_id AND message.id = delivery.message_id
    LEFT JOIN background_tasks AS task
      ON task.company_id = delivery.company_id AND task.id = delivery.task_id
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE delivery.company_id = $1 AND delivery.channel_id = ANY($2)
      AND delivery.status IN ('outcome_unknown', 'dead_letter')
      AND delivery.last_error_class IS DISTINCT FROM 'superseded'
      AND NOT EXISTS (
          SELECT 1 FROM task_outreach_targets AS target
          JOIN task_outreaches AS outreach
            ON outreach.company_id = target.company_id AND outreach.id = target.outreach_id
          WHERE target.company_id = delivery.company_id AND target.delivery_id = delivery.id
            AND outreach.status = 'timeout_pending_approval'
      )
), ranked AS (
    SELECT raw.*, params.as_of,
           CASE WHEN raw.due_at < params.as_of THEN 0
                WHEN raw.due_at <= params.as_of + interval '24 hours' THEN 1 ELSE 2 END AS due_rank,
           CASE raw.business_priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END
               AS priority_rank
    FROM raw CROSS JOIN params
    WHERE ($4 = 'team_work'
           OR ($4 = 'my_work' AND raw.responsibility_kind = 'principal'
                                  AND raw.responsible_principal_id = $3)
           OR ($4 = 'unassigned' AND raw.responsibility_kind = 'channel_team'))
), after_cursor AS (
    SELECT * FROM ranked
    WHERE NOT $6 OR (due_rank, priority_rank, created_at, source_kind, source_id)
          > ($7, $8, $9, $10, $11)
    ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
    LIMIT $12
), counted AS (
    SELECT after_cursor.*, COUNT(*) OVER () AS bounded_count FROM after_cursor
)
SELECT * FROM counted
ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
LIMIT $13
"#;
