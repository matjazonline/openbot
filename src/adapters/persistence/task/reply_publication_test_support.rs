use super::*;

#[derive(Clone, Copy)]
pub(super) struct ChannelThread {
    pub channel_id: Uuid,
    pub thread_id: Uuid,
}

pub(super) struct Pipeline {
    pub task: BackgroundTask,
    pub lease: TaskLeaseRef,
    pub reply: AgentReply,
}

pub(super) struct PublicationFixture {
    pub persistence: PostgresPersistence,
    pub company_id: Uuid,
    pub owner: PrincipalId,
    pub source: CanonicalMessageId,
    pub support: Pipeline,
    pub billing: Pipeline,
    pub sales: ChannelThread,
    pub filed: ChannelThread,
    _database: OwnDatabase,
}

impl PublicationFixture {
    pub async fn new() -> Option<Self> {
        let database = own_database().await?;
        let persistence = PostgresPersistence::new(database.pool.clone());
        let (company, channel) = seed_company_and_channel(&persistence).await;
        let agent = channel.agent_ids.as_ref().unwrap()[0];
        let support_thread = ChannelThread {
            channel_id: channel.id,
            thread_id: persistence
                .create_thread(channel.id, "Support", &[])
                .await
                .unwrap()
                .id,
        };
        let sales = channel_thread(&persistence, company.id, agent, "sales").await;
        let billing_agent =
            seed_channel_agent(&persistence, company.id, "publication-billing").await;
        let billing = channel_thread(&persistence, company.id, billing_agent, "billing").await;
        let filed = channel_thread(&persistence, company.id, agent, "filed").await;
        let source = persistence
            .create_message(&MessageWrite::internal(
                support_thread.thread_id,
                MessageAuthorWrite::Platform,
                "Question",
                "Please answer",
                MessageDirection::Inbound,
                MessageRole::Human,
                CorrelationId::new(),
            ))
            .await
            .unwrap()
            .canonical_id;
        for target in [sales, billing, filed] {
            persistence
                .associate_message(target.thread_id, source, ThreadEntryKind::Conversation)
                .await
                .unwrap();
        }
        let support = pipeline(
            &persistence,
            company.id,
            agent,
            source,
            &[support_thread, sales],
        )
        .await;
        let billing = pipeline(&persistence, company.id, billing_agent, source, &[billing]).await;
        let owner = PrincipalId::new(
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2",
            )
            .bind(company.id)
            .bind(company.user_id)
            .fetch_one(&database.pool)
            .await
            .unwrap(),
        );
        Some(Self {
            persistence,
            company_id: company.id,
            owner,
            source,
            support,
            billing,
            sales,
            filed,
            _database: database,
        })
    }

    pub async fn delegation_count(&self, message_id: CanonicalMessageId) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM thread_messages WHERE company_id = $1 \
            AND message_id = $2 AND entry_kind = 'delegation'",
        )
        .bind(self.company_id)
        .bind(message_id.as_uuid())
        .fetch_one(self.persistence.pool())
        .await
        .unwrap()
    }
}

