//! Route tests for the responsibility commands and the JSON read, on a real database.
//!
//! The handlers are called directly with the extractors axum would build: this repo has no
//! HTTP-level harness, and the sibling route tests ([`super::super::ui_thread_handoffs`]'s among
//! them) all take the same shape. Every assertion is scoped to the company the test created,
//! because the test database is shared and these run in parallel.

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        test_support::{ThreadHandoffFixtureRequest, test_pool, thread_handoff_fixture},
    },
    entities::{thread_handoff::ThreadHandoffState, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::{InboundIngestPorts, ThreadPersistence, ThreadStores},
        user::UserPersistence,
    },
};

/// A deployment with no memory providers and no spam scanning, which is all these routes need.
fn test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        default_agent_harness: crate::entities::harness::HarnessKind::AiAgents,
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: crate::infra::config::ResendApiConfig::default(),
        hydradb: None,
        hindsight: None,
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: String::new(),
        smtp_password: String::new(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: false,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: false,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        enable_llm_spam_guardrail: false,
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    })
}

/// One held reply and everything a request has to name to reach it.
#[derive(Clone, Copy)]
struct Hold {
    channel_id: Uuid,
    handoff_id: Uuid,
    generation: Uuid,
}

/// One company, its owner, a teammate, and the use cases the two routes are called with.
struct Harness {
    persistence: Arc<PostgresPersistence>,
    companies: Arc<CompanyUseCases>,
    channels: Arc<ChannelUseCases>,
    threads: Arc<ThreadUseCases>,
    handoffs: Arc<ThreadHandoffUseCases>,
    reviews: Arc<crate::use_cases::response_review::ResponseReviewUseCases>,
    company_id: Uuid,
    channel_id: Uuid,
    owner: Viewer,
    /// A plain team member: no company authority, and no sight of a restricted channel.
    member: Viewer,
    member_principal_id: Uuid,
}

