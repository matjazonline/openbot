use super::*;
use crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor;
use crate::adapters::protocols::email::parser::RawInboundPayload;
use crate::domain::monitoring::MonitoringService;
use crate::entities::channel::{Channel, ChannelAccessMode};
use crate::entities::company::CompanyAccess;
use crate::entities::company_member::CompanyMembership;
use crate::entities::correlation::CorrelationId;
use crate::entities::message::MessageDirection;
use crate::infra::config::ResendApiConfig;
use crate::transport::InboundOrigin;
use crate::transport::{MAX_ATTACHMENTS, MAX_BODY_BYTES, MAX_INGRESS_HOPS};
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::use_cases::company::CompanyWrite;
use crate::use_cases::participant::test_support::{
    InMemoryParticipantDirectory, TeamFixture, email_allowlist_grants, email_allowlist_policy,
};
use crate::use_cases::thread::test_support::InMemoryThreads;
use chrono::Utc;
use std::sync::Mutex;

fn internal_test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "smtp.invalid".to_string(),
        smtp_port: 2525,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    })
}

struct MockCompanyPersistence {
    companies: Mutex<Vec<Company>>,
    team_members: Mutex<Vec<(Uuid, String)>>,
}

impl MockCompanyPersistence {
    fn new(companies: Vec<Company>) -> Self {
        Self {
            companies: Mutex::new(companies),
            team_members: Mutex::new(Vec::new()),
        }
    }

    fn with_team_members(companies: Vec<Company>, members: Vec<(Uuid, String)>) -> Self {
        Self {
            companies: Mutex::new(companies),
            team_members: Mutex::new(members),
        }
    }
}

/// A flat team: this double knows who belongs to a company, not who owns it, so it never answers
/// `Owner`. The owner exemption is the entity's rule and is tested there.
///
/// With no explicit team, membership falls back to the names these tests use for outsiders, so a
/// scenario that only cares about one hostile sender does not have to enumerate everyone else.
#[async_trait]
impl TeamFixture for MockCompanyPersistence {
    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership> {
        let members = self.team_members.lock().unwrap();
        let clean = email.trim().to_lowercase();
        let on_the_team = if members.is_empty() {
            !(clean.contains("spammer")
                || clean.contains("unauthorized")
                || clean.contains("evil")
                || clean.contains("notallowed"))
        } else {
            members
                .iter()
                .any(|(cid, e)| *cid == company_id && e.eq_ignore_ascii_case(&clean))
        };

        Ok(if on_the_team {
            CompanyMembership::Member
        } else {
            CompanyMembership::None
        })
    }

    async fn company_access(
        &self,
        _user_id: Uuid,
        _company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        unimplemented!("ingest knows a sender by address, never by account")
    }
}

#[async_trait]
impl CompanyPersistence for MockCompanyPersistence {
    async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
        unimplemented!()
    }
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|company| company.id == id)
            .cloned())
    }
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.slug == slug)
            .cloned())
    }
    async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
        unimplemented!()
    }
    async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
        unimplemented!()
    }
    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!()
    }

    /// This double never reaches a provider call: the tests around it assert on ingestion and
    /// threading, and an agent run that gets this far fails at parameter resolution by design.
    async fn list_model_connections(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
        Ok(Vec::new())
    }

    async fn model_api_key(
        &self,
        _company_id: Uuid,
        _provider: &crate::entities::value_objects::ModelProvider,
    ) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn replace_model_connections_for_user(
        &self,
        _user_id: Uuid,
        _company_id: Uuid,
        _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        unimplemented!("this double is not exercised on the model-connection write path")
    }
}

struct MockChannelPersistence {
    channels: Mutex<Vec<Channel>>,
}

#[async_trait]
impl ChannelPersistence for MockChannelPersistence {
    async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
        unimplemented!()
    }
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .find(|channel| channel.id == id)
            .cloned())
    }
    async fn get_by_company_slug_and_channel_slug(
        &self,
        _company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .find(|w| w.matches_slug(channel_slug))
            .cloned())
    }
    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w.company_id == company_id)
            .cloned()
            .collect())
    }
    async fn update(
        &self,
        _request: crate::use_cases::channel::ChannelUpdate,
    ) -> AppResult<crate::entities::channel::Channel> {
        unimplemented!()
    }
    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!()
    }
}

/// One inter-channel hop, as the email sender hands it to the relay.
///
/// The relay is reached through [`InternalMailRelay`] with the mail already frozen. The shape this
/// replaces rendered an `OutboundEmail` to RFC 5322 first, purely so the ingress path had
/// something email-shaped to read back -- so a test had to build a full outbound email to exercise
/// a hop budget.
struct InternalHop {
    source_channel_id: Uuid,
    from: EmailAddress,
    to: EmailAddress,
    subject: String,
    body: String,
    message_id: MessageId,
    in_reply_to: Option<MessageId>,
    references: Vec<MessageId>,
    /// The hop count this message *carries*, which is what the sender hands the relay: the
    /// renderer has already added one for the hop the mail itself is.
    hop_count: u32,
    trace_channels: Vec<Uuid>,
    correlation_id: CorrelationId,
}

impl InternalHop {
    fn new(source_channel_id: Uuid, from: &str, to: &str, subject: &str, body: &str) -> Self {
        Self {
            source_channel_id,
            from: EmailAddress::from(from),
            to: EmailAddress::from(to),
            subject: subject.to_string(),
            body: body.to_string(),
            message_id: MessageId::from(format!("<{}@mailagents.com>", Uuid::new_v4())),
            in_reply_to: None,
            references: Vec::new(),
            hop_count: 0,
            trace_channels: Vec::new(),
            correlation_id: CorrelationId::new(),
        }
    }
}

/// Hand one hop to the relay, the way the email sender does before reaching SMTP.
async fn relay_hop(
    use_cases: &ThreadUseCases,
    hop: &InternalHop,
) -> crate::transport::RelayDisposition {
    let target =
        crate::adapters::protocols::email::EmailChannelSelectorParser::new("mailagents.com")
            .parse(hop.to.as_str())
            .expect("the test recipient is internal")
            .primary()
            .clone();
    use_cases
        .relay_internal(&crate::transport::InternalRelayMail {
            target: &target,
            from: &hop.from,
            recipient_to: &hop.to,
            subject: &hop.subject,
            body_text: &hop.body,
            message_id: &hop.message_id,
            in_reply_to: hop.in_reply_to.as_ref(),
            references: &hop.references,
            source_channel_id: hop.source_channel_id,
            hop_count: hop.hop_count,
            trace: hop.trace_channels.clone(),
            correlation_id: hop.correlation_id,
        })
        .await
        .expect("the relay reached a verdict")
}

/// The refusal a relayed hop carries, or `None` when it was accepted.
fn refusal(disposition: &crate::transport::RelayDisposition) -> Option<&str> {
    match disposition {
        crate::transport::RelayDisposition::Refused(reason) => Some(reason.as_str()),
        _ => None,
    }
}

#[tokio::test]
async fn test_inter_channel_hop_limit_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let source_channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_id,
                company_id,
                name: "Inbound Flow".to_string(),
                description: None,
                slug: "inbound".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: source_channel_id,
                company_id,
                name: "Source Flow".to_string(),
                description: None,
                slug: "source".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let disposition = relay_hop(
        &thread_use_cases,
        &InternalHop {
            hop_count: MAX_INGRESS_HOPS,
            in_reply_to: Some("<source@example.com>".into()),
            ..InternalHop::new(
                source_channel_id,
                "source@acme.mailagents.com",
                "inbound@acme.mailagents.com",
                "Test Inter Channel",
                "Hello",
            )
        },
    )
    .await;

    // Refused, and definitely so: the receiving channel will say the same thing next time, so the
    // sender records a terminal outcome rather than spending five attempts on it.
    assert_eq!(
        refusal(&disposition),
        Some("Max inter-channel hop count reached")
    );
}

#[tokio::test]
async fn test_spf_authentication_failure_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            description: None,
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["@public".into()]),
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "forged@acme.mailagents.com".to_string(),
        subject: Some("Spoofed email".to_string()),
        text: Some("Hello".to_string()),
        headers: Some(format!("X-MailAgents-Channel-ID: {channel_id}\n")),
        spf: crate::entities::auth::AuthVerdict::Fail,
        dmarc: crate::entities::auth::AuthVerdict::Unknown,
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("DMARC authentication did not pass"));
}

#[tokio::test]
async fn test_high_spam_score_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            description: None,
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["@public".into()]),
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "inbound@acme.mailagents.com".to_string(),
        from: "spammer@external.com".to_string(),
        subject: Some("Buy Cheap Rolex".to_string()),
        text: Some("Spam message body".to_string()),
        spam_score: Some(8.5),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("Spam score threshold exceeded"));
}

#[tokio::test]
async fn test_dmarc_authentication_failure_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            description: None,
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "inbound@acme.mailagents.com".to_string(),
        from: "spoofed@external.com".to_string(),
        subject: Some("Spoofed email".to_string()),
        text: Some("Hello".to_string()),
        dmarc: crate::entities::auth::AuthVerdict::Fail,
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("DMARC authentication did not pass"));
}