async fn channel_thread(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    agent_id: Uuid,
    slug: &str,
) -> ChannelThread {
    let channel = ChannelPersistence::create(
        persistence,
        company_id,
        ChannelWrite {
            name: slug.into(),
            slug: slug.into(),
            agent_ids: Some(vec![agent_id]),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let thread = persistence
        .create_thread(channel.id, slug, &[])
        .await
        .unwrap();
    ChannelThread {
        channel_id: channel.id,
        thread_id: thread.id,
    }
}

async fn pipeline(
    persistence: &PostgresPersistence,
    company_id: Uuid,
    agent_id: Uuid,
    source: CanonicalMessageId,
    threads: &[ChannelThread],
) -> Pipeline {
    let task = persistence
        .enqueue_task(NewTask {
            company_id,
            channel_id: threads[0].channel_id,
            thread_id: Some(threads[0].thread_id),
            task_type: "email_agent_dispatch".into(),
            payload: serde_json::json!({}),
            source: TaskSource::Message(source),
            correlation_id: CorrelationId::new(),
            targets: threads
                .iter()
                .map(|target| TaskTarget {
                    channel_id: target.channel_id,
                    thread_id: target.thread_id,
                    recipient_role: RecipientRole::To,
                })
                .collect(),
        })
        .await
        .unwrap();
    let lease = claim(persistence, task.id).await;
    let reply = AgentReply {
        message: MessageWrite::internal(
            threads[0].thread_id,
            MessageAuthorWrite::Agent(crate::use_cases::thread::AgentAuthor {
                agent_id,
                display_label: "Reply Agent".into(),
            }),
            "Answer",
            "Here is the answer",
            MessageDirection::Outbound,
            MessageRole::Agent,
            task.correlation_id,
        )
        .external_conversation(),
        also_in_threads: threads
            .iter()
            .skip(1)
            .map(|target| target.thread_id)
            .collect(),
    };
    Pipeline { task, lease, reply }
}

pub(super) async fn commit(fixture: &PublicationFixture, pipeline: &Pipeline) {
    commit_with(&fixture.persistence, pipeline).await;
}

pub(super) async fn commit_with(persistence: &PostgresPersistence, pipeline: &Pipeline) {
    let outcome = persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease: pipeline.lease,
            reply: &pipeline.reply,
            deliveries: Vec::new(),
            review_candidate: None,
            payload: serde_json::json!({}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, DispatchCommit::Committed { .. }));
}

pub(super) async fn assert_entry(
    fixture: &PublicationFixture,
    thread_id: Uuid,
    message_id: CanonicalMessageId,
    kind: ThreadEntryKind,
) {
    let message = fixture
        .persistence
        .get_thread_message(thread_id, message_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(message.entry_kind, kind);
}

pub(super) async fn review_delivery(fixture: &PublicationFixture) -> NewDelivery {
    let side = fixture
        .persistence
        .create_thread(fixture.support.task.channel_id, "Fixture", &[])
        .await
        .unwrap();
    let mut delivery = delivery_fixture(
        &fixture.persistence,
        DeliveryFixtureRequest {
            task_id: Some(fixture.support.task.id),
            body: "Here is the answer",
            subject: "Answer",
            ..DeliveryFixtureRequest::new(
                fixture.company_id,
                fixture.support.task.channel_id,
                side.id,
                "cross-file-review",
            )
        },
    )
    .await
    .delivery;
    delivery.message_id = fixture.support.reply.message.id;
    delivery.correlation_id = fixture.support.task.correlation_id;
    delivery
}

pub(super) async fn publish_support_and_check_sibling(fixture: &PublicationFixture) {
    let before = fixture
        .persistence
        .get_thread_by_id(fixture.billing.reply.message.thread_id)
        .await
        .unwrap()
        .unwrap();
    commit(fixture, &fixture.support).await;
    assert_entry(
        fixture,
        fixture.billing.reply.message.thread_id,
        fixture.support.reply.message.id,
        ThreadEntryKind::Delegation,
    )
    .await;
    assert_entry(
        fixture,
        fixture.filed.thread_id,
        fixture.support.reply.message.id,
        ThreadEntryKind::Delegation,
    )
    .await;
    assert!(
        fixture
            .persistence
            .find_outbound_reply_after(fixture.billing.reply.message.thread_id, fixture.source,)
            .await
            .unwrap()
            .is_none()
    );
    let refreshed = fixture
        .persistence
        .list_threads_updated_after(
            fixture.billing.task.channel_id,
            Some(ThreadCursor::new(before.updated_at, before.id)),
            20,
        )
        .await
        .unwrap();
    assert!(
        refreshed.iter().any(|thread| thread.id == before.id),
        "cross-filing advances the sidebar's update cursor"
    );
}

pub(super) async fn submit_support_for_review(fixture: &PublicationFixture) -> ReviewCommand {
    fixture
        .persistence
        .set_company_review_policy(
            fixture.company_id,
            ExternalResponseReview::ReviewAllExternal,
        )
        .await
        .unwrap();
    let delivery = review_delivery(fixture).await;
    let outcome = fixture
        .persistence
        .commit_agent_dispatch(AgentDispatchCommit {
            lease: fixture.support.lease,
            reply: &fixture.support.reply,
            deliveries: vec![delivery],
            review_candidate: Some(AgentReviewCandidate {
                recipients: crate::entities::response_draft::DraftRecipientSnapshot::email(
                    "customer@example.com".into(),
                    Vec::new(),
                ),
                evidence: Vec::new(),
            }),
            payload: serde_json::json!({}),
            complete_outreach: false,
        })
        .await
        .unwrap();
    let DispatchCommit::PendingReview {
        draft_id,
        draft_version,
    } = outcome
    else {
        panic!("reply must remain a draft until approval")
    };
    ReviewCommand {
        company_id: fixture.company_id,
        draft_id,
        expected_draft_version: draft_version,
        command_id: Uuid::new_v4(),
        actor_principal_id: fixture.owner,
        action: ReviewAction::Approve { rationale: None },
    }
}
