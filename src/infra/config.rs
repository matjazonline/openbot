use std::env;

use p256::{ecdsa::VerifyingKey, pkcs8::DecodePublicKey};
use time::Duration;

use crate::{adapters::http::session::MIN_SECRET_BYTES, entities::value_objects::EmailAddress};

pub const DEFAULT_TASK_WORKER_CONCURRENCY: usize = 4;
pub const MAX_TASK_WORKER_CONCURRENCY: usize = 64;

/// Load the bounded background-task parallelism once during process startup.
pub fn task_worker_concurrency_from_env() -> usize {
    parse_task_worker_concurrency(env::var("TASK_WORKER_CONCURRENCY").ok().as_deref())
}

fn parse_task_worker_concurrency(value: Option<&str>) -> usize {
    let concurrency = value.map_or(DEFAULT_TASK_WORKER_CONCURRENCY, |value| {
        value
            .parse::<usize>()
            .expect("TASK_WORKER_CONCURRENCY must be a positive integer")
    });
    assert!(
        (1..=MAX_TASK_WORKER_CONCURRENCY).contains(&concurrency),
        "TASK_WORKER_CONCURRENCY must be between 1 and {MAX_TASK_WORKER_CONCURRENCY}"
    );
    concurrency
}

pub struct AppConfig {
    pub jwt_secret: String,
    pub refresh_token_ttl: Duration,
    pub app_domain_name: String,
    pub cors_allowed_origins: Vec<String>,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_from_address: String,
    pub incoming_smtp_enabled: bool,
    pub incoming_smtp_host: String,
    pub incoming_smtp_port: u16,
    pub max_spam_score: f64,
    pub dnsbl_enabled: bool,
    pub dnsbl_servers: Vec<String>,
    pub smtp_rate_limit_conns_per_ip: usize,
    pub reject_self_domain_helo: bool,
    pub enable_heuristic_scanner: bool,
    pub enable_spam_scanner: bool,
    pub spam_scanner_type: String,
    pub spam_scanner_url: String,
    pub enable_llm_spam_guardrail: bool,
    /// Whether session cookies are marked `Secure`, so a browser never sends one over plain HTTP.
    ///
    /// Derived from [`AppConfig::app_domain_name`] rather than defaulted, because the two
    /// deployments that exist -- localhost over HTTP and a real domain over HTTPS -- want opposite
    /// answers, and a `Secure` cookie on localhost is a cookie that silently never arrives.
    pub secure_cookies: bool,
    /// Where picked files are stored, when a bucket has been configured; `None` means this
    /// deployment cannot accept uploads and the pages that offer one say so.
    pub gcs: Option<GcsConfig>,
    /// Addresses allowed to see the whole system rather than one company on `/ui/dashboard`.
    ///
    /// Deliberately deploy-controlled rather than a database role: `company_members.role` grants
    /// `admin` *within one company*, which is not the same authority as reading every company's
    /// traffic. Empty by default, so the global view does not exist until someone is named.
    pub operator_emails: Vec<EmailAddress>,
    /// Authenticated SendGrid inbound webhook configuration. `None` disables the route.
    pub sendgrid_inbound: Option<SendGridInboundConfig>,
}

pub struct SendGridInboundConfig {
    pub verifying_key: VerifyingKey,
    pub webhook_max_age_secs: u64,
}

#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone)]
pub struct AppleOAuthConfig {
    pub client_id: String,
    pub team_id: String,
    pub key_id: String,
    pub private_key_base64: String,
}

impl AppleOAuthConfig {
    pub fn from_env() -> Option<Self> {
        let values = (
            non_empty_var("APPLE_OAUTH_CLIENT_ID"),
            non_empty_var("APPLE_OAUTH_TEAM_ID"),
            non_empty_var("APPLE_OAUTH_KEY_ID"),
            non_empty_var("APPLE_OAUTH_PRIVATE_KEY_BASE64"),
        );
        match values {
            (Some(client_id), Some(team_id), Some(key_id), Some(private_key_base64)) => {
                Some(Self {
                    client_id,
                    team_id,
                    key_id,
                    private_key_base64,
                })
            }
            (None, None, None, None) => None,
            _ => panic!(
                "APPLE_OAUTH_CLIENT_ID, APPLE_OAUTH_TEAM_ID, APPLE_OAUTH_KEY_ID and APPLE_OAUTH_PRIVATE_KEY_BASE64 must either all be set or all be absent"
            ),
        }
    }
}

