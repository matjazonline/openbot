use super::*;
use crate::entities::transport::{ChannelBindingId, ExternalMessageKey, ExternalThreadKey};
use crate::transport::ExternalCorrelationStore;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture(trigger: ChannelResponseTrigger) -> ChannelFixture {
    channel_fixture(TestChannel {
        response_trigger: trigger,
        ..TestChannel::default()
    })
}

fn mail(body: &str, headers: &str) -> RawInboundPayload {
    RawInboundPayload {
        from: "team@acme.com".into(),
        to: "support@acme.mailagents.com".into(),
        subject: Some("Response setting".into()),
        text: Some(body.into()),
        headers: Some(format!(
            "Message-ID: <{}@acme.com>\n{headers}",
            Uuid::new_v4()
        )),
        ..Default::default()
    }
}

fn assert_runs(result: &InboundIngestResult, expected: bool) {
    assert!(result.accepted, "{:?}", result.reason());
    assert_eq!(result.answers(), expected);
    assert_eq!(!result.task_ids.is_empty(), expected);
}

async fn seed(fx: &ChannelFixture) -> (Uuid, Message) {
    let first = fx
        .use_cases
        .ingest_test_email(mail("Please @support", ""))
        .await
        .unwrap();
    assert_runs(&first, true);
    let thread_id = first.thread.unwrap().id;
    let agent = save_turn(&fx.use_cases, thread_id, MessageRole::Agent).await;
    (thread_id, agent)
}

async fn save_turn(uc: &ThreadUseCases, thread_id: Uuid, role: MessageRole) -> Message {
    uc.save_message(&email_write(EmailMessageDraft {
        thread_id,
        message_id: format!("<{}@acme.com>", Uuid::new_v4()).into(),
        sender: "team@acme.com".into(),
        recipients_to: vec!["support@acme.mailagents.com".into()],
        subject: "Response setting".into(),
        clean_text_body: "A previous turn".into(),
        direction: if role == MessageRole::Agent {
            MessageDirection::Outbound
        } else {
            MessageDirection::Inbound
        },
        role,
        ..Default::default()
    }))
    .await
    .unwrap()
}

async fn rfc(uc: &ThreadUseCases, company_id: Uuid, message: &Message) -> MessageId {
    uc.thread_persistence
        .get_message_protocol_extension(company_id, message.canonical_id)
        .await
        .unwrap()
        .email_metadata()
        .unwrap()
        .rfc_message_id
        .clone()
}

#[tokio::test]
async fn mentioned_requires_a_current_body_mention_for_to_and_cc() {
    for role in [RecipientRole::To, RecipientRole::Cc] {
        let fx = fixture(ChannelResponseTrigger::Mentioned);
        for (body, expected) in [("Please help", false), ("Please @support", true)] {
            let mut message = mail(body, "");
            if role == RecipientRole::Cc {
                message.to = "someone@example.com".into();
                message.cc = Some("support@acme.mailagents.com".into());
            }
            let result = fx.use_cases.ingest_test_email(message).await.unwrap();
            assert_runs(&result, expected);
        }
        let (thread, agent) = seed(&fx).await;
        let parent = rfc(&fx.use_cases, fx.company_id, &agent).await;
        let mut message = mail(
            "Context only.\n\nOn Fri, someone wrote:\n> Please @support",
            &format!("In-Reply-To: {parent}\n"),
        );
        if role == RecipientRole::Cc {
            message.to = "someone@example.com".into();
            message.cc = Some("support@acme.mailagents.com".into());
        }
        let result = fx.use_cases.ingest_test_email(message).await.unwrap();
        assert_runs(&result, false);
        assert_eq!(result.thread.unwrap().id, thread);
    }
}