#[tokio::test]
async fn test_unauthorized_sender_blocked_before_spam_checks() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Restricted Flow".to_string(),
            description: None,
            slug: "restricted".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["alice@example.com".into()]),
            access_mode: ChannelAccessMode::Allowlist,
            principal_grants: email_allowlist_grants(company_id, &["alice@example.com"]),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "unauthorized@evil.com".to_string(),
        subject: Some("Hello".to_string()),
        text: Some("Test message".to_string()),
        spam_score: Some(10.0), // high spam score
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("Sender unauthorized for channel"));
}

#[tokio::test]
async fn test_participant_sender_bypasses_spam_checks() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Restricted Flow".to_string(),
            description: None,
            slug: "restricted".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["alice@example.com".into()]),
            access_mode: ChannelAccessMode::Allowlist,
            principal_grants: email_allowlist_grants(company_id, &["alice@example.com"]),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // High spam score payload from a registered participant
    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "alice@example.com".to_string(),
        subject: Some("Urgent update".to_string()),
        text: Some("Meeting notes".to_string()),
        spam_score: Some(99.0), // extreme spam score that would normally be rejected
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(result.thread.is_some());
}

#[tokio::test]
async fn test_channel_in_cc_resolves_properly() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Flow".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "user@example.com".to_string(),
        cc: Some("support@acme.mailagents.com".to_string()),
        from: "customer@client.com".to_string(),
        subject: Some("Need help".to_string()),
        text: Some("Can someone assist?".to_string()),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();

    assert!(result.accepted);
    // Copying a channel records its intent; whether a Cc starts anything is its bindings' call.
    assert_eq!(result.intent_ids.len(), 1);
    assert_eq!(result.channel_matches.len(), 1);
    assert_eq!(result.channel_matches[0].recipient_role, RecipientRole::Cc);
    assert_eq!(thread_persistence.messages().len(), 1);
}

#[tokio::test]
async fn test_multi_channel_to_and_cc_execution() {
    let company_id = Uuid::new_v4();
    let wf1_id = Uuid::new_v4();
    let wf2_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf1_id,
                company_id,
                name: "Support".to_string(),
                description: None,
                slug: "support".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf2_id,
                company_id,
                name: "Billing".to_string(),
                description: None,
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        cc: Some("billing@acme.mailagents.com".to_string()),
        from: "customer@client.com".to_string(),
        subject: Some("Invoice and account question".to_string()),
        text: Some("Please help with my account and invoice.".to_string()),
        ..Default::default()
    };

    let ingest = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();

    assert!(ingest.accepted);
    assert_eq!(ingest.channel_matches.len(), 2);
    assert_eq!(ingest.channel_matches[0].channel.slug, "support");
    assert_eq!(ingest.channel_matches[0].recipient_role, RecipientRole::To);
    assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
    assert_eq!(ingest.channel_matches[1].recipient_role, RecipientRole::Cc);

    // Verify thread creation for both channels
    let threads = thread_persistence.threads();
    assert_eq!(threads.len(), 2);
    drop(threads);

    // One intent per association, each with its own role; whether the `Cc`'d copy starts anything
    // is its bindings' trigger, not ingress's decision. Nothing is queued as a legacy task.
    assert_eq!(ingest.intent_ids.len(), 2);
    let request = thread_persistence
        .committed_requests()
        .last()
        .cloned()
        .unwrap();
    let roles: Vec<_> = request
        .intents
        .iter()
        .map(|intent| (intent.channel_id, intent.role))
        .collect();
    assert_eq!(
        roles,
        vec![
            (ingest.channel_matches[0].channel.id, RecipientRole::To),
            (ingest.channel_matches[1].channel.id, RecipientRole::Cc),
        ]
    );
}

#[tokio::test]
async fn test_pipeline_address_chaining_execution() {
    let company_id = Uuid::new_v4();
    let wf1_id = Uuid::new_v4();
    let wf2_id = Uuid::new_v4();
    let wf3_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf1_id,
                company_id,
                name: "Support".to_string(),
                description: None,
                slug: "support".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf2_id,
                company_id,
                name: "Billing".to_string(),
                description: None,
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf3_id,
                company_id,
                name: "Legal".to_string(),
                description: None,
                slug: "legal".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "support+billing+legal@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Pipeline Request".to_string()),
        text: Some("Please process via support, billing, and legal.".to_string()),
        ..Default::default()
    };

    let ingest = thread_use_cases
        .ingest_test_email(raw_payload)
        .await
        .unwrap();

    assert!(ingest.accepted);
    assert_eq!(ingest.channel_matches.len(), 3);
    assert_eq!(ingest.channel_matches[0].channel.slug, "support");
    assert_eq!(ingest.channel_matches[0].step.index, 0);
    assert_eq!(ingest.channel_matches[0].step.total, 3);
    assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
    assert_eq!(ingest.channel_matches[1].step.index, 1);
    assert_eq!(ingest.channel_matches[2].channel.slug, "legal");
    assert_eq!(ingest.channel_matches[2].step.index, 2);

    // Verify threads were created for all 3 step channels
    let threads = thread_persistence.threads();
    assert_eq!(threads.len(), 3);
}

#[tokio::test]
async fn test_misspelled_channel_bounce_and_strict_pipeline_validation() {
    let company_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Support".to_string(),
                description: None,
                slug: "support".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Billing".to_string(),
                description: None,
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "".to_string(), // skip external SMTP in test
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Single misspelled address
    let raw_payload_single = RawInboundPayload {
        to: "suppport@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Help".to_string()),
        text: Some("Help please".to_string()),
        ..Default::default()
    };

    let ingest_single = thread_use_cases
        .ingest_test_email(raw_payload_single)
        .await
        .unwrap();

    assert!(!ingest_single.accepted);
    assert_eq!(
        ingest_single.reason(),
        Some("Channel address not found or misspelled")
    );
    let bounce_single = ingest_single.bounce_info().unwrap();
    assert_eq!(bounce_single.invalid_slugs, vec!["suppport"]);
    assert_eq!(bounce_single.suggestions[0].suggestions, vec!["support"]);

    // Verify bounce email body formatting
    let bounce_body = format_bounce_email_body(bounce_single, "mailagents.com");
    assert!(bounce_body.contains("suppport@acme.mailagents.com"));
    assert!(bounce_body.contains("support@acme.mailagents.com"));

    thread_use_cases
        .handle_bounce_dispatch(&ingest_single)
        .await
        .unwrap();
    thread_use_cases
        .handle_bounce_dispatch(&ingest_single)
        .await
        .unwrap();
    let queued = thread_persistence.queued_standalone_deliveries();
    assert_eq!(
        queued.len(),
        1,
        "a webhook retry must not queue a second bounce"
    );
    assert_eq!(
        queued[0].external_destination.as_str(),
        "customer@client.com"
    );

    // 2. Strict pipeline validation with misspelled step 'biling'
    let raw_payload_pipeline = RawInboundPayload {
        to: "support+biling@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Pipeline Help".to_string()),
        text: Some("Help please".to_string()),
        ..Default::default()
    };

    let ingest_pipeline = thread_use_cases
        .ingest_test_email(raw_payload_pipeline)
        .await
        .unwrap();

    assert!(!ingest_pipeline.accepted);
    let bounce_pipeline = ingest_pipeline.bounce_info().unwrap();
    assert_eq!(bounce_pipeline.invalid_slugs, vec!["biling"]);
    assert_eq!(bounce_pipeline.suggestions[0].suggestions, vec!["billing"]);
}

#[tokio::test]
async fn test_quote_stripping_rules_for_first_in_thread_and_forwarded_emails() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Initial email in a NEW thread containing blockquotes/header markers.
    // Quotes MUST NOT be stripped because it is the first email in a new thread.
    let msg1_text = "Please check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
    let raw_first_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("New Ticket with Quotes".to_string()),
        text: Some(msg1_text.to_string()),
        headers: Some("Message-ID: <msg1@client.com>\n".to_string()),
        ..Default::default()
    };

    let res1 = thread_use_cases
        .ingest_test_email(raw_first_email)
        .await
        .unwrap();

    assert!(res1.accepted);
    let msg1 = res1.inbound_message.unwrap();
    assert_eq!(msg1.clean_text_body, msg1_text); // Preserved full text!

    // 2. Reply in the EXISTING thread containing repeated quotes from msg1.
    // Quotes MUST be stripped because this is a subsequent message in an existing thread.
    let msg2_text = "Thanks for looking into this.\n\nPlease check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
    let raw_reply_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Re: New Ticket with Quotes".to_string()),
        text: Some(msg2_text.to_string()),
        headers: Some(
            "Message-ID: <msg2@client.com>\nIn-Reply-To: <msg1@client.com>\n".to_string(),
        ),
        ..Default::default()
    };

    let res2 = thread_use_cases
        .ingest_test_email(raw_reply_email)
        .await
        .unwrap();

    assert!(res2.accepted);
    let msg2 = res2.inbound_message.unwrap();
    assert_eq!(msg2.clean_text_body, "Thanks for looking into this."); // Stripped quotes!

    // 3. Forwarded email into existing or new thread.
    // Quotes MUST NOT be stripped because it is a forwarded email.
    let fwd_text = "FYI forwarding this:\n\n---------- Forwarded message ---------\nFrom: Alice <alice@other.com>\nDate: Mon, Aug 10, 2026\nSubject: Forwarded Issue\n\nOriginal forwarded body";
    let raw_fwd_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "manager@client.com".to_string(),
        subject: Some("Fwd: External Report".to_string()),
        text: Some(fwd_text.to_string()),
        headers: Some("Message-ID: <msg3@client.com>\n".to_string()),
        ..Default::default()
    };

    let res3 = thread_use_cases
        .ingest_test_email(raw_fwd_email)
        .await
        .unwrap();

    assert!(res3.accepted);
    let msg3 = res3.inbound_message.unwrap();
    assert_eq!(msg3.clean_text_body, fwd_text); // Preserved full forwarded text!
}

