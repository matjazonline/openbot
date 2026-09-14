//! Route tests for reply handling, on a real database.
//!
//! Every route of the feature is exercised here rather than split across the channel and company
//! modules, so the authorization rules and the two-write channel save can be read in one place.
//! Each test scopes every assertion to the company it creates: the test database is shared and
//! these run in parallel.

use std::sync::Arc;

use axum::{
    Form, Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use super::*;
use crate::adapters::http::routes::{channel, company};
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::test_pool},
    entities::{thread_handoff::ExternalReplyHandling, user::Viewer, value_objects::EmailAddress},
    infra::config::AppConfig,
    services::memory_provider::ConfiguredMemoryProviders,
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        memory::MemoryUseCases,
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

/// Everything the reply-handling routes are called with, over one real database.
struct Harness {
    persistence: Arc<PostgresPersistence>,
    companies: Arc<CompanyUseCases>,
    channels: Arc<ChannelUseCases>,
    handoffs: Arc<ThreadHandoffUseCases>,
    memory: Arc<MemoryUseCases>,
    config: Arc<AppConfig>,
    owner: Viewer,
    company_id: Uuid,
    channel_id: Uuid,
}

impl Harness {
    /// One company with one channel. Naming a participant makes the channel an allowlist channel,
    /// which only granted principals and the company owner may read.
    async fn new(participant_emails: Option<Vec<String>>) -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let (owner, company_id, channel_id) =
            company_with_channel(&persistence, participant_emails).await;
        Some(Self {
            companies: Arc::new(CompanyUseCases::new(persistence.clone())),
            channels: Arc::new(ChannelUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                test_config(),
            )),
            handoffs: Arc::new(ThreadHandoffUseCases::new(persistence.clone())),
            memory: Arc::new(MemoryUseCases::new(
                persistence.clone(),
                persistence.clone(),
                persistence.clone(),
                ConfiguredMemoryProviders::from_iter([]),
            )),
            config: test_config(),
            persistence,
            owner,
            company_id,
            channel_id,
        })
    }

    async fn page(&self, viewer: &Viewer, channel_id: Uuid) -> AppResult<String> {
        let rendered = reply_handling_policy(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            State(self.channels.clone()),
            viewer.clone(),
            Query(PolicyQuery {
                company_id: self.company_id,
                channel_id,
            }),
        )
        .await?;
        Ok(rendered.0)
    }

    async fn save_company_default(
        &self,
        user_id: Uuid,
        policy: ExternalReplyHandling,
    ) -> AppResult<String> {
        let rendered = update_company_reply_handling(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            AuthenticatedUser { id: user_id },
            Form(CompanyPolicyForm {
                company_id: self.company_id,
                channel_id: self.channel_id,
                policy,
            }),
        )
        .await?;
        Ok(rendered.0)
    }

    async fn save_channel_override(&self, user_id: Uuid, submitted: &str) -> AppResult<String> {
        let rendered = update_channel_reply_handling(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            AuthenticatedUser { id: user_id },
            Form(ChannelPolicyForm {
                company_id: self.company_id,
                channel_id: self.channel_id,
                policy_override: submitted.to_string(),
            }),
        )
        .await?;
        Ok(rendered.0)
    }

    async fn stored_override(&self) -> Option<ExternalReplyHandling> {
        self.handoffs
            .reply_handling_policy(self.company_id, self.channel_id)
            .await
            .unwrap()
            .expect("the channel exists")
            .channel_override
    }

    async fn stored_company_default(&self) -> ExternalReplyHandling {
        self.handoffs
            .reply_handling_policy(self.company_id, self.channel_id)
            .await
            .unwrap()
            .expect("the channel exists")
            .company_default
    }

    async fn get_reply_handling(&self, channel_id: Uuid) -> AppResult<serde_json::Value> {
        let response = channel::get_reply_handling_json(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            AuthenticatedUser {
                id: self.owner.user_id,
            },
            Path((self.company_id, channel_id)),
        )
        .await?;
        Ok(json_body(response.into_response()).await)
    }

    async fn put_reply_handling(
        &self,
        channel_id: Uuid,
        policy: Option<ExternalReplyHandling>,
    ) -> AppResult<serde_json::Value> {
        let response = channel::put_reply_handling_json(
            State(self.handoffs.clone()),
            State(self.companies.clone()),
            AuthenticatedUser {
                id: self.owner.user_id,
            },
            Path((self.company_id, channel_id)),
            Json(channel::ReplyHandlingPayload {
                channel_override: policy,
            }),
        )
        .await?;
        Ok(json_body(response.into_response()).await)
    }

    async fn put_channel(&self, policy: Option<ExternalReplyHandling>) -> AppResult<()> {
        channel::update_channel_json(
            State(self.channels.clone()),
            State(self.handoffs.clone()),
            AuthenticatedUser {
                id: self.owner.user_id,
            },
            Path((self.company_id, self.channel_id)),
            Json(channel::ChannelJsonPayload {
                response_trigger: None,
                name: "Support".into(),
                description: None,
                slug: None,
                alias_slugs: None,
                system_prompt: None,
                participant_emails: None,
                agent_ids: None,
                confirm_spam_disabled: Some(true),
                // The fixture channel has no agent, and an enabled channel must have one. This
                // test is about the policy write that follows the channel write, not about agents.
                enabled: Some(false),
                add_3rd_party: Some(true),
                external_response_review_override: None,
                external_reply_handling_override: policy,
                preferred_reviewer_principal_id: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
            }),
        )
        .await
        .map(|_| ())
    }

    async fn put_company(&self, body: serde_json::Value) -> AppResult<()> {
        let form: company::CompanyForm = serde_json::from_value(body).unwrap();
        company::update_company_json(
            State(self.companies.clone()),
            State(self.memory.clone()),
            State(self.config.clone()),
            AuthenticatedUser {
                id: self.owner.user_id,
            },
            Path(self.company_id),
            Json(form),
        )
        .await
        .map(|_| ())
    }
}