#[tokio::test]
async fn reply_trigger_uses_only_the_direct_parent_including_references_only_and_cc() {
    let fx = fixture(ChannelResponseTrigger::MentionedOrReplyToAgent);
    let (thread, agent) = seed(&fx).await;
    let agent_rfc = rfc(&fx.use_cases, fx.company_id, &agent).await;
    let human = save_turn(&fx.use_cases, thread, MessageRole::Human).await;
    let human_rfc = rfc(&fx.use_cases, fx.company_id, &human).await;
    for (headers, expected) in [
        (format!("In-Reply-To: {agent_rfc}\n"), true),
        (format!("References: {human_rfc} {agent_rfc}\n"), true),
        (
            format!("In-Reply-To: {human_rfc}\nReferences: {agent_rfc}\n"),
            false,
        ),
        (format!("References: {agent_rfc} {human_rfc}\n"), false),
        (
            format!("In-Reply-To: <unknown@acme.com>\nReferences: {agent_rfc}\n"),
            false,
        ),
    ] {
        for role in [RecipientRole::To, RecipientRole::Cc] {
            let mut message = mail("Follow up", &headers);
            if role == RecipientRole::Cc {
                message.to = "someone@example.com".into();
                message.cc = Some("support@acme.mailagents.com".into());
            }
            let result = fx.use_cases.ingest_test_email(message).await.unwrap();
            assert_runs(&result, expected);
            assert_eq!(result.thread.unwrap().id, thread);
        }
    }
    assert_runs(
        &fx.use_cases
            .ingest_test_email(mail("New conversation", ""))
            .await
            .unwrap(),
        false,
    );
}

#[tokio::test]
async fn quiet_wins_over_a_qualifying_agent_reply() {
    let fx = fixture(ChannelResponseTrigger::MentionedOrReplyToAgent);
    let (_, agent) = seed(&fx).await;
    let parent = rfc(&fx.use_cases, fx.company_id, &agent).await;
    for (body, address) in [
        ("[quiet] Follow up", "support@acme.mailagents.com"),
        ("Follow up", "support+quiet@acme.mailagents.com"),
    ] {
        let mut message = mail(body, &format!("In-Reply-To: {parent}\n"));
        message.to = address.into();
        assert_runs(
            &fx.use_cases.ingest_test_email(message).await.unwrap(),
            false,
        );
    }
}

#[tokio::test]
async fn canonical_composer_honors_explicit_and_implicit_agent_and_human_parents() {
    let fx = fixture(ChannelResponseTrigger::MentionedOrReplyToAgent);
    let (thread, agent) = seed(&fx).await;
    let author = qualified_email_identity("team@acme.com").unwrap();
    let compose = || {
        CanonicalMessageIngress::new(
            fx.company_id,
            fx.channel_id,
            author.clone(),
            "Follow up",
            IngressOrigin::TrustedApplication,
        )
        .with_target_thread(thread)
    };
    // Immediately after the agent, implicit threading qualifies.
    assert_runs(
        &fx.use_cases.ingest_canonical(compose()).await.unwrap(),
        true,
    );
    // The preceding composer message was human.
    assert_runs(
        &fx.use_cases.ingest_canonical(compose()).await.unwrap(),
        false,
    );
    assert_runs(
        &fx.use_cases
            .ingest_canonical(compose().with_reply_to_message(agent.canonical_id))
            .await
            .unwrap(),
        true,
    );
    let human = save_turn(&fx.use_cases, thread, MessageRole::Human).await;
    assert_runs(
        &fx.use_cases
            .ingest_canonical(compose().with_reply_to_message(human.canonical_id))
            .await
            .unwrap(),
        false,
    );
}

// Intercept only parent resolution; correlation still exercises the real in-memory implementation.
struct ParentLookup {
    inner: Arc<dyn ExternalCorrelationStore>,
    calls: AtomicUsize,
    result: AppResult<Option<CanonicalMessageId>>,
}

#[async_trait::async_trait]
impl ExternalCorrelationStore for ParentLookup {
    async fn thread_for_thread_keys(
        &self,
        binding: ChannelBindingId,
        keys: &[ExternalThreadKey],
    ) -> AppResult<Option<Uuid>> {
        self.inner.thread_for_thread_keys(binding, keys).await
    }
    async fn thread_for_message_keys(
        &self,
        binding: ChannelBindingId,
        keys: &[ExternalMessageKey],
    ) -> AppResult<Option<Uuid>> {
        self.inner.thread_for_message_keys(binding, keys).await
    }
    async fn message_for_external_key(
        &self,
        _: ChannelBindingId,
        _: &ExternalMessageKey,
    ) -> AppResult<Option<CanonicalMessageId>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match &self.result {
            Ok(id) => Ok(*id),
            Err(_) => Err(AppError::Internal("parent lookup unavailable".into())),
        }
    }
}