#[tokio::test]
async fn test_participant_modes_company_team_public_and_explicit() {
    let company_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team_member@acme.com".to_string())],
    ));

    let flow_team_only = Uuid::new_v4();
    let flow_public = Uuid::new_v4();
    let flow_explicit = Uuid::new_v4();

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_team_only,
                company_id,
                name: "Team Only".to_string(),
                description: None,
                slug: "team-only".into(),
                alias_slugs: Vec::new(),
                participant_emails: None, // Default = Company Team
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_public,
                company_id,
                name: "Public Flow".to_string(),
                description: None,
                slug: "public-flow".into(),
                alias_slugs: Vec::new(),
                participant_emails: Some(vec!["@public".into()]), // Public
                access_mode: ChannelAccessMode::Public,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_explicit,
                company_id,
                name: "Explicit Flow".to_string(),
                description: None,
                slug: "explicit-flow".into(),
                alias_slugs: Vec::new(),
                participant_emails: Some(vec!["allowed@external.com".into()]), // Explicit list
                access_mode: ChannelAccessMode::Allowlist,
                principal_grants: email_allowlist_grants(company_id, &["allowed@external.com"]),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Team-only flow: Team member accepted
    let res1 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "team-only@acme.mailagents.com".to_string(),
            from: "team_member@acme.com".to_string(),
            subject: Some("Team msg".to_string()),
            text: Some("Hello team".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res1.accepted);

    // 2. Team-only flow: External sender rejected
    let res2 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "team-only@acme.mailagents.com".to_string(),
            from: "external@other.com".to_string(),
            subject: Some("External msg".to_string()),
            text: Some("Hello team".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res2.accepted);
    assert_eq!(res2.reason(), Some("Sender unauthorized for channel"));

    // 3. Public flow: External sender accepted
    let res3 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "public-flow@acme.mailagents.com".to_string(),
            from: "external@other.com".to_string(),
            subject: Some("Public msg".to_string()),
            text: Some("Hello public".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res3.accepted);

    // 4. Explicit list flow: Allowed sender accepted, non-allowed rejected
    let res4 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "explicit-flow@acme.mailagents.com".to_string(),
            from: "allowed@external.com".to_string(),
            subject: Some("Explicit allowed".to_string()),
            text: Some("Hello allowed".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res4.accepted);

    let res5 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "explicit-flow@acme.mailagents.com".to_string(),
            from: "notallowed@external.com".to_string(),
            subject: Some("Explicit blocked".to_string()),
            text: Some("Hello blocked".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res5.accepted);
    assert_eq!(res5.reason(), Some("Sender unauthorized for channel"));
}

#[tokio::test]
async fn test_sender_verification_refuses_strangers_replying_into_a_thread() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: Some(vec!["@public".into()]),
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Initial email from client creates thread
    let res1 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <msg-client-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "client@external.com".to_string(),
            subject: Some("Need quote".to_string()),
            text: Some("Can I get a quote?".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res1.accepted);
    assert!(res1.thread.is_some());

    // 3. Unauthorized third-party attacker tries to inject message into thread using In-Reply-To
    let res_attacker = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-attacker-1@evil.com>\nIn-Reply-To: <msg-client-1@external.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com".to_string(),
            from: "attacker@evil.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Fake reply".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res_attacker.accepted);
    assert!(res_attacker.bounce_info().is_some());
    assert_eq!(
        res_attacker.reason(),
        Some("Sender is not an authorized participant or delegation target for this thread")
    );

    // 4. Answering an outbound message the thread sent does not make a stranger a party to it:
    //    there is no outreach authorization any more, and a workflow that wants an answer from
    //    a member addresses them so they become a participant (p08 D1.5.4).
    let res_vendor = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-vendor-1@supplier.com>\nIn-Reply-To: <outreach-vendor@mailagents.com>\nReferences: <msg-client-1@external.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com".to_string(),
            from: "vendor@supplier.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Quote is $500".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res_vendor.accepted);
}

#[tokio::test]
async fn uncorrelated_inter_channel_cycle_is_rejected() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                description: None,
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                description: None,
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });
    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));
    let use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    let disposition = relay_hop(
        &use_cases,
        &InternalHop {
            hop_count: 1,
            trace_channels: vec![channel_a_id],
            in_reply_to: Some("<unsolicited@example.com>".into()),
            references: vec!["<unsolicited@example.com>".into()],
            ..InternalHop::new(
                channel_b_id,
                "agent-b@acme.mailagents.com",
                "agent-a@acme.mailagents.com",
                "Unsolicited message",
                "Spontaneous message",
            )
        },
    )
    .await;

    assert_eq!(
        refusal(&disposition),
        Some("Inter-channel loop cycle detected")
    );
}

/// p08 DV-B4c2-1: a channel needs no agent. What answers it is a workflow binding at admission,
/// so the relay accepts a well-formed hop to a plain channel rather than refusing it for having
/// no configured agent.
#[tokio::test]
async fn a_hop_to_a_plain_channel_is_relayed() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                description: None,
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                description: None,
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });
    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));
    let use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    let disposition = relay_hop(
        &use_cases,
        &InternalHop::new(
            channel_a_id,
            "agent-a@acme.mailagents.com",
            "agent-b@acme.mailagents.com",
            "A question for B",
            "Can you take this one?",
        ),
    )
    .await;

    assert!(
        matches!(disposition, crate::transport::RelayDisposition::Relayed),
        "{disposition:?}"
    );
}

#[tokio::test]
async fn inter_channel_max_hops_exceeded_is_rejected() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                description: None,
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                description: None,
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            },
        ]),
    });
    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));
    let use_cases = ThreadUseCases::for_test(
        thread_persistence,
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    let disposition = relay_hop(
        &use_cases,
        &InternalHop {
            hop_count: MAX_INGRESS_HOPS,
            in_reply_to: Some("<msg@example.com>".into()),
            ..InternalHop::new(
                channel_a_id,
                "agent-a@acme.mailagents.com",
                "agent-b@acme.mailagents.com",
                "Deep loop",
                "Exceeding max hops",
            )
        },
    )
    .await;

    assert_eq!(
        refusal(&disposition),
        Some("Max inter-channel hop count reached")
    );
}

#[tokio::test]
async fn test_third_party_thread_participants_addition_and_authorization() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None, // Default = company team members
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Team member (workflow participant) sends email to channel with third-party in CC
    let res1 = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <msg-team-1@acme.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("client@external.com".to_string()),
            from: "team@acme.com".to_string(),
            subject: Some("Client Inquiry".to_string()),
            text: Some("Adding third-party client to thread".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res1.accepted);
    let thread = res1.thread.unwrap();
    assert!(
        thread
            .participant_projection
            .subjects_for(TransportKind::Email)
            .contains(&"client@external.com")
    );

    // 2. Third-party client replies to thread -> ACCEPTED because they were added to thread participants
    let res_reply = thread_use_cases
            .ingest_test_email(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-1@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Thanks for adding me".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_reply.accepted);

    // 3. Third-party client tries to send a NEW email to channel without thread reference -> REJECTED
    let res_unauth = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <msg-client-new-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "client@external.com".to_string(),
            subject: Some("Unrelated Subject".to_string()),
            text: Some("Starting a new conversation".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(!res_unauth.accepted);
    assert_eq!(res_unauth.reason(), Some("Sender unauthorized for channel"));

    // 4. Team member replies to existing thread, adding another third party (vendor@supplier.com) in To
    let res_expand = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-team-2@acme.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com, vendor@supplier.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Re: Client Inquiry".to_string()),
            text: Some("Looping in supplier".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_expand.accepted);
    let updated_thread = res_expand.thread.unwrap();
    assert!(
        updated_thread
            .participant_projection
            .subjects_for(TransportKind::Email)
            .contains(&"vendor@supplier.com")
    );

    // 5. vendor@supplier.com replies to thread -> ACCEPTED
    let res_vendor_reply = thread_use_cases
            .ingest_test_email(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-vendor-reply-1@supplier.com>\nIn-Reply-To: <msg-team-2@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "vendor@supplier.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Vendor here".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_vendor_reply.accepted);

    // 6. External non-team-member (client@external.com) replies and tries to CC unauthorized@other.com -> unauthorized@other.com is NOT added
    let res_external_cc = thread_use_cases
            .ingest_test_email(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-2@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                cc: Some("unauthorized@other.com".to_string()),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Adding random CC".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_external_cc.accepted);
    let current_thread = res_external_cc.thread.unwrap();
    assert!(
        !current_thread
            .participant_projection
            .subjects_for(TransportKind::Email)
            .contains(&"unauthorized@other.com")
    );
}

#[tokio::test]
async fn test_context_only_quiet_mode_ingestion() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        sendgrid_inbound: None,
        resend_api: ResendApiConfig::default(),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: true,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".to_string(),
        spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
        secure_cookies: false,
        gcs: None,
        operator_emails: Vec::new(),
    });

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        config,
    );

    // 1. Ingest email with .quiet suffix in address -> accepted, recorded but not answering
    let res_quiet_addr = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <msg-quiet-1@acme.com>\n".to_string()),
            to: "support.quiet@acme.mailagents.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Quiet Note".to_string()),
            text: Some("Background context note".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_quiet_addr.accepted);
    assert_eq!(res_quiet_addr.intent_ids.len(), 1);
    assert!(!res_quiet_addr.answers());

    // 2. Ingest email with [[quiet]] body tag -> accepted, not answering, tag stripped from text
    let res_quiet_body = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-quiet-2@acme.com>\nIn-Reply-To: <msg-quiet-1@acme.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Re: Quiet Note".to_string()),
            text: Some("[[quiet]] Additional background note".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_quiet_body.accepted);
    assert_eq!(res_quiet_body.intent_ids.len(), 1);
    assert!(!res_quiet_body.answers());
    assert_eq!(
        res_quiet_body
            .envelope
            .as_deref()
            .unwrap()
            .content
            .body_text(),
        "Additional background note"
    );
}

