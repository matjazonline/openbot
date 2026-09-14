use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::test_pool},
    entities::channel::ChannelResponseTrigger,
    use_cases::{
        channel::ChannelPersistence,
        company::{CompanyPersistence, CompanyWrite},
        thread_handoff::ThreadHandoffUseCases,
        user::UserPersistence,
    },
};
use axum::{Json, extract::State};

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

struct Fixture {
    persistence: Arc<PostgresPersistence>,
    company: Company,
    user_id: Uuid,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("response-{suffix}@example.com");
        persistence
            .create_user(&format!("response-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let user = UserPersistence::get_by_email(persistence.as_ref(), &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            user.id,
            CompanyWrite {
                name: "Response settings".into(),
                slug: format!("response-{suffix}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        Some(Self {
            persistence,
            company,
            user_id: user.id,
        })
    }

    fn workspace(&self) -> Workspace {
        let p = &self.persistence;
        Workspace {
            company_use_cases: Arc::new(CompanyUseCases::new(p.clone())),
            channel_use_cases: Arc::new(ChannelUseCases::new(
                p.clone(),
                p.clone(),
                p.clone(),
                test_config(),
            )),
            schedule_use_cases: Arc::new(ScheduleUseCases::new(
                p.clone(),
                p.clone(),
                p.clone(),
                p.clone(),
                p.clone(),
            )),
            agent_use_cases: Arc::new(AgentUseCases::new(
                p.clone(),
                p.clone(),
                p.clone(),
                crate::use_cases::agent::SpamScanning::Unavailable,
            )),
            user_use_cases: Arc::new(UserUseCases::new(
                Arc::new(crate::infra::argon2_password_hasher()),
                p.clone(),
            )),
            config: test_config(),
            user_id: self.user_id,
        }
    }

    async fn stored(&self, id: Uuid) -> Channel {
        ChannelPersistence::get_by_id(self.persistence.as_ref(), id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn save(&self, id: Uuid, body: serde_json::Value) -> String {
        let response = update_channel(
            self.workspace(),
            Path(id),
            Query(CompanyQuery {
                company_id: self.company.id,
            }),
            Form(serde_json::from_value(body).unwrap()),
        )
        .await
        .unwrap();
        body_text(response).await
    }
}

async fn body_text(response: Response) -> String {
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn form(trigger: &str) -> serde_json::Value {
    serde_json::json!({"name":"Support", "slug":"support", "form_mode":"advanced", "response_trigger":trigger})
}

#[test]
fn response_trigger_form_and_json_parsing_agree() {
    for (wire, expected) in [
        ("always", ChannelResponseTrigger::Always),
        ("mentioned", ChannelResponseTrigger::Mentioned),
        (
            "mentioned_or_reply",
            ChannelResponseTrigger::MentionedOrReplyToAgent,
        ),
    ] {
        let submitted = SubmittedChannel::new(serde_json::from_value(form(wire)).unwrap());
        assert_eq!(submitted.write(None).unwrap().response_trigger, expected);
        assert_eq!(submitted.draft().response_trigger, expected);
        let payload: super::super::channel::ChannelJsonPayload =
            serde_json::from_value(form(wire)).unwrap();
        assert_eq!(payload.response_trigger, Some(expected));
    }
    let submitted = SubmittedChannel::new(
        serde_json::from_value(serde_json::json!({"name":"Support"})).unwrap(),
    );
    assert_eq!(
        submitted.write(None).unwrap().response_trigger,
        ChannelResponseTrigger::Always
    );
    let submitted = SubmittedChannel::new(serde_json::from_value(form("invalid")).unwrap());
    assert!(submitted.write(None).is_err());
    assert!(
        serde_json::from_value::<super::super::channel::ChannelJsonPayload>(form("invalid"))
            .is_err()
    );
}

#[tokio::test]
async fn response_trigger_ui_creates_updates_reloads_and_retains_rejected_selection() {
    let Some(fx) = Fixture::new().await else {
        return;
    };
    let response = create_channel(
        fx.workspace(),
        Query(CompanyQuery {
            company_id: fx.company.id,
        }),
        Form(serde_json::from_value(form("mentioned")).unwrap()),
    )
    .await
    .unwrap();
    let html = body_text(response).await;
    assert!(html.contains("value=\"mentioned\" selected"), "{html}");
    let channels = ChannelPersistence::list_by_company_id(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
    let channel = &channels[0];
    assert_eq!(channel.response_trigger, ChannelResponseTrigger::Mentioned);
    let html = fx.save(channel.id, form("mentioned_or_reply")).await;
    assert!(html.contains("value=\"mentioned_or_reply\" selected"));
    assert_eq!(
        fx.stored(channel.id).await.response_trigger,
        ChannelResponseTrigger::MentionedOrReplyToAgent
    );
    let html = body_text(
        edit_pane(
            fx.workspace(),
            Path(channel.id),
            Query(CompanyQuery {
                company_id: fx.company.id,
            }),
        )
        .await
        .unwrap()
        .into_response(),
    )
    .await;
    assert!(html.contains("value=\"mentioned_or_reply\" selected"));
    let mut invalid = form("mentioned");
    invalid["slug"] = serde_json::json!("quiet");
    let html = fx.save(channel.id, invalid).await;
    assert!(html.contains("Failed to save channel"));
    assert!(html.contains("value=\"mentioned\" selected"));
    assert_eq!(
        fx.stored(channel.id).await.response_trigger,
        ChannelResponseTrigger::MentionedOrReplyToAgent
    );
    let html = fx.save(channel.id, form("invalid")).await;
    assert!(html.contains("invalid channel response trigger"));
    assert_eq!(
        fx.stored(channel.id).await.response_trigger,
        ChannelResponseTrigger::MentionedOrReplyToAgent
    );
    fx.save(
        channel.id,
        serde_json::json!({"name":"Support", "slug":"support"}),
    )
    .await;
    assert_eq!(
        fx.stored(channel.id).await.response_trigger,
        ChannelResponseTrigger::Always
    );
}

#[tokio::test]
async fn response_trigger_json_round_trip_and_omission_default() {
    let Some(fx) = Fixture::new().await else {
        return;
    };
    let channel = ChannelPersistence::create(
        fx.persistence.as_ref(),
        fx.company.id,
        ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for trigger in [Some("mentioned"), Some("mentioned_or_reply"), None] {
        let mut body = serde_json::json!({"name":"Support", "slug":"support", "enabled":false});
        if let Some(trigger) = trigger {
            body["response_trigger"] = serde_json::json!(trigger);
        }
        let response = super::super::channel::update_channel_json(
            State(fx.workspace().channel_use_cases),
            State(Arc::new(ThreadHandoffUseCases::new(fx.persistence.clone()))),
            AuthenticatedUser { id: fx.user_id },
            Path((fx.company.id, channel.id)),
            Json(serde_json::from_value(body).unwrap()),
        )
        .await
        .unwrap()
        .into_response();
        let json: serde_json::Value = serde_json::from_str(&body_text(response).await).unwrap();
        let expected = trigger.unwrap_or("always");
        assert_eq!(json["channel"]["response_trigger"], expected);
        assert_eq!(
            fx.stored(channel.id).await.response_trigger.as_str(),
            expected
        );
    }
}

#[tokio::test]
async fn response_trigger_owned_channel_form_saves_and_reopens() {
    use crate::use_cases::agent::{AgentWrite, OwnedAgentChannelPersistence};
    let Some(fx) = Fixture::new().await else {
        return;
    };
    let (agent, channel) = OwnedAgentChannelPersistence::create_owned_agent_channel(
        fx.persistence.as_ref(),
        fx.company.id,
        AgentWrite {
            name: "Support".into(),
            slug: "support".into(),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
        ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for trigger in ["mentioned", "mentioned_or_reply"] {
        let mut body = form(trigger);
        body["agent_ids"] = serde_json::json!(agent.id.to_string());
        body["enabled"] = serde_json::json!("true");
        let html = fx.save(channel.id, body).await;
        assert!(
            html.contains(&format!("value=\"{trigger}\" selected")),
            "{html}"
        );
        assert_eq!(
            fx.stored(channel.id).await.response_trigger.as_str(),
            trigger
        );
        let html = edit_pane(
            fx.workspace(),
            Path(channel.id),
            Query(CompanyQuery {
                company_id: fx.company.id,
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(html.contains(&format!("value=\"{trigger}\" selected")));
    }
}

#[tokio::test]
async fn response_trigger_sql_default_and_invalid_value_constraint() {
    let Some(fx) = Fixture::new().await else {
        return;
    };
    let mut tx = fx.persistence.pool().begin().await.unwrap();
    let provenance =
        serde_json::to_value(crate::entities::creation::CreationProvenance::system()).unwrap();
    let trigger: String = sqlx::query_scalar(
        "INSERT INTO channels (id, company_id, name, created_by) VALUES ($1, $2, 'Default', $3) RETURNING response_trigger"
    ).bind(Uuid::new_v4()).bind(fx.company.id).bind(&provenance).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(trigger, "always");
    let error = sqlx::query(
        "INSERT INTO channels (id, company_id, name, created_by, response_trigger) VALUES ($1, $2, 'Invalid', $3, 'unexpected')"
    ).bind(Uuid::new_v4()).bind(fx.company.id).bind(provenance).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("channels_response_trigger_check")
    );
    tx.rollback().await.unwrap();
}