async fn json_body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// A company with one channel, and the viewer who owns it.
async fn company_with_channel(
    persistence: &PostgresPersistence,
    participant_emails: Option<Vec<String>>,
) -> (Viewer, Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-route-{suffix}@example.com");
    persistence
        .create_user(&format!("handoff-route-{suffix}"), &email, "hash")
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
            name: "Reply Handling Routes".into(),
            slug: format!("handoff-route-{suffix}"),
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
            slug: format!("handoff-route-support-{suffix}"),
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

/// A second person in the company, with a principal so channel reads can place them.
async fn add_member(persistence: &PostgresPersistence, company_id: Uuid, role: &str) -> Viewer {
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("handoff-member-{suffix}@example.com");
    let user = persistence
        .create_user(&format!("handoff-member-{suffix}"), &email, "hash")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(user.id)
    .bind(role)
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

fn is_company_not_found(error: &AppError) -> bool {
    matches!(error, AppError::NotFound(message)
        if message == "Company not found, or you do not have permission.")
}

#[tokio::test]
async fn the_page_names_where_the_effective_value_came_from() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };
    let owner = harness.owner.user_id;

    let page = harness
        .page(&harness.owner, harness.channel_id)
        .await
        .unwrap();
    assert!(page.contains("Effective policy: Automatic (inherited from company)"));

    harness
        .save_channel_override(owner, "manual_handoff")
        .await
        .unwrap();
    let page = harness
        .page(&harness.owner, harness.channel_id)
        .await
        .unwrap();
    assert!(page.contains("Effective policy: Manual handoff (channel override)"));

    harness
        .save_company_default(owner, ExternalReplyHandling::ManualHandoff)
        .await
        .unwrap();
    harness
        .save_channel_override(owner, "inherit")
        .await
        .unwrap();
    let page = harness
        .page(&harness.owner, harness.channel_id)
        .await
        .unwrap();
    assert!(page.contains("Effective policy: Manual handoff (inherited from company)"));
}

#[tokio::test]
async fn the_channel_save_sets_clears_and_refuses_a_value_the_policy_does_not_name() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };
    let owner = harness.owner.user_id;

    harness
        .save_channel_override(owner, "manual_handoff")
        .await
        .unwrap();
    assert_eq!(
        harness.stored_override().await,
        Some(ExternalReplyHandling::ManualHandoff)
    );

    let refused = harness
        .save_channel_override(owner, "nonsense")
        .await
        .expect_err("a value the policy does not name is a bad request");
    assert!(matches!(refused, AppError::BadRequest(_)));
    assert_eq!(
        harness.stored_override().await,
        Some(ExternalReplyHandling::ManualHandoff),
        "a refused save must change nothing"
    );

    harness
        .save_channel_override(owner, "inherit")
        .await
        .unwrap();
    assert_eq!(harness.stored_override().await, None);
}

#[tokio::test]
async fn a_member_who_does_not_manage_the_company_is_refused_by_all_three_routes() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };
    let member = add_member(&harness.persistence, harness.company_id, "member").await;

    let page = harness
        .page(&member, harness.channel_id)
        .await
        .expect_err("a plain member may not read the policy");
    assert!(is_company_not_found(&page));

    let company_save = harness
        .save_company_default(member.user_id, ExternalReplyHandling::ManualHandoff)
        .await
        .expect_err("a plain member may not set the company default");
    assert!(is_company_not_found(&company_save));

    let channel_save = harness
        .save_channel_override(member.user_id, "manual_handoff")
        .await
        .expect_err("a plain member may not set the channel override");
    assert!(is_company_not_found(&channel_save));

    assert_eq!(harness.stored_override().await, None);
    assert_eq!(
        harness.stored_company_default().await,
        ExternalReplyHandling::Automatic
    );
}