/// The `support` channel one of these fixtures builds.
///
/// Named fields rather than positional arguments: `enabled` and `add_3rd_party` are same-typed
/// switches that a positional call could silently transpose.
struct TestChannel {
    enabled: bool,
    add_3rd_party: bool,
    alias_slugs: Vec<ChannelSlug>,
    participant_emails: Option<Vec<EmailAddress>>,
}

impl Default for TestChannel {
    /// A plain team-only channel, taking traffic and pulling CC'd outsiders onto its threads —
    /// what every channel is until someone changes it.
    fn default() -> Self {
        Self {
            enabled: true,
            add_3rd_party: true,
            alias_slugs: Vec::new(),
            participant_emails: None,
        }
    }
}

/// One company with a `support` channel whose on/off state the caller picks, wired for ingest.
fn use_cases_with_channel_enabled(enabled: bool) -> (ThreadUseCases, Uuid) {
    use_cases_with_channel(TestChannel {
        enabled,
        ..TestChannel::default()
    })
}

fn use_cases_with_channel_aliases(alias_slugs: Vec<ChannelSlug>) -> (ThreadUseCases, Uuid) {
    use_cases_with_channel(TestChannel {
        alias_slugs,
        ..TestChannel::default()
    })
}

fn use_cases_with_channel(spec: TestChannel) -> (ThreadUseCases, Uuid) {
    let fixture = channel_fixture(spec);
    (fixture.use_cases, fixture.channel_id)
}

struct ChannelFixture {
    use_cases: ThreadUseCases,
    company_id: Uuid,
    channel_id: Uuid,
    threads: Arc<InMemoryThreads>,
    participants: Arc<InMemoryParticipantDirectory>,
}

impl ChannelFixture {
    fn durable_counts(&self) -> [usize; 3] {
        [
            self.threads.threads().len(),
            self.threads.messages().len(),
            self.participants.identity_count(),
        ]
    }
}

fn channel_fixture(spec: TestChannel) -> ChannelFixture {
    channel_fixture_with_config(spec, internal_test_config())
}

fn channel_fixture_with_config(spec: TestChannel, config: Arc<AppConfig>) -> ChannelFixture {
    let TestChannel {
        enabled,
        add_3rd_party,
        alias_slugs,
        participant_emails,
    } = spec;
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let (access_mode, principal_grants) =
        email_allowlist_policy(company_id, participant_emails.as_deref());

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled,
            add_3rd_party,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs,
            participant_emails,
            access_mode,
            principal_grants,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let threads = Arc::new(InMemoryThreads::for_company(company_id));
    let participants =
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence.clone()));
    let use_cases = ThreadUseCases::for_test(
        threads.clone(),
        channel_persistence.clone(),
        company_persistence,
        participants.clone(),
        config,
    );

    ChannelFixture {
        use_cases,
        company_id,
        channel_id,
        threads,
        participants,
    }
}

fn rejection_message(message_id: &str) -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(format!("Message-ID: <{message_id}@example.com>\n")),
        to: "support@acme.mailagents.com".into(),
        from: "team@acme.com".into(),
        subject: Some("Question".into()),
        text: Some("Body".into()),
        ..Default::default()
    }
}

async fn assert_rejected_without_writes(
    fixture: &ChannelFixture,
    payload: RawInboundPayload,
    origin: IngressOrigin,
    expected_reason: &str,
) {
    let before = fixture.durable_counts();
    let result = fixture
        .use_cases
        .ingest_test_email_as(payload, origin)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reason(), Some(expected_reason));
    assert_eq!(fixture.durable_counts(), before);
}

/// Every pre-commit refusal is read-only. Attachment count and canonical validation fail even
/// earlier, at the email boundary, and likewise leave every in-memory persistence surface alone.
#[tokio::test]
async fn ingress_rejections_create_no_identity_thread_message_task_delivery_or_attachment_state() {
    let authentication = channel_fixture(TestChannel::default());
    let mut payload = rejection_message("reject-auth");
    payload.dmarc = crate::entities::auth::AuthVerdict::Fail;
    assert_rejected_without_writes(
        &authentication,
        payload,
        IngressOrigin::ExternalTransport,
        "DMARC authentication did not pass",
    )
    .await;

    let acl = channel_fixture(TestChannel::default());
    let mut payload = rejection_message("reject-acl");
    payload.from = "unauthorized@example.net".into();
    assert_rejected_without_writes(
        &acl,
        payload,
        IngressOrigin::ExternalTransport,
        "Sender unauthorized for channel",
    )
    .await;

    let unknown = channel_fixture(TestChannel::default());
    let mut payload = rejection_message("reject-unknown");
    payload.to = "support@missing.mailagents.com".into();
    assert_rejected_without_writes(
        &unknown,
        payload,
        IngressOrigin::ExternalTransport,
        "Company or Channel not found",
    )
    .await;

    let spam = channel_fixture(TestChannel {
        participant_emails: Some(vec![EmailAddress::from("@public")]),
        ..TestChannel::default()
    });
    let mut payload = rejection_message("reject-spam");
    payload.from = "spammer@example.net".into();
    payload.spam_score = Some(5.0);
    assert_rejected_without_writes(
        &spam,
        payload,
        IngressOrigin::ExternalTransport,
        "Spam score threshold exceeded",
    )
    .await;

    let hop = channel_fixture(TestChannel::default());
    let mut payload = rejection_message("reject-hop");
    payload.headers = Some(format!(
        "Message-ID: <reject-hop@example.com>\nX-MailAgents-Channel-ID: {}\nX-MailAgents-Hop-Count: {MAX_INGRESS_HOPS}\n",
        hop.channel_id
    ));
    assert_rejected_without_writes(
        &hop,
        payload,
        IngressOrigin::InternalChannel {
            company_id: hop.company_id,
            channel_id: hop.channel_id,
        },
        "Max inter-channel hop count reached",
    )
    .await;

    let cycle = channel_fixture(TestChannel::default());
    let mut payload = rejection_message("reject-cycle");
    payload.headers = Some(format!(
        "Message-ID: <reject-cycle@example.com>\nX-MailAgents-Channel-ID: {}\nX-MailAgents-Hop-Count: 1\nX-MailAgents-Trace: {}\n",
        cycle.channel_id, cycle.channel_id
    ));
    assert_rejected_without_writes(
        &cycle,
        payload,
        IngressOrigin::InternalChannel {
            company_id: cycle.company_id,
            channel_id: cycle.channel_id,
        },
        "Inter-channel loop cycle detected",
    )
    .await;

    let adapter_failures = channel_fixture(TestChannel::default());
    let before = adapter_failures.durable_counts();
    let mut too_many = rejection_message("reject-attachments");
    too_many.attachments_data = (0..=MAX_ATTACHMENTS)
        .map(
            |index| crate::adapters::protocols::email::parser::RawAttachmentData {
                filename: format!("{index}.txt"),
                content_type: "text/plain".into(),
                content: Vec::new(),
                stored_key: None,
            },
        )
        .collect();
    assert!(
        adapter_failures
            .use_cases
            .ingest_test_email(too_many)
            .await
            .is_err()
    );
    assert_eq!(adapter_failures.durable_counts(), before);

    let mut invalid = rejection_message("reject-validation");
    invalid.text = Some("x".repeat(MAX_BODY_BYTES + 1));
    assert!(
        adapter_failures
            .use_cases
            .ingest_test_email(invalid)
            .await
            .is_err()
    );
    assert_eq!(adapter_failures.durable_counts(), before);

    let mut storage_config = internal_test_config();
    Arc::get_mut(&mut storage_config).unwrap().gcs = Some(crate::infra::config::GcsConfig {
        bucket: "public".into(),
        service_account_json_base64: "unused-by-fake".into(),
        public_base_url_override: None,
        avatar_folder: "avatars".into(),
        attachments_bucket: Some("private".into()),
        attachments_folder: "attachments".into(),
    });
    let storage_fixture = channel_fixture_with_config(TestChannel::default(), storage_config);
    let storage = crate::adapters::storage::test_support::FakeStorage::new();
    let mut rejected_attachment = rejection_message("reject-before-upload");
    rejected_attachment.from = "unauthorized@example.net".into();
    rejected_attachment.attachments_data = vec![
        crate::adapters::protocols::email::parser::RawAttachmentData {
            filename: "evidence.txt".into(),
            content_type: "text/plain".into(),
            content: b"must not be uploaded".to_vec(),
            stored_key: None,
        },
    ];
    let accepted = crate::adapters::protocols::email::EmailIngressAdapter::for_config(
        storage_fixture.use_cases.config(),
    )
    .accept(
        rejected_attachment,
        crate::adapters::protocols::email::EmailIngressTrust::Verified(
            crate::adapters::protocols::email::VerifiedEmailAuth {
                spf: crate::entities::auth::AuthVerdict::Pass,
                dkim: crate::entities::auth::AuthVerdict::Pass,
                dmarc: crate::entities::auth::AuthVerdict::Pass,
                spam_score: None,
            },
        ),
    )
    .unwrap();
    let (inbound, _pending_bytes) =
        accepted.into_preflight_parts(IngressOrigin::ExternalTransport, ReplyDelivery::Send);
    assert!(storage.objects().is_empty());
    assert!(matches!(
        storage_fixture
            .use_cases
            .preflight_inbound(inbound)
            .await
            .unwrap(),
        InboundPreflight::Rejected(_)
    ));
    assert!(storage.objects().is_empty());
}

