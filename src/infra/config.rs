use std::{env, time::Duration as StdDuration};

use p256::{ecdsa::VerifyingKey, pkcs8::DecodePublicKey};
use time::Duration;

use crate::{adapters::http::session::MIN_SECRET_BYTES, entities::value_objects::EmailAddress};

/// Stack size for the async runtime's worker and blocking threads.
///
/// Tokio's 2 MiB default is not enough for this process. A background job descends through several
/// levels of `async fn` whose unoptimized poll frames run to a few hundred KiB each; the agent
/// dispatch chain the workflow runtime replaced measured 2,069 KiB on one aborted run and hit the
/// guard page. Reserve enough that the depth is not the thing that decides whether a job
/// completes; `scripts/stack-budget.sh` is what notices the chain growing.
pub const DEFAULT_RUNTIME_THREAD_STACK_BYTES: usize = 16 * 1024 * 1024;
/// Below Tokio's own default the process would be less stable than leaving this unset.
pub const MIN_RUNTIME_THREAD_STACK_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RUNTIME_THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

/// Load the async runtime's per-thread stack reservation once during process startup.
pub fn runtime_thread_stack_bytes_from_env() -> usize {
    parse_runtime_thread_stack_bytes(env::var("RUNTIME_THREAD_STACK_BYTES").ok().as_deref())
}

fn parse_runtime_thread_stack_bytes(value: Option<&str>) -> usize {
    let bytes = value.map_or(DEFAULT_RUNTIME_THREAD_STACK_BYTES, |value| {
        value
            .parse::<usize>()
            .expect("RUNTIME_THREAD_STACK_BYTES must be a positive integer")
    });
    assert!(
        (MIN_RUNTIME_THREAD_STACK_BYTES..=MAX_RUNTIME_THREAD_STACK_BYTES).contains(&bytes),
        "RUNTIME_THREAD_STACK_BYTES must be between {MIN_RUNTIME_THREAD_STACK_BYTES} and \
         {MAX_RUNTIME_THREAD_STACK_BYTES}"
    );
    bytes
}

pub fn smtp_allow_plaintext_local_from_env() -> bool {
    parse_smtp_allow_plaintext_local(env::var("SMTP_ALLOW_PLAINTEXT_LOCAL").ok().as_deref())
}

fn parse_smtp_allow_plaintext_local(value: Option<&str>) -> bool {
    value
        .unwrap_or("false")
        .parse::<bool>()
        .expect("SMTP_ALLOW_PLAINTEXT_LOCAL must be true or false")
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
    /// How this deployment reaches Resend, for the companies that have configured an account.
    pub resend_api: ResendApiConfig,
}

fn parse_positive_seconds(name: &str, value: String) -> StdDuration {
    let seconds = value
        .parse::<u64>()
        .unwrap_or_else(|_| panic!("{name} must be a positive integer"));
    assert!(seconds > 0, "{name} must be positive");
    StdDuration::from_secs(seconds)
}

pub struct SendGridInboundConfig {
    pub verifying_key: VerifyingKey,
    pub webhook_max_age_secs: u64,
}

/// The largest `Retry-After` a Resend 429 can push a delivery out by. Beyond this the queue's own
/// backoff is the better answer, because a provider asking for an hour is reporting an outage.
pub const MAX_RESEND_API_RETRY_AFTER_SECS: u64 = 900;
pub const DEFAULT_RESEND_API_BASE_URL: &str = "https://api.resend.com";
pub const DEFAULT_RESEND_API_REQUEST_TIMEOUT_SECS: u64 = 30;
pub const MAX_RESEND_API_REQUEST_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_RESEND_API_WEBHOOK_MAX_AGE_SECS: u64 = 300;

/// How this deployment talks to Resend, for whichever companies have configured an account.
///
/// No credential lives here. The API key and the webhook signing secret belong to a company, not
/// to the deployment -- see `company_resend_api_integrations` -- and a deployment-wide key would be a
/// key with which one tenant's mail could be fetched under another tenant's account. What is left
/// is the dial settings, which genuinely are the same for every tenant: where the API lives, how
/// long one of its requests may take, and how old a signed webhook may be before it is a replay.
///
/// Always present, and every field has a default, so there is no "Resend is configured" state at
/// this level any more: the answer to that question is a row, per company.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResendApiConfig {
    pub base_url: String,
    pub request_timeout: StdDuration,
    pub webhook_max_age_secs: u64,
}