impl Harness {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let (owner, company_id, channel_id) = company_with_channel(&persistence, None).await;
        let member = add_member(&persistence, company_id).await;
        let member_principal_id = principal_of(&persistence, company_id, member.user_id).await;
        let config = test_config();
        let renderers = Arc::new(
            crate::transport::ports::TransportRenderers::new()
                .register(Arc::new(
                    crate::adapters::protocols::email::EmailRenderer::new(&config.app_domain_name),
                ))
                .expect("one renderer registers"),
        );
        Some(Self {
            companies: Arc::new(CompanyUseCases::new(persistence.clone())),
            channels: Arc::new(ChannelUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                config.clone(),
            )),
            threads: Arc::new(ThreadUseCases::new(
                ThreadStores {
                    threads: persistence.clone(),
                    channels: persistence.clone(),
                    companies: persistence.clone(),
                    participants: persistence.clone(),
                    tasks: persistence.clone(),
                    handoff_policy: persistence.clone(),
                },
                InboundIngestPorts {
                    committer: persistence.clone(),
                    correlation: persistence.clone(),
                    bindings: persistence.clone(),
                    standalone_deliveries: persistence.clone(),
                },
                renderers,
                config,
            )),
            handoffs: Arc::new(ThreadHandoffUseCases::new(persistence.clone())),
            reviews: Arc::new(
                crate::use_cases::response_review::ResponseReviewUseCases::new(persistence.clone()),
            ),
            persistence,
            company_id,
            channel_id,
            owner,
            member,
            member_principal_id,
        })
    }

    /// A freshly held reply on a thread of its own, nobody's yet.
    async fn hold(&self, channel_id: Uuid) -> Hold {
        let thread_id = ThreadPersistence::create_thread(
            self.persistence.as_ref(),
            channel_id,
            "Invoice 4471",
            &[],
        )
        .await
        .unwrap()
        .id;
        let held = thread_handoff_fixture(
            self.persistence.as_ref(),
            ThreadHandoffFixtureRequest::new(self.company_id, channel_id, thread_id),
        )
        .await;
        Hold {
            channel_id,
            handoff_id: held.handoff_id,
            generation: held.generation,
        }
    }

    /// A channel only its named participants and the company owner may see.
    async fn restricted_channel(&self) -> Uuid {
        ChannelPersistence::create(
            self.persistence.as_ref(),
            self.company_id,
            ChannelWrite {
                name: "Restricted".into(),
                slug: format!("route-restricted-{}", Uuid::new_v4().simple()),
                participant_emails: Some(vec!["outsider@example.com".to_string()]),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap()
        .id
    }

    async fn post(
        &self,
        viewer: &Viewer,
        handoff_id: Uuid,
        body: serde_json::Value,
    ) -> AppResult<serde_json::Value> {
        let body: CommandBody =
            serde_json::from_value(body).expect("this test's body is well formed");
        change_thread_handoff(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            State(self.channels.clone()),
            State(self.threads.clone()),
            viewer.clone(),
            Path((self.company_id, handoff_id)),
            Json(body),
        )
        .await
        .map(|Json(value)| value)
    }

    async fn get(&self, viewer: &Viewer, handoff_id: Uuid) -> AppResult<serde_json::Value> {
        read_thread_handoff(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            State(self.channels.clone()),
            State(self.threads.clone()),
            viewer.clone(),
            Path((self.company_id, handoff_id)),
        )
        .await
        .map(|Json(value)| value)
    }

    /// The handoff as the database holds it, read past every route's authorization.
    async fn stored(&self, hold: &Hold) -> ThreadHandoff {
        self.handoffs
            .get_thread_handoff(self.company_id, hold.handoff_id, &[hold.channel_id])
            .await
            .unwrap()
            .expect("the fixture handoff exists")
    }
}

/// A claim command body, exactly as a client that read the queue item would send it.
fn claim_body(hold: &Hold, expected_version: u64) -> serde_json::Value {
    serde_json::json!({
        "command_id": Uuid::new_v4(),
        "expected_version": expected_version,
        "expected_generation": hold.generation,
        "operation": { "kind": "claim" },
        "priority": "normal",
        "due_at": null,
    })
}

/// A company with one channel, and the viewer who owns it.
async fn company_with_channel(
    persistence: &PostgresPersistence,
    participant_emails: Option<Vec<String>>,
) -> (Viewer, Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-cmd-{suffix}@example.com");
    persistence
        .create_user(&format!("handoff-cmd-{suffix}"), &email, "hash")
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
            name: "Thread Handoff Routes".into(),
            slug: format!("handoff-cmd-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        persistence,
        company.id,
        ChannelWrite {
            name: "Support".into(),
            slug: format!("handoff-cmd-support-{suffix}"),
            participant_emails,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    (
        Viewer {
            user_id: owner.id,
            email: EmailAddress::from(email.as_str()),
        },
        company.id,
        channel.id,
    )
}

/// A second person on the team, with a principal so `read_context` can place them.
async fn add_member(persistence: &PostgresPersistence, company_id: Uuid) -> Viewer {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-cmd-member-{suffix}@example.com");
    let user = persistence
        .create_user(&format!("handoff-cmd-member-{suffix}"), &email, "hash")
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
    sqlx::query(
        r#"INSERT INTO principals (id, company_id, kind, user_id, display_label)
           VALUES ($1, $2, 'person', $3, $4)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(user.id)
    .bind(format!("Member {suffix}"))
    .execute(persistence.pool())
    .await
    .unwrap();
    Viewer {
        user_id: user.id,
        email: EmailAddress::from(email.as_str()),
    }
}

/// Somebody with an account and no membership anywhere.
async fn outsider(persistence: &PostgresPersistence) -> Viewer {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-cmd-outsider-{suffix}@example.com");
    let user = persistence
        .create_user(&format!("handoff-cmd-outsider-{suffix}"), &email, "hash")
        .await
        .unwrap();
    Viewer {
        user_id: user.id,
        email: EmailAddress::from(email.as_str()),
    }
}

async fn principal_of(persistence: &PostgresPersistence, company_id: Uuid, user_id: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
        .bind(company_id)
        .bind(user_id)
        .fetch_one(persistence.pool())
        .await
        .unwrap()
}

fn is_handoff_not_found(error: &AppError) -> bool {
    matches!(error, AppError::NotFound(message) if message == "Thread handoff not found.")
}

fn is_company_not_found(error: &AppError) -> bool {
    matches!(error, AppError::NotFound(message)
        if message == "Company not found, or you do not have permission.")
}

/// Case 26. Never a message that distinguishes "another company's", "a channel you cannot see"
/// and "no such handoff" -- and a non-member does not learn the company exists either.
#[tokio::test]
async fn the_command_route_refuses_another_company_an_unseen_channel_and_a_non_member() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let visible = harness.hold(harness.channel_id).await;
    let restricted = harness.restricted_channel().await;
    let unseen = harness.hold(restricted).await;

    // Another company's handoff, addressed through this company's path.
    let foreign_persistence = harness.persistence.clone();
    let (_, foreign_company_id, foreign_channel_id) =
        company_with_channel(&foreign_persistence, None).await;
    let foreign_thread = ThreadPersistence::create_thread(
        foreign_persistence.as_ref(),
        foreign_channel_id,
        "Somebody else's invoice",
        &[],
    )
    .await
    .unwrap()
    .id;
    let foreign = thread_handoff_fixture(
        foreign_persistence.as_ref(),
        ThreadHandoffFixtureRequest::new(foreign_company_id, foreign_channel_id, foreign_thread),
    )
    .await;

    let cross_company = harness
        .post(
            &harness.owner,
            foreign.handoff_id,
            claim_body(&visible, 1).clone(),
        )
        .await
        .expect_err("a handoff of another company is not addressable here");
    assert!(is_handoff_not_found(&cross_company), "{cross_company:?}");
    assert!(matches!(
        harness.get(&harness.owner, foreign.handoff_id).await,
        Err(ref error) if is_handoff_not_found(error)
    ));

    // A channel the caller cannot view answers the same way, so the two are indistinguishable.
    let hidden = harness
        .post(&harness.member, unseen.handoff_id, claim_body(&unseen, 1))
        .await
        .expect_err("a plain member has no sight of a restricted channel");
    assert!(is_handoff_not_found(&hidden), "{hidden:?}");
    assert_eq!(
        format!("{cross_company:?}"),
        format!("{hidden:?}"),
        "neither refusal may leak which of the two it was"
    );
    assert_eq!(
        harness.stored(&unseen).await.version,
        1,
        "a refused command writes nothing"
    );

    // And somebody who is not on the team does not get as far as the handoff at all.
    let stranger = outsider(harness.persistence.as_ref()).await;
    for refused in [
        harness
            .post(&stranger, visible.handoff_id, claim_body(&visible, 1))
            .await
            .expect_err("a non-member may not command"),
        harness
            .get(&stranger, visible.handoff_id)
            .await
            .expect_err("a non-member may not read"),
    ] {
        assert!(is_company_not_found(&refused), "{refused:?}");
    }
    assert_eq!(harness.stored(&visible).await.version, 1);
}

/// Case 27. An unrecognised operation is refused at the body boundary rather than defaulted --
/// deliberately not `parse_review_override`'s shape, which turns a typo into "inherit".
#[tokio::test]
async fn a_body_the_command_does_not_understand_is_refused_and_changes_nothing() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let hold = harness.hold(harness.channel_id).await;

    for operation in [
        serde_json::json!({ "kind": "resolve" }),
        serde_json::json!({ "kind": "reassign" }),
        serde_json::json!({ "kind": "Claim" }),
        serde_json::json!("claim"),
    ] {
        let mut body = claim_body(&hold, 1);
        body["operation"] = operation.clone();
        assert!(
            serde_json::from_value::<CommandBody>(body).is_err(),
            "{operation} must be a deserialization error, never a default"
        );
    }

    // A body that does deserialize but states an impossible command is the handler's own
    // `BadRequest`, from `ThreadHandoffCommand::validate`.
    let mut invalid = claim_body(&hold, 1);
    invalid["expected_version"] = serde_json::json!(0);
    let refused = harness
        .post(&harness.owner, hold.handoff_id, invalid)
        .await
        .expect_err("version zero is not a version any handoff ever had");
    assert!(matches!(refused, AppError::BadRequest(_)), "{refused:?}");

    let stored = harness.stored(&hold).await;
    assert_eq!(stored.version, 1);
    assert_eq!(stored.responsible_principal_id, None);
}

/// Case 28. The round trip a client performs: claim, then read back what it now holds.
#[tokio::test]
async fn a_claim_through_the_route_is_visible_to_the_read_that_follows_it() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let hold = harness.hold(harness.channel_id).await;

    let before = harness.get(&harness.member, hold.handoff_id).await.unwrap();
    assert_eq!(before["responsible_principal_id"], serde_json::Value::Null);
    assert_eq!(before["version"], 1);
    assert_eq!(
        before["state"],
        ThreadHandoffState::NeedsInstruction.as_str()
    );

    // A plain member claiming unclaimed work is the one case a non-manager may act on.
    let claimed = harness
        .post(&harness.member, hold.handoff_id, claim_body(&hold, 1))
        .await
        .unwrap();
    assert_eq!(claimed, serde_json::json!({ "version": 2 }));

    let after = harness.get(&harness.member, hold.handoff_id).await.unwrap();
    assert_eq!(
        after["responsible_principal_id"],
        serde_json::json!(harness.member_principal_id)
    );
    assert_eq!(after["version"], 2);
    assert_eq!(after["generation"], serde_json::json!(hold.generation));
    assert_eq!(after["id"], serde_json::json!(hold.handoff_id));
    assert_eq!(after["channel_id"], serde_json::json!(hold.channel_id));
}

// -- Phase 4: the four action routes ------------------------------------------------------------

impl Harness {
    async fn post_dismiss(
        &self,
        viewer: &Viewer,
        handoff_id: Uuid,
        body: serde_json::Value,
    ) -> AppResult<serde_json::Value> {
        dismiss(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            State(self.channels.clone()),
            State(self.threads.clone()),
            viewer.clone(),
            Path((self.company_id, handoff_id)),
            Json(serde_json::from_value(body).expect("this test's body is well formed")),
        )
        .await
        .map(|Json(value)| value)
    }
}

fn fences(hold: &Hold, expected_version: u64) -> serde_json::Value {
    serde_json::json!({
        "command_id": Uuid::new_v4(),
        "expected_version": expected_version,
        "expected_generation": hold.generation,
    })
}

/// The dismissal route end to end, and the conflict a stale generation gets.
#[tokio::test]
async fn the_dismiss_route_closes_a_hold_and_refuses_a_replaced_generation() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let hold = harness.hold(harness.channel_id).await;

    let stale = harness
        .post_dismiss(
            &harness.owner,
            hold.handoff_id,
            serde_json::json!({
                "command_id": Uuid::new_v4(),
                "expected_version": 1,
                "expected_generation": Uuid::new_v4(),
            }),
        )
        .await
        .expect_err("that generation is not this thread's");
    let AppError::Conflict(message) = &stale else {
        panic!("{stale:?}")
    };
    assert!(
        message.contains(&hold.generation.to_string()),
        "the conflict names the current generation: {message}"
    );
    assert_eq!(
        harness.stored(&hold).await.state,
        ThreadHandoffState::NeedsInstruction
    );

    let body = harness
        .post_dismiss(&harness.owner, hold.handoff_id, fences(&hold, 1))
        .await
        .unwrap();
    assert_eq!(body["version"], 2);
    assert_eq!(
        harness.stored(&hold).await.state,
        ThreadHandoffState::Dismissed
    );
}