#[tokio::test]
async fn authenticated_application_message_does_not_require_dmarc() {
    let (use_cases, _) = use_cases_with_channel(TestChannel::default());
    let result = use_cases
        .ingest_test_email_as(
            RawInboundPayload {
                to: "support@acme.mailagents.com".to_string(),
                from: "team@acme.com".to_string(),
                subject: Some("Mailbox message".to_string()),
                text: Some("Sent by a signed-in user".to_string()),
                spf: crate::entities::auth::AuthVerdict::Unknown,
                dkim: crate::entities::auth::AuthVerdict::Unknown,
                dmarc: crate::entities::auth::AuthVerdict::Unknown,
                ..Default::default()
            },
            IngressOrigin::TrustedApplication,
        )
        .await
        .unwrap();

    assert!(result.accepted, "{:?}", result.reason());
}

#[tokio::test]
async fn authenticated_application_origin_does_not_bypass_participant_authorization() {
    let (use_cases, _) = use_cases_with_channel(TestChannel::default());
    let result = use_cases
        .ingest_test_email_as(
            RawInboundPayload {
                // Treat every payload field as hostile even though the HTTP route normally derives
                // this value from the authenticated account.
                to: "support@acme.mailagents.com".to_string(),
                from: "fabricated-attacker@example.net".to_string(),
                subject: Some("Forged mailbox message".to_string()),
                text: Some("Try to cross the channel boundary".to_string()),
                dmarc: crate::entities::auth::AuthVerdict::Unknown,
                ..Default::default()
            },
            IngressOrigin::TrustedApplication,
        )
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("Sender unauthorized for channel"));
}

/// A message from a team member that copies someone outside the platform.
fn message_to_support_cc_outsider() -> RawInboundPayload {
    RawInboundPayload {
        headers: Some("Message-ID: <cc-outsider@acme.com>\n".to_string()),
        to: "support@acme.mailagents.com".to_string(),
        cc: Some("client@external.com".to_string()),
        from: "team@acme.com".to_string(),
        subject: Some("Client inquiry".to_string()),
        text: Some("Looping the client in".to_string()),
        ..Default::default()
    }
}

fn participants_of(result: &InboundIngestResult) -> Vec<String> {
    result
        .thread
        .as_ref()
        .unwrap()
        .participant_projection
        .subjects_for(TransportKind::Email)
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[path = "address_grouping_tests.rs"]
mod address_grouping;

fn cc_message(message_id: &str, body: &str, cc: &str) -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(format!("Message-ID: <{message_id}@acme.com>\n")),
        to: "person@example.com".to_string(),
        cc: Some(cc.to_string()),
        from: "team@acme.com".to_string(),
        subject: Some("FYI".to_string()),
        text: Some(body.to_string()),
        ..Default::default()
    }
}

/// The intent the last commit recorded for the fixture's channel.
fn committed_intent(fixture: &ChannelFixture) -> crate::transport::InboundAdmissionIntent {
    let request = last_commit(fixture);
    *request
        .intents
        .iter()
        .find(|intent| intent.channel_id == fixture.channel_id)
        .expect("the channel was associated, so it has an intent")
}

#[tokio::test]
async fn a_cc_d_channels_address_in_the_body_is_a_mention() {
    let fixture = channel_fixture(TestChannel::default());

    let result = fixture
        .use_cases
        .ingest_test_email(cc_message(
            "cc-email-mention",
            "Please ask SUPPORT@ACME.MAILAGENTS.COM to help.",
            "support@acme.mailagents.com",
        ))
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.intent_ids.len(), 1);
    let intent = committed_intent(&fixture);
    assert_eq!(intent.role, RecipientRole::Cc);
    assert!(intent.mentioned);
    assert!(last_commit(&fixture).admissible);
}

#[tokio::test]
async fn a_cc_d_channels_name_or_alias_in_the_body_is_a_mention() {
    for (message_id, body) in [
        ("cc-channel-mention", "Please ask @support."),
        ("cc-alias-mention", "Please ask @helpdesk."),
    ] {
        let fixture = channel_fixture(TestChannel {
            alias_slugs: vec!["helpdesk".into()],
            ..TestChannel::default()
        });
        let result = fixture
            .use_cases
            .ingest_test_email(cc_message(message_id, body, "helpdesk@acme.mailagents.com"))
            .await
            .unwrap();

        assert!(result.accepted);
        assert!(
            committed_intent(&fixture).mentioned,
            "body did not mention the channel: {body}"
        );
    }
}

/// A plain copy is recorded too, unmentioned: whether a Cc starts anything is its bindings' call.
#[tokio::test]
async fn a_cc_d_channel_the_body_never_names_is_not_mentioned() {
    let fixture = channel_fixture(TestChannel::default());

    let result = fixture
        .use_cases
        .ingest_test_email(cc_message(
            "cc-no-mention",
            "Just keeping you posted.",
            "support@acme.mailagents.com",
        ))
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.intent_ids.len(), 1);
    let intent = committed_intent(&fixture);
    assert_eq!(intent.role, RecipientRole::Cc);
    assert!(!intent.mentioned);
}

#[tokio::test]
async fn a_mention_only_in_quoted_history_is_not_a_mention() {
    let fixture = channel_fixture(TestChannel::default());
    let first = fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <quoted-mention-root@acme.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Mention history".to_string()),
            text: Some("Please ask @support.".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(committed_intent(&fixture).mentioned);
    assert_eq!(first.intent_ids.len(), 1);

    let result = fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <quoted-mention-reply@acme.com>\nIn-Reply-To: <quoted-mention-root@acme.com>\n"
                    .to_string(),
            ),
            to: "person@example.com".to_string(),
            cc: Some("support@acme.mailagents.com".to_string()),
            from: "team@acme.com".to_string(),
            subject: Some("Re: Mention history".to_string()),
            text: Some(
                "Just keeping this for context.\n\nOn Fri, someone wrote:\n> Please ask @support."
                    .to_string(),
            ),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(!committed_intent(&fixture).mentioned);
}

#[tokio::test]
async fn an_explicit_quiet_trigger_makes_a_mentioning_message_inadmissible() {
    let fixture = channel_fixture(TestChannel::default());

    let result = fixture
        .use_cases
        .ingest_test_email(cc_message(
            "quiet-cc-mention",
            "[quiet] Please ask @support.",
            "support@acme.mailagents.com",
        ))
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(!result.answers());
    assert!(!last_commit(&fixture).admissible);
    assert_eq!(result.intent_ids.len(), 1);
}

/// The flag off makes the channel internal: the CC'd outsider is not recorded on the thread, and
/// so does not inherit the thread's standing invitation to post into it.
#[tokio::test]
async fn a_closed_channel_keeps_cc_d_outsiders_off_the_thread() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        ..TestChannel::default()
    });

    let result = thread_use_cases
        .ingest_test_email(message_to_support_cc_outsider())
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(participants_of(&result), vec!["team@acme.com".to_string()]);
}

/// The same message on the default channel, so the test above is pinned to the flag rather than to
/// something else about the fixture.
#[tokio::test]
async fn an_open_channel_still_adds_cc_d_outsiders_to_the_thread() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel::default());

    let result = thread_use_cases
        .ingest_test_email(message_to_support_cc_outsider())
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(
        participants_of(&result)
            .iter()
            .any(|p| p.eq_ignore_ascii_case("client@external.com"))
    );
}

/// The flag narrows, it never widens. A `@public` channel admits any sender, but an admitted
/// stranger is still not *trusted*, so turning the flag on does not let them attach whoever they
/// like to the thread — and every such address would otherwise gain the right to post into it.
#[tokio::test]
async fn an_untrusted_sender_cannot_add_outsiders_even_with_the_flag_on() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        participant_emails: Some(vec!["@public".into()]),
        ..TestChannel::default()
    });

    let result = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <stranger@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("accomplice@external.com".to_string()),
            from: "stranger@external.com".to_string(),
            subject: Some("Hello".to_string()),
            text: Some("Bringing a friend".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(
        participants_of(&result),
        vec!["stranger@external.com".to_string()]
    );
}