impl GoogleOAuthConfig {
    /// Loads Google sign-in when both credentials are present. A half-configured deployment is a
    /// startup/configuration error rather than a button that fails after it is clicked.
    pub fn from_env() -> Option<Self> {
        match (
            non_empty_var("GOOGLE_OAUTH_CLIENT_ID"),
            non_empty_var("GOOGLE_OAUTH_CLIENT_SECRET"),
        ) {
            (Some(client_id), Some(client_secret)) => Some(Self {
                client_id,
                client_secret,
            }),
            (None, None) => None,
            _ => panic!(
                "GOOGLE_OAUTH_CLIENT_ID and GOOGLE_OAUTH_CLIENT_SECRET must either both be set or both be absent"
            ),
        }
    }
}

/// The Google Cloud Storage bucket uploads are written to.
///
/// One value rather than three loose `Option<String>`s on [`AppConfig`], because a bucket without
/// a key (or the reverse) is not a half-configured uploader — it is no uploader at all, and this
/// makes that unrepresentable.
#[derive(Debug, Clone)]
pub struct GcsConfig {
    pub bucket: String,
    /// The service account key file, base64-encoded: JSON does not survive an environment variable
    /// intact, and the key is multi-line PEM inside JSON.
    pub service_account_json_base64: String,
    /// A CDN or custom domain in front of the bucket; `None` serves straight from Google.
    pub public_base_url_override: Option<String>,
    /// The folder inside the public bucket avatars are written to.
    pub avatar_folder: String,
    /// The bucket mail attachments are written to, which must **not** be publicly readable: what
    /// arrives on a channel is only ever served through the app, to somebody the channel's own
    /// rules allow. `None` means attachments are not stored at all.
    pub attachments_bucket: Option<String>,
    /// The folder inside the private bucket attachments are written to.
    pub attachments_folder: String,
}

impl GcsConfig {
    /// Storage configuration, or `None` when this deployment has no bucket.
    ///
    /// Both the bucket and the key are required together: a deployment naming one but not the
    /// other has a mistake in it, and starting with uploads silently disabled would hide it until
    /// somebody tried to change their picture.
    fn from_env() -> Option<Self> {
        let bucket = non_empty_var("GCS_BUCKET");
        let service_account_json_base64 = non_empty_var("GCS_SERVICE_ACCOUNT_JSON_BASE64");

        match (bucket, service_account_json_base64) {
            (Some(bucket), Some(service_account_json_base64)) => Some(Self {
                bucket,
                service_account_json_base64,
                public_base_url_override: non_empty_var("GCS_PUBLIC_BASE_URL"),
                avatar_folder: non_empty_var("GCS_AVATAR_FOLDER")
                    .unwrap_or_else(|| "avatars".to_string()),
                attachments_bucket: non_empty_var("GCS_ATTACHMENTS_BUCKET"),
                attachments_folder: non_empty_var("GCS_ATTACHMENTS_FOLDER")
                    .unwrap_or_else(|| "attachments".to_string()),
            }),
            (None, None) => None,
            (bucket, _) => panic!(
                "{} is set but {} is not; both are needed to store uploads",
                if bucket.is_some() {
                    "GCS_BUCKET"
                } else {
                    "GCS_SERVICE_ACCOUNT_JSON_BASE64"
                },
                if bucket.is_some() {
                    "GCS_SERVICE_ACCOUNT_JSON_BASE64"
                } else {
                    "GCS_BUCKET"
                },
            ),
        }
    }

    /// What an uploaded object's URL is built from.
    pub fn public_base_url(&self) -> String {
        self.public_base_url_override.clone().unwrap_or_else(|| {
            format!(
                "https://storage.googleapis.com/{bucket}",
                bucket = self.bucket
            )
        })
    }
}

/// Whether this is a developer's machine rather than a deployment, for the purpose of cookies.
fn is_local_domain(domain: &str) -> bool {
    let domain = domain.trim().to_ascii_lowercase();
    let domain = domain.split(':').next().unwrap_or(&domain);

    domain.is_empty() || domain == "localhost" || domain == "127.0.0.1" || domain == "[::1]"
}

