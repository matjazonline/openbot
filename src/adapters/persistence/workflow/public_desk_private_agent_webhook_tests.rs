//! Actual signed HTTP handler → durable inbox/claim/decoder/commit → workflow/publication.
//! Credential lookup and provider SDK responses are explicit external-service stand-ins.
use super::*;
use crate::{
    adapters::{
        http::routes::webhooks::resend_api::{ResendApiWebhookState, resend_api_inbound_webhook},
        monitoring::InMemoryMonitor,
        resend_api::{
            CompanyResendApiClients, ResendApiInboundDecoder,
            client::{RawReference, ReceivedEmail},
            test_support::FakeResendApi,
        },
    },
    application::{
        services::inbound_event_worker::{InboundEventWakeups, InboundEventWorker},
        use_cases::company_resend_api::CompanyResendApiAccounts,
    },
    entities::{
        company_resend_api::{ResendApiAccountCredentials, ResendApiInboundCredentials},
        value_objects::{AuthservId, ResendApiWebhookToken},
    },
    transport::InboundEventDecoderRegistry,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use base64::Engine;
use secrecy::SecretString;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
const OTHER_SECRET: &str = "whsec_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

struct Account(Uuid);
#[async_trait::async_trait]
impl CompanyResendApiAccounts for Account {
    async fn inbound_credentials(
        &self,
        token: &ResendApiWebhookToken,
    ) -> AppResult<Option<ResendApiInboundCredentials>> {
        Ok(
            (token.as_str() == TOKEN).then(|| ResendApiInboundCredentials {
                company_id: self.0,
                signing_secret: SecretString::from(SECRET),
            }),
        )
    }
    async fn account_credentials(
        &self,
        company: Uuid,
    ) -> AppResult<Option<ResendApiAccountCredentials>> {
        Ok((company == self.0).then(|| ResendApiAccountCredentials {
            api_key: SecretString::from("fixture-provider-key"),
            authserv_id: AuthservId::parse("resend.com").unwrap(),
        }))
    }
}

enum ProviderAuth {
    ReceivingMta,
    ForgedTopHeader,
}
struct Mail {
    id: Uuid,
    recipient: EmailAddress,
    auth: ProviderAuth,
    score: f64,
    sender: EmailAddress,
    cc: Option<EmailAddress>,
}
impl Mail {
    fn to(recipient: EmailAddress) -> Self {
        Self {
            id: Uuid::new_v4(),
            recipient,
            auth: ProviderAuth::ReceivingMta,
            score: 0.0,
            sender: CUSTOMER.into(),
            cc: None,
        }
    }
    fn raw(&self) -> Vec<u8> {
        let prefix = match self.auth {
            ProviderAuth::ReceivingMta => "",
            ProviderAuth::ForgedTopHeader => {
                "Authentication-Results: attacker.test; dmarc=pass\r\n"
            }
        };
        let domain = self.sender.rsplit('@').next().unwrap();
        let cc = self
            .cc
            .as_ref()
            .map(|address| format!("Cc: {address}\r\n"))
            .unwrap_or_default();
        format!("{prefix}Authentication-Results: resend.com; spf=pass smtp.mailfrom={domain}; dkim=pass header.d={domain}; dmarc=pass header.from={domain}\r\nMessage-ID: <{}@provider.test>\r\nFrom: {}\r\nTo: {}\r\n{cc}Subject: Signed webhook desk mail\r\n\r\nPlease answer this message.\r\n", self.id, self.sender, self.recipient).into_bytes()
    }
}

struct Webhook {
    router: Router,
    worker: Arc<InboundEventWorker>,
    persistence: PostgresPersistence,
    company: Uuid,
    mail: Uuid,
    body: String,
}

impl Webhook {
    fn new(d: &Deployment, mail: Mail) -> Self {
        let company = d.fixture.target.company.as_uuid();
        let p = Arc::new(d.p().clone());
        let config = Arc::new(AppConfig {
            app_domain_name: APP_DOMAIN.into(),
            ..AppConfig::for_test()
        });
        let accounts = Arc::new(Account(company));
        let api = FakeResendApi::new()
            .retrieving(Ok(ReceivedEmail {
                id: mail.id.to_string(),
                from: mail.sender.to_string(),
                to: vec![mail.recipient.to_string()],
                received_for: vec![mail.recipient.to_string()],
                raw: Some(RawReference {
                    download_url: "https://inbound-cdn.resend.com/fixture".into(),
                    expires_at: None,
                }),
            }))
            .with_raw(Ok(mail.raw()));
        let clients = Arc::new(
            CompanyResendApiClients::new(accounts.clone(), config.resend_api.clone())
                .unwrap()
                .with_api_for_test(Arc::new(api)),
        );
        let threads = Arc::new(
            crate::adapters::persistence::test_support::thread_use_cases(d.p(), APP_DOMAIN),
        );
        let decoders = InboundEventDecoderRegistry::new()
            .register(Arc::new(ResendApiInboundDecoder::new(
                clients,
                config.clone(),
                threads,
                None,
                Arc::new(super::smtp::Score(mail.score)),
            )))
            .unwrap();
        let wakeups = InboundEventWakeups::new();
        let worker = Arc::new(InboundEventWorker::new(
            p.clone(),
            p.clone(),
            Arc::new(decoders),
            wakeups.clone(),
        ));
        let state = ResendApiWebhookState {
            config,
            accounts,
            inbox: p,
            wakeups,
            monitoring: Arc::new(InMemoryMonitor::new()),
        };
        let router = Router::new()
            .route(
                "/webhooks/email/resend_api/{token}",
                post(resend_api_inbound_webhook),
            )
            .with_state(state);
        let body = json!({"type":"email.received","data":{"email_id":mail.id,"received_for":[mail.recipient]}}).to_string();
        Self {
            router,
            worker,
            persistence: d.p().clone(),
            company,
            mail: mail.id,
            body,
        }
    }

    async fn post(&self, key: &ring::hmac::Key) -> StatusCode {
        let id = format!("msg_{}", Uuid::new_v4());
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signed = format!("{id}.{timestamp}.{}", self.body);
        let signature = base64::engine::general_purpose::STANDARD
            .encode(ring::hmac::sign(key, signed.as_bytes()).as_ref());
        self.router
            .clone()
            .oneshot(
                Request::post(format!("/webhooks/email/resend_api/{TOKEN}"))
                    .header("content-type", "application/json")
                    .header("svix-id", id)
                    .header("svix-timestamp", timestamp)
                    .header("svix-signature", format!("v1,{signature}"))
                    .body(Body::from(self.body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn process(&self) -> String {
        let (shutdown, receiver) = tokio::sync::broadcast::channel(1);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(self.worker.clone().run(receiver));
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let state: String = sqlx::query_scalar("SELECT status FROM inbound_events WHERE company_id=$1 AND external_event_key=$2")
                    .bind(self.company).bind(self.mail.to_string()).fetch_one(self.persistence.pool()).await.unwrap();
                if matches!(state.as_str(), "completed" | "ignored" | "dead_letter") { return state; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        shutdown.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(result) = tasks.join_next().await {
                result.unwrap();
            }
        })
        .await
        .expect("inbound worker shuts down");
        result.expect("the real fenced inbox claim reaches a terminal source state")
    }
}

fn key(secret: &str) -> ring::hmac::Key {
    ring::hmac::Key::new(
        ring::hmac::HMAC_SHA256,
        &crate::adapters::resend_api::decode_signing_secret(secret).unwrap(),
    )
}

async fn rejections(d: &Deployment, endpoint: &PrincipalEndpoint) {
    let direct = Mail::to(endpoint.address.clone());
    let mut forged = Mail::to(d.address("support").into());
    forged.auth = ProviderAuth::ForgedTopHeader;
    let mut spam = Mail::to(d.address("support").into());
    spam.score = 99.0;
    for mail in [direct, forged, spam] {
        let webhook = Webhook::new(d, mail);
        assert_eq!(webhook.post(&key(SECRET)).await, StatusCode::OK);
        assert_eq!(webhook.process().await, "ignored");
        assert_eq!(
            d.count("SELECT count(*) FROM messages WHERE company_id=$1 AND direction='inbound'")
                .await,
            1
        );
    }
}

pub(in super::super::super) async fn receive_member_mail(
    d: &Deployment,
    sender: EmailAddress,
    cc: Option<EmailAddress>,
) -> Uuid {
    let mut mail = Mail::to(d.address("support").into());
    mail.sender = sender;
    mail.cc = cc;
    let webhook = Webhook::new(d, mail);
    assert_eq!(webhook.post(&key(SECRET)).await, StatusCode::OK);
    assert_eq!(webhook.process().await, "completed");
    only_desk_thread(d).await
}

#[tokio::test]
async fn public_desk_private_agent_signed_webhook_claims_real_inbox_and_preserves_auth_spam_and_reply_scope()
 {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"Webhook desk answer"}"#,
    )]))
    .await;
    let endpoint = private_endpoint(&d).await;
    open_desk(&d).await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let webhook = Webhook::new(&d, Mail::to(d.address("support").into()));
    assert_eq!(
        webhook.post(&key(OTHER_SECRET)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        d.count("SELECT count(*) FROM inbound_events WHERE company_id=$1")
            .await,
        0
    );
    assert_eq!(webhook.post(&key(SECRET)).await, StatusCode::OK);
    assert_eq!(webhook.process().await, "completed");
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("signed webhook reply from private agent", async || {
            responses_published(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let rows = finals(&d).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        provider.messages()[0].email.body_text,
        "Webhook desk answer"
    );
    assert_desk_publication(&d, rows[0].source, rows[0].thread, &endpoint).await;
    rejections(&d, &endpoint).await;
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(d.fixture.target.company.as_uuid())
        .bind(endpoint.principal.as_uuid())
        .execute(d.p().pool())
        .await
        .unwrap();
    let direct = Webhook::new(&d, Mail::to(endpoint.address));
    assert_eq!(direct.post(&key(SECRET)).await, StatusCode::OK);
    assert_eq!(direct.process().await, "completed");
    restricted_desk(&d).await;
    let restricted = Webhook::new(&d, Mail::to(d.address("restricted").into()));
    assert_eq!(restricted.post(&key(SECRET)).await, StatusCode::OK);
    assert_eq!(restricted.process().await, "ignored");
    assert_eq!(
        d.count("SELECT count(*) FROM messages WHERE company_id=$1 AND direction='inbound'")
            .await,
        2
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