impl Default for ResendApiConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_RESEND_API_BASE_URL.to_string(),
            request_timeout: StdDuration::from_secs(DEFAULT_RESEND_API_REQUEST_TIMEOUT_SECS),
            webhook_max_age_secs: DEFAULT_RESEND_API_WEBHOOK_MAX_AGE_SECS,
        }
    }
}

impl ResendApiConfig {
    fn from_env() -> Self {
        Self::from_values(
            non_empty_var("RESEND_API_BASE_URL"),
            non_empty_var("RESEND_API_REQUEST_TIMEOUT_SECS"),
            non_empty_var("RESEND_API_WEBHOOK_MAX_AGE_SECS"),
        )
    }

    /// Split out from [`Self::from_env`] so the validation is testable without mutating the
    /// process environment.
    fn from_values(
        base_url: Option<String>,
        request_timeout: Option<String>,
        webhook_max_age_secs: Option<String>,
    ) -> Self {
        let defaults = Self::default();
        let base_url = base_url.unwrap_or(defaults.base_url);
        let parsed_url = url::Url::parse(&base_url)
            .unwrap_or_else(|_| panic!("RESEND_API_BASE_URL must be an absolute https URL"));
        assert!(
            parsed_url.scheme() == "https" && parsed_url.host().is_some(),
            "RESEND_API_BASE_URL must be an absolute https URL"
        );
        let request_timeout = request_timeout.map_or(defaults.request_timeout, |value| {
            parse_positive_seconds("RESEND_API_REQUEST_TIMEOUT_SECS", value)
        });
        assert!(
            request_timeout.as_secs() <= MAX_RESEND_API_REQUEST_TIMEOUT_SECS,
            "RESEND_API_REQUEST_TIMEOUT_SECS must be at most {MAX_RESEND_API_REQUEST_TIMEOUT_SECS}"
        );
        let webhook_max_age_secs =
            webhook_max_age_secs.map_or(defaults.webhook_max_age_secs, |value| {
                let seconds: u64 = value
                    .parse()
                    .expect("RESEND_API_WEBHOOK_MAX_AGE_SECS must be a positive integer");
                assert!(
                    seconds > 0,
                    "RESEND_API_WEBHOOK_MAX_AGE_SECS must be positive"
                );
                seconds
            });
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            request_timeout,
            webhook_max_age_secs,
        }
    }
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
    /// The folder inside the private bucket attachments are written to. By default it namespaces
    /// them under this deployment's application domain.
    pub attachments_folder: String,
}