/// An environment variable that is set to something, treating blank as unset.
///
/// A deploy platform writes an empty string for a secret that was never filled in, and an empty
/// bucket name is not a bucket.
fn non_empty_var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl AppConfig {
    /// Whether registration must prove ownership of the supplied email address.
    /// Local SMTP defaults deliberately do not turn this on.
    pub fn email_confirmation_enabled(&self) -> bool {
        !self.smtp_host.trim().is_empty()
            && self.smtp_host != "localhost"
            && !self.smtp_from_address.trim().is_empty()
            && self.smtp_from_address != "noreply@localhost"
    }

    /// Is this the address of a system operator, and so entitled to the cross-company rollup?
    pub fn is_operator(&self, email: &EmailAddress) -> bool {
        self.operator_emails
            .iter()
            .any(|operator| operator.eq_ignore_case(email))
    }

    pub fn from_env() -> Self {
        // Validate the optional pair at startup even though the OAuth routes load it only when a
        // flow begins (credentials are deliberately not copied into the broad application config).
        let _ = GoogleOAuthConfig::from_env();
        let _ = AppleOAuthConfig::from_env();
        let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
        assert!(
            jwt_secret.len() >= MIN_SECRET_BYTES,
            "JWT_SECRET must be at least {MIN_SECRET_BYTES} characters: it signs every session, \
             so a guessable one is every account at once",
        );

        let refresh_token_ttl_days: i64 = env::var("REFRESH_TOKEN_TTL_DAYS")
            .unwrap_or("30".to_string())
            .parse()
            .expect("REFRESH_TOKEN_TTL_DAYS must be a valid number");

        let app_domain_name =
            env::var("APP_DOMAIN_NAME").unwrap_or_else(|_| "localhost".to_string());

        // Overridable for the deployment that terminates TLS somewhere this app cannot see.
        let secure_cookies = match env::var("COOKIE_SECURE").ok().as_deref() {
            Some(setting) => setting.trim().eq_ignore_ascii_case("true"),
            None => !is_local_domain(&app_domain_name),
        };

        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5173".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // No default: an unset variable means nobody holds the cross-company view.
        let operator_emails = env::var("OPERATOR_EMAILS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(EmailAddress::from)
            .collect();

        let smtp_host = env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string());
        let smtp_port: u16 = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "1025".to_string())
            .parse()
            .unwrap_or(1025);
        let smtp_username = env::var("SMTP_USERNAME").unwrap_or_default();
        let smtp_password = env::var("SMTP_PASSWORD").unwrap_or_default();
        let smtp_from_address =
            env::var("SMTP_FROM_ADDRESS").unwrap_or_else(|_| "noreply@localhost".to_string());

        let incoming_smtp_enabled: bool = env::var("INCOMING_SMTP_ENABLED")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);
        let incoming_smtp_host =
            env::var("INCOMING_SMTP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let incoming_smtp_port: u16 = env::var("INCOMING_SMTP_PORT")
            .unwrap_or_else(|_| "2525".to_string())
            .parse()
            .unwrap_or(2525);

        let sendgrid_inbound_enabled: bool = env::var("SENDGRID_INBOUND_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .expect("SENDGRID_INBOUND_ENABLED must be true or false");
        let sendgrid_webhook_public_key = non_empty_var("SENDGRID_WEBHOOK_PUBLIC_KEY");
        assert!(
            !sendgrid_inbound_enabled || sendgrid_webhook_public_key.is_some(),
            "SENDGRID_WEBHOOK_PUBLIC_KEY is required when SENDGRID_INBOUND_ENABLED=true"
        );
        let sendgrid_verifying_key = if sendgrid_inbound_enabled {
            Some(
                VerifyingKey::from_public_key_pem(
                    sendgrid_webhook_public_key
                        .as_deref()
                        .expect("checked above"),
                )
                .expect(
                    "SENDGRID_WEBHOOK_PUBLIC_KEY must be a valid ECDSA P-256 public key in PEM format",
                ),
            )
        } else {
            None
        };
        let sendgrid_webhook_max_age_secs: u64 = env::var("SENDGRID_WEBHOOK_MAX_AGE_SECS")
            .unwrap_or_else(|_| "300".to_string())
            .parse()
            .expect("SENDGRID_WEBHOOK_MAX_AGE_SECS must be a positive integer");
        assert!(
            sendgrid_webhook_max_age_secs > 0,
            "SENDGRID_WEBHOOK_MAX_AGE_SECS must be positive"
        );
        let sendgrid_inbound = sendgrid_verifying_key.map(|verifying_key| SendGridInboundConfig {
            verifying_key,
            webhook_max_age_secs: sendgrid_webhook_max_age_secs,
        });

        let max_spam_score: f64 = env::var("MAX_SPAM_SCORE")
            .unwrap_or_else(|_| "5.0".to_string())
            .parse()
            .unwrap_or(5.0);

        let dnsbl_enabled: bool = env::var("INCOMING_SMTP_DNSBL_ENABLED")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false);

        let dnsbl_servers = env::var("INCOMING_SMTP_DNSBL_SERVERS")
            .unwrap_or_else(|_| "zen.spamhaus.org,bl.spamcop.net".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let smtp_rate_limit_conns_per_ip: usize = env::var("INCOMING_SMTP_RATE_LIMIT_PER_IP")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .unwrap_or(30);

        let reject_self_domain_helo: bool = env::var("INCOMING_SMTP_REJECT_SELF_HELO")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        let enable_heuristic_scanner: bool = env::var("ENABLE_HEURISTIC_SCANNER")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        let enable_spam_scanner: bool = env::var("ENABLE_SPAM_SCANNER")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false);

        let spam_scanner_type =
            env::var("SPAM_SCANNER_TYPE").unwrap_or_else(|_| "rspamd".to_string());

        let spam_scanner_url = env::var("SPAM_SCANNER_URL")
            .unwrap_or_else(|_| "http://localhost:11333/checkv2".to_string());

        let enable_llm_spam_guardrail: bool = env::var("ENABLE_LLM_SPAM_GUARDRAIL")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false);

        Self {
            jwt_secret,
            refresh_token_ttl: Duration::days(refresh_token_ttl_days),
            app_domain_name,
            cors_allowed_origins,
            smtp_host,
            smtp_port,
            smtp_username,
            smtp_password,
            smtp_from_address,
            incoming_smtp_enabled,
            incoming_smtp_host,
            incoming_smtp_port,
            max_spam_score,
            dnsbl_enabled,
            dnsbl_servers,
            smtp_rate_limit_conns_per_ip,
            reject_self_domain_helo,
            enable_heuristic_scanner,
            enable_spam_scanner,
            spam_scanner_type,
            spam_scanner_url,
            enable_llm_spam_guardrail,
            secure_cookies,
            gcs: GcsConfig::from_env(),
            operator_emails,
            sendgrid_inbound,
        }
    }

    pub fn is_spam_scan_enabled(&self) -> bool {
        self.enable_heuristic_scanner || self.enable_spam_scanner || self.enable_llm_spam_guardrail
    }
}