/// Every new route, for another company's handoff id and for a channel the caller cannot view:
/// the same `NotFound`, never a leak of existence.
#[tokio::test]
async fn the_action_routes_refuse_another_companys_hold_and_an_invisible_channel() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let restricted = harness.restricted_channel().await;
    let hidden = harness.hold(restricted).await;
    let elsewhere = Uuid::new_v4();

    for (label, handoff_id) in [
        ("another company's", elsewhere),
        ("invisible", hidden.handoff_id),
    ] {
        let dismissed = harness
            .post_dismiss(&harness.member, handoff_id, fences(&hidden, 1))
            .await
            .expect_err("{label} handoffs are not dismissible");
        assert!(
            matches!(&dismissed, AppError::NotFound(message)
                if message == "Thread handoff not found."),
            "{label} dismiss: {dismissed:?}"
        );

        let drafted = request_draft(
            State(harness.handoffs.clone()),
            State(harness.companies.clone()),
            State(harness.channels.clone()),
            State(harness.threads.clone()),
            harness.member.clone(),
            Path((harness.company_id, handoff_id)),
            Json(
                serde_json::from_value(serde_json::json!({
                    "command_id": Uuid::new_v4(),
                    "expected_version": 1,
                    "expected_generation": hidden.generation,
                    "note_ids": [],
                }))
                .unwrap(),
            ),
        )
        .await
        .expect_err("{label} handoffs cannot be drafted for");
        assert!(
            matches!(&drafted, AppError::NotFound(message)
                if message == "Thread handoff not found."),
            "{label} draft: {drafted:?}"
        );

        let sent = send_draft(
            State(harness.handoffs.clone()),
            State(harness.reviews.clone()),
            State(harness.companies.clone()),
            State(harness.channels.clone()),
            State(harness.threads.clone()),
            harness.member.clone(),
            Path((harness.company_id, handoff_id)),
            Json(
                serde_json::from_value(serde_json::json!({
                    "command_id": Uuid::new_v4(),
                    "expected_version": 1,
                    "expected_generation": hidden.generation,
                    "draft_version": 1,
                }))
                .unwrap(),
            ),
        )
        .await
        .expect_err("{label} handoffs have nothing to send");
        assert!(
            matches!(&sent, AppError::NotFound(message)
                if message == "Thread handoff not found."),
            "{label} send: {sent:?}"
        );
    }
}