#[tokio::test]
async fn a_parent_outside_the_target_thread_does_not_qualify_and_lookup_errors_propagate() {
    let mut fx = fixture(ChannelResponseTrigger::MentionedOrReplyToAgent);
    let (thread, agent) = seed(&fx).await;
    let parent = rfc(&fx.use_cases, fx.company_id, &agent).await;
    let (_, other_agent) = seed(&fx).await;
    let lookup = Arc::new(ParentLookup {
        inner: fx.use_cases.correlation_store.clone(),
        calls: AtomicUsize::new(0),
        result: Ok(Some(other_agent.canonical_id)),
    });
    fx.use_cases.correlation_store = lookup.clone();
    let result = fx
        .use_cases
        .ingest_test_email(mail("Follow up", &format!("In-Reply-To: {parent}\n")))
        .await
        .unwrap();
    assert_runs(&result, false);
    assert_eq!(result.thread.unwrap().id, thread);
    assert_eq!(lookup.calls.load(Ordering::Relaxed), 1);
    fx.use_cases.correlation_store = Arc::new(ParentLookup {
        inner: lookup,
        calls: AtomicUsize::new(0),
        result: Err(AppError::Internal("offline".into())),
    });
    assert!(
        fx.use_cases
            .ingest_test_email(mail("Follow up", &format!("In-Reply-To: {parent}\n")))
            .await
            .is_err()
    );
    // A mention short-circuits the failing parent store.
    assert_runs(
        &fx.use_cases
            .ingest_test_email(mail(
                "Now @support investigate this new detail",
                &format!("In-Reply-To: {parent}\n"),
            ))
            .await
            .unwrap(),
        true,
    );
}

#[tokio::test]
async fn always_to_does_not_resolve_parent_or_require_an_agent_directory() {
    let mut fx = fixture(ChannelResponseTrigger::Always);
    let (_, agent) = seed(&fx).await;
    let parent = rfc(&fx.use_cases, fx.company_id, &agent).await;
    let lookup = Arc::new(ParentLookup {
        inner: fx.use_cases.correlation_store.clone(),
        calls: AtomicUsize::new(0),
        result: Err(AppError::Internal("must not query".into())),
    });
    fx.use_cases.correlation_store = lookup.clone();
    fx.channels.channels.lock().unwrap()[0].agent_ids = Some(vec![Uuid::new_v4()]);
    fx.use_cases.agent_persistence = Some(Arc::new(RefuseAgentLookup));
    assert_runs(
        &fx.use_cases
            .ingest_test_email(mail("Follow up", &format!("In-Reply-To: {parent}\n")))
            .await
            .unwrap(),
        true,
    );
    assert_eq!(lookup.calls.load(Ordering::Relaxed), 0);
}

fn add_sibling(fx: &ChannelFixture, slug: &str, trigger: ChannelResponseTrigger) -> Channel {
    let mut channels = fx.channels.channels.lock().unwrap();
    let mut sibling = channels[0].clone();
    sibling.id = Uuid::new_v4();
    sibling.slug = slug.into();
    sibling.name = slug.into();
    sibling.response_trigger = trigger;
    sibling.owner_agent_id = None;
    channels.push(sibling.clone());
    sibling
}

#[tokio::test]
async fn each_receiving_channel_applies_its_own_trigger_and_parent_binding() {
    let fx = fixture(ChannelResponseTrigger::MentionedOrReplyToAgent);
    let sibling = add_sibling(&fx, "sales", ChannelResponseTrigger::Mentioned);
    let (thread, agent) = seed(&fx).await;
    let parent = rfc(&fx.use_cases, fx.company_id, &agent).await;
    let mut message = mail("Please @sales", &format!("In-Reply-To: {parent}\n"));
    message.to = "support@acme.mailagents.com, sales@acme.mailagents.com".into();
    let result = fx.use_cases.ingest_test_email(message).await.unwrap();
    assert_eq!(result.task_ids.len(), 2);
    // The agent id maps only in support. Sales is configured for replies now and has an existing
    // thread, but cannot borrow support's mapping even though the same header names it.
    fx.channels
        .channels
        .lock()
        .unwrap()
        .iter_mut()
        .find(|c| c.id == sibling.id)
        .unwrap()
        .response_trigger = ChannelResponseTrigger::MentionedOrReplyToAgent;
    let sales_thread = result
        .channel_matches
        .iter()
        .find(|m| m.channel.id == sibling.id)
        .unwrap()
        .thread
        .id;
    let ingress = CanonicalMessageIngress::new(
        fx.company_id,
        sibling.id,
        qualified_email_identity("team@acme.com").unwrap(),
        "No mention",
        IngressOrigin::TrustedApplication,
    )
    .with_target_thread(sales_thread);
    assert_runs(
        &fx.use_cases.ingest_canonical(ingress).await.unwrap(),
        false,
    );
    // Use normal email routing to sales while preserving the support-only external parent.
    let mut message = mail("Follow up", &format!("In-Reply-To: {parent}\n"));
    message.to = "sales@acme.mailagents.com".into();
    let result = fx.use_cases.ingest_test_email(message).await.unwrap();
    assert_runs(&result, false);
    assert_ne!(result.thread.unwrap().id, thread);
}