#[cfg(test)]
impl AppConfig {
    /// A configuration with everything at its default, for tests that care about one field.
    ///
    /// Spelling out all of these at each test's call site is what made adding a field a
    /// twenty-seven-file change; `..AppConfig::for_test()` keeps the next one to this function.
    pub fn for_test() -> Self {
        Self {
            jwt_secret: "a-test-secret-long-enough-to-sign-with-01".to_string(),
            refresh_token_ttl: Duration::days(30),
            app_domain_name: "localhost".to_string(),
            cors_allowed_origins: Vec::new(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: String::new(),
            smtp_password: String::new(),
            smtp_from_address: "noreply@localhost".to_string(),
            incoming_smtp_enabled: false,
            incoming_smtp_host: "0.0.0.0".to_string(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: Vec::new(),
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: false,
            enable_heuristic_scanner: false,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: String::new(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
            sendgrid_inbound: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough to pass the startup check, which is itself part of what is under test.
    const TEST_SECRET: &str = "a-test-secret-long-enough-to-sign-with-01";

    #[test]
    fn task_worker_concurrency_is_bounded_and_defaults_to_four() {
        assert_eq!(parse_task_worker_concurrency(None), 4);
        assert_eq!(parse_task_worker_concurrency(Some("8")), 8);
    }

    #[test]
    #[should_panic(expected = "TASK_WORKER_CONCURRENCY must be between 1 and 64")]
    fn task_worker_concurrency_rejects_zero() {
        parse_task_worker_concurrency(Some("0"));
    }

    #[test]
    fn test_app_config_from_env() {
        unsafe {
            env::set_var("JWT_SECRET", TEST_SECRET);
            env::set_var("APP_DOMAIN_NAME", "example.com");
        }

        let config = AppConfig::from_env();
        assert_eq!(config.jwt_secret, TEST_SECRET);
        assert_eq!(config.app_domain_name, "example.com");
        // A real domain is served over HTTPS, so its cookies say so.
        assert!(config.secure_cookies);
    }

    #[test]
    fn a_developer_machine_gets_cookies_it_can_send() {
        assert!(is_local_domain("localhost"));
        assert!(is_local_domain("LocalHost:3001"));
        assert!(is_local_domain("127.0.0.1"));
        assert!(is_local_domain(""));

        assert!(!is_local_domain("example.com"));
        assert!(!is_local_domain("mail.example.com"));
    }
}
