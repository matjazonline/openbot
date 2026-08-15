use std::env;

use time::Duration;

pub struct AppConfig {
    pub jwt_secret: String,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub app_domain_name: String,
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
}

impl AppConfig {
    pub fn from_env() -> Self {
        let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

        let refresh_token_ttl_days: i64 = env::var("REFRESH_TOKEN_TTL_DAYS")
            .unwrap_or("30".to_string())
            .parse()
            .expect("REFRESH_TOKEN_TTL_DAYS must be a valid number");

        let access_token_ttl_secs: i64 = env::var("ACCESS_TOKEN_TTL_SECS")
            .unwrap_or("30".to_string())
            .parse()
            .expect("ACCESS_TOKEN_TTL_SECS must be a valid number");

        let app_domain_name =
            env::var("APP_DOMAIN_NAME").unwrap_or_else(|_| "localhost".to_string());

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
            access_token_ttl: Duration::seconds(access_token_ttl_secs),
            refresh_token_ttl: Duration::days(refresh_token_ttl_days),
            app_domain_name,
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
        }
    }

    pub fn is_spam_scan_enabled(&self) -> bool {
        self.enable_heuristic_scanner || self.enable_spam_scanner || self.enable_llm_spam_guardrail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_from_env() {
        unsafe {
            env::set_var("JWT_SECRET", "test_secret");
            env::set_var("APP_DOMAIN_NAME", "example.com");
        }

        let config = AppConfig::from_env();
        assert_eq!(config.jwt_secret, "test_secret");
        assert_eq!(config.app_domain_name, "example.com");
    }
}