impl GcsConfig {
    /// Storage configuration, or `None` when this deployment has no bucket.
    ///
    /// Both the bucket and the key are required together: a deployment naming one but not the
    /// other has a mistake in it, and starting with uploads silently disabled would hide it until
    /// somebody tried to change their picture.
    fn from_env(app_domain_name: &str) -> Option<Self> {
        let bucket = non_empty_var("GCS_AVATAR_BUCKET_PUBLIC");
        let service_account_json_base64 = non_empty_var("GCS_SERVICE_ACCOUNT_JSON_BASE64");

        match (bucket, service_account_json_base64) {
            (Some(bucket), Some(service_account_json_base64)) => Some(Self {
                bucket,
                service_account_json_base64,
                public_base_url_override: non_empty_var("GCS_PUBLIC_BASE_URL"),
                avatar_folder: non_empty_var("GCS_AVATAR_FOLDER")
                    .unwrap_or_else(|| "avatars".to_string()),
                attachments_bucket: non_empty_var("GCS_ATTACHMENTS_BUCKET_PRIVATE"),
                attachments_folder: non_empty_var("GCS_ATTACHMENTS_FOLDER")
                    .unwrap_or_else(|| default_attachments_folder(app_domain_name)),
            }),
            (None, None) => None,
            (bucket, _) => panic!(
                "{} is set but {} is not; both are needed to store uploads",
                if bucket.is_some() {
                    "GCS_AVATAR_BUCKET_PUBLIC"
                } else {
                    "GCS_SERVICE_ACCOUNT_JSON_BASE64"
                },
                if bucket.is_some() {
                    "GCS_SERVICE_ACCOUNT_JSON_BASE64"
                } else {
                    "GCS_AVATAR_BUCKET_PUBLIC"
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

fn default_attachments_folder(app_domain_name: &str) -> String {
    format!("{app_domain_name}/mail-attachments")
}

/// Whether this is a developer's machine rather than a deployment, for the purpose of cookies.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    host.is_empty()
        || host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn is_local_domain(domain: &str) -> bool {
    let domain = domain.trim();
    let host = if domain.starts_with('[') {
        domain.split_once(']').map_or(domain, |(host, _)| host)
    } else {
        domain.split_once(':').map_or(domain, |(host, _)| host)
    };
    is_loopback_host(host)
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
            && !is_loopback_host(&self.smtp_host)
            && !self.smtp_from_address.trim().is_empty()
            && self.smtp_from_address != "noreply@localhost"
    }

    /// The absolute origin this deployment is reached at, e.g. `https://example.com`.
    ///
    /// For the values that have to be *given* to somebody outside the browser -- a webhook URL
    /// pasted into a provider's dashboard, above all -- rather than linked from a page, where a
    /// relative path would do.
    ///
    /// The scheme follows [`AppConfig::secure_cookies`] rather than a second environment variable:
    /// both answer the one question of whether this deployment is served over TLS, and two
    /// settings that can disagree is how a page comes to show a URL nobody can reach.
    pub fn public_base_url(&self) -> String {
        let domain = self.app_domain_name.trim().trim_end_matches('/');
        let host = domain
            .strip_prefix("https://")
            .or_else(|| domain.strip_prefix("http://"))
            .unwrap_or(domain);
        let scheme = if self.secure_cookies { "https" } else { "http" };
        format!("{scheme}://{host}")
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

        let resend_api = ResendApiConfig::from_env();

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

        let gcs = GcsConfig::from_env(&app_domain_name);

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
            secure_cookies,
            gcs,
            operator_emails,
            sendgrid_inbound,
            resend_api,
        }
    }

    pub fn is_spam_scan_enabled(&self) -> bool {
        self.enable_heuristic_scanner || self.enable_spam_scanner
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
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
            sendgrid_inbound: None,
            resend_api: ResendApiConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough to pass the startup check, which is itself part of what is under test.
    const TEST_SECRET: &str = "a-test-secret-long-enough-to-sign-with-01";

    #[test]
    fn runtime_thread_stack_is_bounded_and_defaults_to_sixteen_mebibytes() {
        assert_eq!(parse_runtime_thread_stack_bytes(None), 16 * 1024 * 1024);
        assert_eq!(
            parse_runtime_thread_stack_bytes(Some("8388608")),
            8 * 1024 * 1024
        );
    }

    #[test]
    #[should_panic(expected = "RUNTIME_THREAD_STACK_BYTES must be between")]
    fn runtime_thread_stack_below_the_tokio_default_is_rejected() {
        parse_runtime_thread_stack_bytes(Some("1048576"));
    }

    #[test]
    fn smtp_plaintext_local_is_an_explicit_opt_in() {
        assert!(!parse_smtp_allow_plaintext_local(None));
        assert!(parse_smtp_allow_plaintext_local(Some("true")));
    }

    #[test]
    fn attachments_are_namespaced_by_application_domain_by_default() {
        assert_eq!(
            default_attachments_folder("mail.example.com"),
            "mail.example.com/mail-attachments"
        );
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
        assert!(is_local_domain("[::1]"));
        assert!(is_local_domain("[::1]:3001"));
        assert!(is_local_domain(""));

        assert!(!is_local_domain("example.com"));
        assert!(!is_local_domain("mail.example.com"));
    }
}