/// A body with a field none of these routes knows is a deserialization error, never a silent
/// default -- the same rule Phase 3 applied to an unrecognised operation kind.
#[test]
fn an_unknown_field_is_refused_by_every_action_body() {
    let base = serde_json::json!({
        "command_id": Uuid::new_v4(),
        "expected_version": 1,
        "expected_generation": Uuid::new_v4(),
    });
    let with_extra = |extra: serde_json::Value| {
        let mut body = base.clone();
        let object = body.as_object_mut().unwrap();
        for (key, value) in extra.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        body
    };

    assert!(serde_json::from_value::<DismissBody>(base.clone()).is_ok());
    assert!(
        serde_json::from_value::<DismissBody>(with_extra(serde_json::json!({ "force": true })))
            .is_err()
    );
    assert!(
        serde_json::from_value::<DraftBody>(with_extra(
            serde_json::json!({ "note_ids": [], "prompt": "go" })
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendBody>(with_extra(
            serde_json::json!({ "draft_version": 1, "rationale": "looks fine" })
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendEditedBody>(with_extra(serde_json::json!({
            "draft_version": 1,
            "subject": "Re: invoice",
            "body": "Thirty days.",
            "recipient_to": "ana@client.test",
            "bcc": ["nobody@example.com"],
        })))
        .is_err()
    );
    // And the well-formed shapes still deserialize, so the rule above is a bound rather than a
    // blanket refusal.
    assert!(
        serde_json::from_value::<SendEditedBody>(with_extra(serde_json::json!({
            "draft_version": 1,
            "subject": "Re: invoice",
            "body": "Thirty days.",
            "recipient_to": "ana@client.test",
        })))
        .is_ok()
    );
}

/// The draft route refuses an unclaimed hold with the sentence that says what to do about it.
#[tokio::test]
async fn the_draft_route_asks_for_a_claim_before_it_starts_a_run() {
    let Some(harness) = Harness::new().await else {
        return;
    };
    let hold = harness.hold(harness.channel_id).await;

    let refused = request_draft(
        State(harness.handoffs.clone()),
        State(harness.companies.clone()),
        State(harness.channels.clone()),
        State(harness.threads.clone()),
        harness.owner.clone(),
        Path((harness.company_id, hold.handoff_id)),
        Json(
            serde_json::from_value(serde_json::json!({
                "command_id": Uuid::new_v4(),
                "expected_version": 1,
                "expected_generation": hold.generation,
                "note_ids": [],
            }))
            .unwrap(),
        ),
    )
    .await
    .expect_err("an unclaimed reply has no assigned reviewer yet");
    assert!(
        matches!(&refused, AppError::BadRequest(_) | AppError::Conflict(_)),
        "{refused:?}"
    );
    assert_eq!(
        harness.stored(&hold).await.state,
        ThreadHandoffState::NeedsInstruction
    );
}