/// The outsider's own reply-all. Never having joined the thread, they have no way into it — the
/// thread membership that `add_3rd_party` withholds is exactly what would have let them in.
///
/// On a team-only channel they are turned away by the channel ACL, before the thread is consulted
/// at all, which is a silent drop rather than a bounce. That is the pre-existing shape of an
/// unauthorized sender, and this test pins it so the difference from the `@public` case below is
/// deliberate rather than accidental.
#[tokio::test]
async fn a_closed_channel_refuses_the_outsider_s_own_reply() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        ..TestChannel::default()
    });

    thread_use_cases
        .ingest_test_email(message_to_support_cc_outsider())
        .await
        .unwrap();

    let reply = thread_use_cases
        .ingest_test_email(outsider_reply())
        .await
        .unwrap();

    assert!(!reply.accepted);
    assert_eq!(reply.reason(), Some("Sender unauthorized for channel"));
}

/// Where the channel ACL does admit the sender, the thread is consulted, and refusing them there
/// is a bounce — they get told, rather than having their mail disappear.
#[tokio::test]
async fn a_closed_public_channel_bounces_the_outsider_s_own_reply() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        participant_emails: Some(vec!["@public".into(), "team@acme.com".into()]),
        ..TestChannel::default()
    });

    thread_use_cases
        .ingest_test_email(message_to_support_cc_outsider())
        .await
        .unwrap();

    let reply = thread_use_cases
        .ingest_test_email(outsider_reply())
        .await
        .unwrap();

    assert!(!reply.accepted);
    assert!(reply.bounce_info().is_some(), "the sender must be told why");
}

/// The CC'd outsider answering the message that copied them.
fn outsider_reply() -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(
            "Message-ID: <outsider-reply@external.com>\nIn-Reply-To: <cc-outsider@acme.com>\n"
                .to_string(),
        ),
        to: "support@acme.mailagents.com".to_string(),
        from: "client@external.com".to_string(),
        subject: Some("Re: Client inquiry".to_string()),
        text: Some("Replying to all".to_string()),
        ..Default::default()
    }
}

fn message_to_support() -> RawInboundPayload {
    RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "team@acme.com".to_string(),
        subject: Some("Hello".to_string()),
        text: Some("Anyone there?".to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn disabled_channel_bounces_inbound_mail() {
    let (thread_use_cases, _) = use_cases_with_channel_enabled(false);

    let result = thread_use_cases
        .ingest_test_email(message_to_support())
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(result.reason(), Some("Channel is disabled"));

    let bounce = result
        .bounce_info()
        .expect("a disabled channel bounces rather than dropping the message");
    assert_eq!(bounce.disabled_slugs, vec![ChannelSlug::from("support")]);
    assert!(
        bounce.invalid_slugs.is_empty(),
        "a disabled channel is not a misspelling"
    );
    assert_eq!(bounce.recipient_to, EmailAddress::from("team@acme.com"));

    let body = format_bounce_email_body(bounce, "mailagents.com");
    assert!(body.contains("support@acme.mailagents.com"));
    assert!(body.contains("switched off"));
    assert!(!body.contains("Did you mean"));
}

#[tokio::test]
async fn enabled_channel_still_accepts_the_same_message() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_enabled(true);

    let result = thread_use_cases
        .ingest_test_email(message_to_support())
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason());
    assert_eq!(result.thread.unwrap().channel_id, channel_id);
}

#[tokio::test]
async fn alias_address_reaches_the_channel_and_is_replied_from() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "sales@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason());
    assert_eq!(result.channel_matches.len(), 1);

    let matched = &result.channel_matches[0];
    assert_eq!(matched.channel.id, channel_id);
    assert_eq!(matched.channel.slug, "support");
    // The reply must go back out on the address the sender wrote to, not the canonical slug.
    assert_eq!(matched.reply_slug(), ChannelSlug::from("sales"));
}

#[tokio::test]
async fn alias_and_canonical_slug_in_one_pipeline_ingest_once() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "sales+support@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason());
    assert_eq!(
        result.channel_matches.len(),
        1,
        "two names for one channel are still one delivery"
    );
    assert_eq!(result.channel_matches[0].channel.id, channel_id);
    // The first address on the envelope wins, so the reply stays on the alias.
    assert_eq!(
        result.channel_matches[0].reply_slug(),
        ChannelSlug::from("sales")
    );
}

#[tokio::test]
async fn unknown_slug_is_still_a_bounce_when_the_channel_has_aliases() {
    let (thread_use_cases, _) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "salez@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(!result.accepted);
    let bounce = result
        .bounce_info()
        .cloned()
        .expect("a misspelled alias bounces");
    assert_eq!(bounce.invalid_slugs, vec![ChannelSlug::from("salez")]);
    assert!(
        bounce
            .suggestions
            .iter()
            .any(|s| s.suggestions.contains(&ChannelSlug::from("sales"))),
        "aliases are offered as did-you-mean suggestions: {:?}",
        bounce.suggestions
    );
}

/// One channel in the [`use_cases_with_directory`] fixture.
///
/// Named fields rather than positional arguments: `slug`, `name` and `description` are all
/// strings, and a transposed pair would silently build a different channel.
struct DirectoryChannel {
    slug: &'static str,
    name: &'static str,
    description: Option<&'static str>,
    enabled: bool,
    participant_emails: Option<Vec<EmailAddress>>,
}

impl Default for DirectoryChannel {
    fn default() -> Self {
        Self {
            slug: "support",
            name: "Support",
            description: None,
            enabled: true,
            participant_emails: None,
        }
    }
}

/// A company whose channel list the bounce directory has to filter, with `team@acme.com` on the
/// team and nobody else.
fn use_cases_with_directory(specs: Vec<DirectoryChannel>) -> ThreadUseCases {
    let company_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channels = specs
        .into_iter()
        .map(|spec| {
            let (access_mode, principal_grants) =
                email_allowlist_policy(company_id, spec.participant_emails.as_deref());
            Channel {
                id: Uuid::new_v4(),
                company_id,
                name: spec.name.to_string(),
                description: spec.description.map(str::to_string),
                slug: spec.slug.into(),
                alias_slugs: Vec::new(),
                participant_emails: spec.participant_emails,
                access_mode,
                principal_grants,
                enabled: spec.enabled,
                add_3rd_party: true,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
                settings_revision: 0,
            }
        })
        .collect();

    ThreadUseCases::for_test(
        Arc::new(InMemoryThreads::for_company(company_id)),
        Arc::new(MockChannelPersistence {
            channels: Mutex::new(channels),
        }),
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    )
}

fn misspelled_message_from(sender: &str) -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(format!("Message-ID: <typo-{sender}>\n")),
        to: "suport@acme.mailagents.com".to_string(),
        from: sender.to_string(),
        subject: Some("Quick question".to_string()),
        text: Some("Can you take a look?".to_string()),
        ..Default::default()
    }
}

/// A `Thread-Index` the adapter cannot canonicalize is dropped, counted, and never guessed at.
///
/// The split is the point: the adapter *parses* and reports what it could not use, and the
/// application decides that a discarded header is worth a counter. Neither side reaches into the
/// other -- the parser holds no monitoring handle, and the ingest never sees a header.
#[tokio::test]
async fn a_malformed_thread_index_is_dropped_at_the_boundary_and_counted_by_ingest() {
    let monitor = Arc::new(InMemoryMonitor::new());
    let use_cases = use_cases_with_directory(Vec::new()).with_monitoring(monitor.clone());

    let accepted = crate::adapters::protocols::email::EmailIngressAdapter::new("mailagents.com")
        .accept(
            RawInboundPayload {
                headers: Some("Thread-Index: legacy-invalid".to_string()),
                to: "support@acme.mailagents.com".to_string(),
                from: "team@acme.com".to_string(),
                ..Default::default()
            },
            crate::adapters::protocols::email::EmailIngressTrust::Application,
        )
        .unwrap();
    assert!(
        accepted
            .draft
            .extension
            .email_metadata()
            .unwrap()
            .thread_index
            .is_none(),
        "an unusable header is dropped rather than stored"
    );

    use_cases
        .ingest(accepted.into_inbound(IngressOrigin::TrustedApplication, ReplyDelivery::Send))
        .await
        .unwrap();

    let stats = monitor.get_stats_json();
    assert_eq!(stats["custom_counters"]["thread_index_rejected_total"], 1);
    assert_eq!(
        stats["gauges"]["thread_index_rejected_encoded_bytes{reason=invalid_base64}"],
        15.0
    );
}

async fn bounce_for(use_cases: &ThreadUseCases, sender: &str) -> BounceInfo {
    let result = use_cases
        .ingest_test_email(misspelled_message_from(sender))
        .await
        .unwrap();

    assert!(!result.accepted, "reason: {:?}", result.reason());
    assert_eq!(
        result.reason(),
        Some("Channel address not found or misspelled")
    );
    result
        .bounce_info()
        .cloned()
        .expect("an unknown channel slug bounces rather than dropping the message")
}

fn listed_addresses(bounce: &BounceInfo) -> Vec<String> {
    bounce
        .available_channels
        .iter()
        .map(|entry| entry.address.to_string())
        .collect()
}