#[tokio::test]
async fn internal_relays_obey_triggers_on_standalone_and_owned_channels() {
    for owner in [None, Some(Uuid::new_v4())] {
        for trigger in [
            ChannelResponseTrigger::Always,
            ChannelResponseTrigger::Mentioned,
            ChannelResponseTrigger::MentionedOrReplyToAgent,
        ] {
            let fx = fixture(trigger);
            {
                let mut channels = fx.channels.channels.lock().unwrap();
                channels[0].owner_agent_id = owner;
                channels[0].agent_ids = Some(vec![owner.unwrap_or_else(Uuid::new_v4)]);
            }
            let source = add_sibling(&fx, "source", ChannelResponseTrigger::Always);
            let initial = InternalHop::new(
                source.id,
                "source@acme.mailagents.com",
                "support@acme.mailagents.com",
                "Response setting",
                "Start this request @support",
            );
            assert!(matches!(
                relay_hop(&fx.use_cases, &initial).await,
                crate::transport::RelayDisposition::Relayed
            ));
            let thread = fx
                .threads
                .threads()
                .into_iter()
                .find(|thread| thread.channel_id == fx.channel_id)
                .unwrap()
                .id;
            let agent = save_turn(&fx.use_cases, thread, MessageRole::Agent).await;
            let agent_rfc = rfc(&fx.use_cases, fx.company_id, &agent).await;
            let human = save_turn(&fx.use_cases, thread, MessageRole::Human).await;
            let human_rfc = rfc(&fx.use_cases, fx.company_id, &human).await;
            for (body, parent, expected) in [
                (
                    "Please help",
                    None,
                    trigger == ChannelResponseTrigger::Always,
                ),
                ("Please @support", None, true),
                (
                    "Follow up",
                    Some(agent_rfc.clone()),
                    trigger != ChannelResponseTrigger::Mentioned,
                ),
                (
                    "Follow up",
                    Some(human_rfc.clone()),
                    trigger == ChannelResponseTrigger::Always,
                ),
                ("[quiet] Please @support", None, false),
            ] {
                let mut hop = InternalHop::new(
                    source.id,
                    "source@acme.mailagents.com",
                    "support@acme.mailagents.com",
                    "Response setting",
                    body,
                );
                hop.in_reply_to = parent;
                let before = fx.tasks.tasks.lock().unwrap().len();
                let result = relay_hop(&fx.use_cases, &hop).await;
                assert!(
                    matches!(result, crate::transport::RelayDisposition::Relayed),
                    "{result:?}"
                );
                assert_eq!(
                    fx.tasks.tasks.lock().unwrap().len() - before,
                    usize::from(expected),
                    "{trigger:?}, owner={owner:?}, body={body}"
                );
            }
        }
    }
}

struct RefuseAgentLookup;

#[async_trait::async_trait]
impl AgentPersistence for RefuseAgentLookup {
    async fn create(&self, _: Uuid, _: AgentWrite) -> AppResult<Agent> {
        unreachable!()
    }
    async fn create_library(&self, _: AgentWrite) -> AppResult<Agent> {
        unreachable!()
    }
    async fn get_by_id(&self, _: Uuid) -> AppResult<Option<Agent>> {
        Err(AppError::Internal(
            "agent mention lookup must not run".into(),
        ))
    }
    async fn get_by_company_slug_and_agent_slug(
        &self,
        _: &str,
        _: &str,
    ) -> AppResult<Option<Agent>> {
        unreachable!()
    }
    async fn list_by_company_id(&self, _: Uuid) -> AppResult<Vec<Agent>> {
        unreachable!()
    }
    async fn list_library(&self) -> AppResult<Vec<Agent>> {
        unreachable!()
    }
    async fn update(&self, _: Uuid, _: AgentWrite) -> AppResult<Agent> {
        unreachable!()
    }
    async fn delete(&self, _: Uuid) -> AppResult<()> {
        unreachable!()
    }
}
