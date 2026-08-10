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

        let app_domain_name = env::var("APP_DOMAIN_NAME")
            .unwrap_or_else(|_| "localhost".to_string());

        let smtp_host = env::var("SMTP_HOST").unwrap_or_else(|_| "localhost".to_string());
        let smtp_port: u16 = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "1025".to_string())
            .parse()
            .unwrap_or(1025);
        let smtp_username = env::var("SMTP_USERNAME").unwrap_or_default();
        let smtp_password = env::var("SMTP_PASSWORD").unwrap_or_default();
        let smtp_from_address = env::var("SMTP_FROM_ADDRESS").unwrap_or_else(|_| "noreply@localhost".to_string());

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
        }
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