#[tokio::test]
async fn a_team_member_who_mistypes_a_channel_is_told_which_ones_they_can_use() {
    let use_cases = use_cases_with_directory(vec![
        DirectoryChannel {
            slug: "support",
            name: "Support Desk",
            description: Some("Answers customer support and refund questions."),
            ..DirectoryChannel::default()
        },
        DirectoryChannel {
            slug: "billing",
            name: "Billing",
            ..DirectoryChannel::default()
        },
    ]);

    let bounce = bounce_for(&use_cases, "team@acme.com").await;

    assert_eq!(
        listed_addresses(&bounce),
        vec![
            "billing@acme.mailagents.com".to_string(),
            "support@acme.mailagents.com".to_string(),
        ],
        "every channel the sender may write to is listed, in address order"
    );
    assert_eq!(
        bounce.available_channels[1].description.as_deref(),
        Some("Answers customer support and refund questions.")
    );
    assert_eq!(bounce.available_channels[0].description, None);

    let body = format_bounce_email_body(&bounce, "mailagents.com");
    assert!(body.contains("Channels you can write to:"));
    assert!(body.contains("support@acme.mailagents.com — Support Desk"));
    assert!(body.contains("Answers customer support and refund questions."));
    assert!(body.contains("billing@acme.mailagents.com — Billing"));
}

#[tokio::test]
async fn an_outsider_s_bounce_never_discloses_the_channel_directory() {
    let use_cases = use_cases_with_directory(vec![DirectoryChannel {
        slug: "support",
        name: "Support Desk",
        description: Some("Answers customer support and refund questions."),
        ..DirectoryChannel::default()
    }]);

    let bounce = bounce_for(&use_cases, "stranger@elsewhere.com").await;

    assert!(
        bounce.available_channels.is_empty(),
        "a guessed address must not enumerate the company's channels"
    );

    let body = format_bounce_email_body(&bounce, "mailagents.com");
    assert!(!body.contains("Channels you can write to:"));
    assert!(!body.contains("Support Desk"));
    assert!(
        body.contains("Did you mean:"),
        "the spelling hint an outsider already got is unchanged"
    );
}

#[tokio::test]
async fn the_directory_lists_only_what_this_sender_could_actually_reach() {
    let use_cases = use_cases_with_directory(vec![
        DirectoryChannel {
            slug: "support",
            name: "Support Desk",
            ..DirectoryChannel::default()
        },
        DirectoryChannel {
            slug: "legal",
            name: "Legal",
            participant_emails: Some(vec![EmailAddress::from("counsel@acme.com")]),
            ..DirectoryChannel::default()
        },
        DirectoryChannel {
            slug: "archive",
            name: "Archive",
            enabled: false,
            ..DirectoryChannel::default()
        },
    ]);

    let bounce = bounce_for(&use_cases, "team@acme.com").await;

    assert_eq!(
        listed_addresses(&bounce),
        vec!["support@acme.mailagents.com".to_string()],
        "a restricted channel the sender is not a participant of, and a disabled one, stay hidden"
    );
}

#[test]
fn the_bounce_body_omits_the_directory_section_when_there_is_nothing_to_list() {
    let bounce = BounceInfo {
        source_message_key: ExternalMessageKey::parse("<missing@example.com>").unwrap(),
        recipient_to: EmailAddress::from("team@acme.com"),
        company_slug: Some(CompanySlug::from("acme")),
        invalid_slugs: vec![ChannelSlug::from("suport")],
        disabled_slugs: Vec::new(),
        suggestions: vec![BounceSuggestion {
            invalid_slug: ChannelSlug::from("suport"),
            suggestions: vec![ChannelSlug::from("support")],
        }],
        available_channels: Vec::new(),
        original_subject: "Quick question".to_string(),
    };

    let body = format_bounce_email_body(&bounce, "mailagents.com");
    assert!(!body.contains("Channels you can write to:"));
    assert!(body.contains("support@acme.mailagents.com"));
}