#[tokio::test]
async fn a_manager_without_a_view_grant_cannot_read_a_restricted_channels_policy() {
    let Some(harness) = Harness::new(Some(vec!["outsider@example.com".to_string()])).await else {
        return;
    };
    let admin = add_member(&harness.persistence, harness.company_id, "admin").await;

    // The admin manages company operations, so authorization is not what refuses the read below.
    harness
        .save_company_default(admin.user_id, ExternalReplyHandling::ManualHandoff)
        .await
        .expect("an admin manages company operations");

    let refused = harness
        .page(&admin, harness.channel_id)
        .await
        .expect_err("a restricted channel is not readable without a view grant");
    assert!(matches!(refused, AppError::NotFound(message) if message == "Channel not found."));

    assert!(
        harness
            .page(&harness.owner, harness.channel_id)
            .await
            .is_ok(),
        "the owner still reads it, so the refusal is the missing grant and not the channel"
    );
}

#[tokio::test]
async fn the_json_pair_round_trips_and_refuses_another_companys_channel() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };
    let (_, _, foreign_channel_id) = company_with_channel(&harness.persistence, None).await;

    let written = harness
        .put_reply_handling(
            harness.channel_id,
            Some(ExternalReplyHandling::ManualHandoff),
        )
        .await
        .unwrap();
    assert_eq!(
        written,
        serde_json::json!({
            "company_default": "automatic",
            "channel_override": "manual_handoff",
            "effective": "manual_handoff",
            "inherited": false,
        })
    );
    assert_eq!(
        harness
            .get_reply_handling(harness.channel_id)
            .await
            .unwrap(),
        written,
        "the read must return what the write stored"
    );

    let cleared = harness
        .put_reply_handling(harness.channel_id, None)
        .await
        .unwrap();
    assert_eq!(
        cleared,
        serde_json::json!({
            "company_default": "automatic",
            "channel_override": null,
            "effective": "automatic",
            "inherited": true,
        })
    );

    // A channel of another company must answer exactly like a missing one, on both verbs.
    for channel_id in [foreign_channel_id, Uuid::new_v4()] {
        assert!(matches!(
            harness.get_reply_handling(channel_id).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            harness
                .put_reply_handling(channel_id, Some(ExternalReplyHandling::ManualHandoff))
                .await,
            Err(AppError::NotFound(_))
        ));
    }
    assert!(
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT external_reply_handling_override FROM channels WHERE id = $1"
        )
        .bind(foreign_channel_id)
        .fetch_one(harness.persistence.pool())
        .await
        .unwrap()
        .is_none(),
        "the refused write must not have reached the other company's channel"
    );
}

#[tokio::test]
async fn a_channel_put_applies_the_override_it_carries_and_clears_it_when_absent() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };

    harness
        .put_channel(Some(ExternalReplyHandling::ManualHandoff))
        .await
        .unwrap();
    assert_eq!(
        harness.stored_override().await,
        Some(ExternalReplyHandling::ManualHandoff)
    );

    harness.put_channel(None).await.unwrap();
    assert_eq!(
        harness.stored_override().await,
        None,
        "a PUT replaces rather than patches, so an absent override returns the channel to inherit"
    );
}

#[tokio::test]
async fn a_company_put_changes_reply_handling_only_when_it_names_it() {
    let Some(harness) = Harness::new(None).await else {
        return;
    };
    let company = harness
        .companies
        .get_company(harness.company_id)
        .await
        .unwrap()
        .unwrap();
    let name = company.name.clone();
    let slug = company.slug.to_string();

    harness
        .put_company(serde_json::json!({
            "name": name,
            "slug": slug,
            "external_reply_handling": "manual_handoff",
        }))
        .await
        .unwrap();
    assert_eq!(
        harness.stored_company_default().await,
        ExternalReplyHandling::ManualHandoff
    );

    harness
        .put_company(serde_json::json!({ "name": name, "slug": slug }))
        .await
        .unwrap();
    assert_eq!(
        harness.stored_company_default().await,
        ExternalReplyHandling::ManualHandoff,
        "an omitted field preserves rather than resets"
    );

    harness
        .put_company(serde_json::json!({
            "name": name,
            "slug": slug,
            "external_reply_handling": "automatic",
        }))
        .await
        .unwrap();
    assert_eq!(
        harness.stored_company_default().await,
        ExternalReplyHandling::Automatic
    );
}