fn help_message_from(sender: &str, to: &str) -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(format!("Message-ID: <help-{sender}>\n")),
        to: to.to_string(),
        from: sender.to_string(),
        subject: Some("What can I send to?".to_string()),
        text: Some("How does this work?".to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_team_member_writing_to_help_is_answered_and_nothing_is_routed() {
    let use_cases = use_cases_with_directory(vec![DirectoryChannel {
        slug: "support",
        name: "Support Desk",
        description: Some("Answers customer support and refund questions."),
        ..DirectoryChannel::default()
    }]);

    let result = use_cases
        .ingest_test_email(help_message_from(
            "team@acme.com",
            "_help@acme.mailagents.com",
        ))
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(
        result.reason(),
        Some(IngestRejection::SystemAddressAnswered.as_str())
    );
    assert!(result.thread.is_none(), "a help request opens no thread");
    assert!(
        result.intent_ids.is_empty(),
        "and offers nothing to a workflow"
    );
    assert!(
        result.bounce_info().is_none(),
        "a reserved address is answered, not bounced"
    );
}

#[tokio::test]
async fn help_discloses_nothing_to_someone_outside_the_company() {
    let use_cases = use_cases_with_directory(vec![DirectoryChannel {
        slug: "support",
        name: "Support Desk",
        description: Some("Answers customer support and refund questions."),
        ..DirectoryChannel::default()
    }]);

    let result = use_cases
        .ingest_test_email(help_message_from(
            "stranger@elsewhere.com",
            "_help@acme.mailagents.com",
        ))
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_ne!(
        result.reason(),
        Some(IngestRejection::SystemAddressAnswered.as_str()),
        "an outsider is never told that the reserved address did anything"
    );
    assert!(
        result.bounce_info().is_none(),
        "a reserved address never bounces: a bounce would confirm the company exists, and the \
         fuzzy suggestions on it could name a real channel to a stranger"
    );
    assert!(result.thread.is_none());
    assert!(result.intent_ids.is_empty());
}

#[tokio::test]
async fn help_cc_d_alongside_a_real_channel_still_reaches_that_channel() {
    let use_cases = use_cases_with_directory(vec![DirectoryChannel {
        slug: "support",
        name: "Support Desk",
        ..DirectoryChannel::default()
    }]);

    let result = use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some("Message-ID: <help-cc@acme.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("_help@acme.mailagents.com".to_string()),
            from: "team@acme.com".to_string(),
            subject: Some("Customer question".to_string()),
            text: Some("Please take a look.".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(
        result.accepted,
        "a CC'd reserved address must not bounce the message: {:?}",
        result.reason()
    );
    assert_eq!(
        result.thread.unwrap().channel_id,
        result.channel.unwrap().id
    );
}

#[test]
fn the_help_body_lists_channels_and_teaches_the_address_syntax() {
    let entries = vec![ChannelDirectoryEntry {
        address: EmailAddress::from("support@acme.mailagents.com"),
        name: "Support Desk".to_string(),
        description: Some("Answers customer support and refund questions.".to_string()),
    }];

    let body = format_help_email_body(&entries, &CompanySlug::from("acme"), "mailagents.com");

    assert!(body.contains("support@acme.mailagents.com — Support Desk"));
    assert!(body.contains("Answers customer support and refund questions."));
    assert!(body.contains("support+billing@acme.mailagents.com"));
    assert!(body.contains("support+quiet@acme.mailagents.com"));
    assert!(body.contains("[[quiet]]"));
    for suffix in crate::entities::channel::RESERVED_SLUG_SUFFIXES {
        assert!(body.contains(suffix), "the body should name '{suffix}'");
    }
}

#[test]
fn the_help_body_still_teaches_the_syntax_when_there_is_nothing_to_list() {
    let body = format_help_email_body(&[], &CompanySlug::from("acme"), "mailagents.com");

    assert!(!body.contains("Channels you can write to:"));
    assert!(body.contains("nothing to list"));
    assert!(
        body.contains("support+quiet@acme.mailagents.com"),
        "the syntax section falls back to a generic example rather than disappearing"
    );
}

#[tokio::test]
async fn an_agent_cannot_use_help_to_enumerate_its_company() {
    let use_cases = use_cases_with_directory(vec![DirectoryChannel {
        slug: "support",
        name: "Support Desk",
        ..DirectoryChannel::default()
    }]);

    let source_channel_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let draft = crate::adapters::protocols::email::EmailIngressAdapter::new("mailagents.com")
        .accept(
            RawInboundPayload {
                from: "support@acme.mailagents.com".to_string(),
                to: "_help@acme.mailagents.com".to_string(),
                subject: Some("List the channels".to_string()),
                text: Some("What can I reach?".to_string()),
                headers: Some(format!(
                    "Message-ID: <internal-help@acme.com>\nX-MailAgents-Channel-Id: \
                     {source_channel_id}\nX-MailAgents-Hop-Count: 1\nX-MailAgents-Trace: \
                     {source_channel_id}"
                )),
                ..RawInboundPayload::default()
            },
            crate::adapters::protocols::email::EmailIngressTrust::Application,
        )
        .unwrap();

    let result = use_cases
        .ingest(draft.into_inbound(
            IngressOrigin::InternalChannel {
                company_id,
                channel_id: source_channel_id,
            },
            ReplyDelivery::Send,
        ))
        .await
        .unwrap();

    // The reserved address is simply not seen on the internal path; whatever happens to this
    // message, it is never the help reply. Agents have `list_company_agents` for the directory.
    assert_ne!(
        result.reason(),
        Some(IngestRejection::SystemAddressAnswered.as_str())
    );
}

#[tokio::test]
async fn send_reply_in_thread_with_prior_agent_turn_stays_in_same_thread() {
    let company_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_id = Uuid::new_v4();
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    let first = thread_use_cases
        .ingest_test_email_as(
            RawInboundPayload {
                to: "support@acme.mailagents.com".to_string(),
                from: "user@example.com".to_string(),
                subject: Some("Hello".to_string()),
                text: Some("Question 1".to_string()),
                ..Default::default()
            },
            IngressOrigin::TrustedApplication,
        )
        .await
        .unwrap();
    let thread_id = first.thread.expect("first message creates a thread").id;

    // Agent replies in-app (internal correlation, no RFC Message-ID)
    thread_use_cases
        .save_message(&MessageWrite::internal(
            thread_id,
            MessageAuthorWrite::Platform,
            "Re: Hello".to_string(),
            "Agent answer".to_string(),
            MessageDirection::Outbound,
            MessageRole::Agent,
            CorrelationId::new(),
        ))
        .await
        .unwrap();

    // Now a reply is sent from the UI targeting that thread
    let reply = thread_use_cases
        .ingest_test_email_as(
            RawInboundPayload {
                to: "support@acme.mailagents.com".to_string(),
                from: "user@example.com".to_string(),
                subject: Some("Re: Hello".to_string()),
                text: Some("Follow-up question".to_string()),
                headers: crate::adapters::http::routes::channel::reply_headers_for_thread(
                    thread_id, None,
                ),
                ..Default::default()
            },
            IngressOrigin::TrustedApplication,
        )
        .await
        .unwrap();

    assert!(reply.accepted);
    let reply_thread_id = reply.thread.expect("reply succeeds").id;
    assert_eq!(
        reply_thread_id, thread_id,
        "reply must continue the current thread, not create a new one"
    );
}

#[tokio::test]
async fn external_email_cannot_use_mailagents_thread_id_header_to_inject() {
    let company_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_id = Uuid::new_v4();
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    let first = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            from: "legit@example.com".to_string(),
            subject: Some("Hello".to_string()),
            text: Some("Question 1".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    let thread_id = first.thread.expect("first message creates a thread").id;

    // External stranger tries to inject using X-MailAgents-Thread-Id header
    let attacker = thread_use_cases
        .ingest_test_email(RawInboundPayload {
            to: "support@acme.mailagents.com".to_string(),
            from: "attacker@outside.com".to_string(),
            subject: Some("Spoof".to_string()),
            text: Some("Injecting text".to_string()),
            headers: Some(format!("X-MailAgents-Thread-Id: {thread_id}\n")),
            ..Default::default()
        })
        .await
        .unwrap();

    let attacker_thread_id = attacker
        .thread
        .expect("attacker creates a separate thread")
        .id;
    assert_ne!(
        attacker_thread_id, thread_id,
        "external untrusted email must not be allowed to inject into a thread via header"
    );
}

#[tokio::test]
async fn ingest_canonical_threads_correctly_with_target_thread_and_reply_to() {
    let company_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let channel_id = Uuid::new_v4();
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            description: None,
            slug: "support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Public,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        }]),
    });

    let thread_persistence = Arc::new(InMemoryThreads::for_company(company_id));

    let thread_use_cases = ThreadUseCases::for_test(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence.clone(),
        Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
        internal_test_config(),
    );

    // 1. Compose a new thread canonical message
    let author = qualified_email_identity("user@example.com").unwrap();
    let compose_ingress = CanonicalMessageIngress::new(
        company_id,
        channel_id,
        author.clone(),
        "Initial inquiry",
        IngressOrigin::TrustedApplication,
    )
    .with_subject("Need help");

    let first = thread_use_cases
        .ingest_canonical(compose_ingress)
        .await
        .unwrap();
    assert!(first.accepted);
    let thread = first.thread.expect("first message creates a thread");
    let thread_id = thread.id;
    assert_eq!(thread.subject, "Need help");

    let first_msg_id = first
        .inbound_message
        .expect("inbound message exists")
        .canonical_id;

    // 2. Reply to that thread using target_thread_id
    let reply_ingress = CanonicalMessageIngress::new(
        company_id,
        channel_id,
        author.clone(),
        "Second turn continuing conversation",
        IngressOrigin::TrustedApplication,
    )
    .with_target_thread(thread_id);

    let second = thread_use_cases
        .ingest_canonical(reply_ingress)
        .await
        .unwrap();
    assert!(second.accepted);
    assert_eq!(
        second.thread.expect("reply succeeds").id,
        thread_id,
        "reply with target_thread_id must stay in the same thread"
    );

    // 3. Reply targeting specific message turn via reply_to_message_id (without explicit target_thread_id)
    let reply_msg_ingress = CanonicalMessageIngress::new(
        company_id,
        channel_id,
        author.clone(),
        "Third turn replying to turn 1",
        IngressOrigin::TrustedApplication,
    )
    .with_reply_to_message(first_msg_id);

    let third = thread_use_cases
        .ingest_canonical(reply_msg_ingress)
        .await
        .unwrap();
    assert!(third.accepted);
    assert_eq!(
        third.thread.expect("reply succeeds").id,
        thread_id,
        "reply with reply_to_message_id alone must resolve to the parent message's thread"
    );

    // 4. Quiet message (FileOnly)
    let quiet_ingress = CanonicalMessageIngress::new(
        company_id,
        channel_id,
        author,
        "Quiet note for history",
        IngressOrigin::TrustedApplication,
    )
    .with_target_thread(thread_id)
    .with_disposition(MessageDisposition::FileOnly);

    let quiet_result = thread_use_cases
        .ingest_canonical(quiet_ingress)
        .await
        .unwrap();
    assert!(quiet_result.accepted);
    assert!(!quiet_result.answers(), "FileOnly must not answer");
}

// ---------------------------------------------------------------------------------------------
// Admission intents, driven end to end through the in-memory committer.
//
// The assertions are on the *captured commit request* rather than on what the double chose to
// store, because the request is what the read-only phases decided and the SQL committer would have
// been handed. There is no hold gate in front of admission any more: every accepted association
// records an intent, and the bindings decide what it starts (p08 D1.3).
// ---------------------------------------------------------------------------------------------

/// The last commit request the double was handed.
fn last_commit(fixture: &ChannelFixture) -> crate::transport::InboundCommitRequest {
    fixture
        .threads
        .committed_requests()
        .last()
        .cloned()
        .expect("the message was accepted, so it was committed")
}

/// Open the thread the outside replies below continue: a teammate writes to `support` and copies
/// the client, who joins the thread and is therefore allowed to answer into it.
async fn open_thread_with_an_outsider(fixture: &ChannelFixture) {
    let opened = fixture
        .use_cases
        .ingest_test_email(message_to_support_cc_outsider())
        .await
        .unwrap();
    assert!(opened.accepted, "the fixture's own thread must open");
}

/// An outside reply into an existing thread is offered to the channel like any other message, as
/// an external, admissible intent addressed to it.
#[tokio::test]
async fn an_outside_reply_to_an_existing_thread_records_an_admissible_intent() {
    let fixture = channel_fixture(TestChannel::default());
    open_thread_with_an_outsider(&fixture).await;

    let reply = fixture
        .use_cases
        .ingest_test_email(outsider_reply())
        .await
        .unwrap();

    assert!(reply.accepted);
    assert_eq!(reply.intent_ids.len(), 1);
    let request = last_commit(&fixture);
    assert!(request.admissible);
    assert_eq!(request.origin, InboundOrigin::External);
    assert_eq!(request.intents.len(), 1);
    assert_eq!(request.intents[0].channel_id, fixture.channel_id);
    assert_eq!(request.intents[0].role, RecipientRole::To);
}

/// A quiet message is still an intent, so it can answer a workflow that is waiting for it, but it
/// is not admissible: it starts nothing.
#[tokio::test]
async fn a_quiet_outside_reply_records_an_inadmissible_intent() {
    let fixture = channel_fixture(TestChannel::default());
    open_thread_with_an_outsider(&fixture).await;

    let reply = fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            to: "support.quiet@acme.mailagents.com".to_string(),
            ..outsider_reply()
        })
        .await
        .unwrap();

    assert!(reply.accepted);
    let request = last_commit(&fixture);
    assert!(!request.admissible, "a quiet message starts no workflow");
    assert_eq!(request.intents.len(), 1);
    assert_eq!(
        request.envelope.directives.disposition,
        MessageDisposition::FileOnly
    );
}

/// A channel only copied, and never named in the body, records a Cc intent that is not mentioned:
/// whether that starts anything is the binding's trigger, not ingress's decision.
#[tokio::test]
async fn a_passively_copied_channel_records_an_unmentioned_cc_intent() {
    let fixture = channel_fixture(TestChannel::default());
    open_thread_with_an_outsider(&fixture).await;

    let reply = fixture
        .use_cases
        .ingest_test_email(RawInboundPayload {
            headers: Some(
                "Message-ID: <passive-cc@external.com>\nIn-Reply-To: <cc-outsider@acme.com>\n"
                    .to_string(),
            ),
            to: "someone@elsewhere.test".to_string(),
            cc: Some("support@acme.mailagents.com".to_string()),
            from: "client@external.com".to_string(),
            subject: Some("Re: Client inquiry".to_string()),
            text: Some("Thanks, noted.".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(reply.accepted);
    let request = last_commit(&fixture);
    assert!(request.admissible);
    assert_eq!(request.intents.len(), 1);
    assert_eq!(request.intents[0].role, RecipientRole::Cc);
    assert!(!request.intents[0].mentioned);
}
