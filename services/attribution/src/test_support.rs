use std::{str::FromStr, time::Duration};

use sqlx::{
    ConnectOptions, Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use uuid::Uuid;

use crate::infra::config::Config;

pub(crate) struct Database {
    pub pool: PgPool,
    pub options: PgConnectOptions,
    resource: OwnedDatabase,
}

struct OwnedDatabase {
    name: String,
    admin: PgConnectOptions,
    created: bool,
}

fn admin_options() -> PgConnectOptions {
    let url = std::env::var("ATTRIBUTION_TEST_ADMIN_URL").expect(
        "ATTRIBUTION_TEST_ADMIN_URL must name a PostgreSQL maintenance database; tests create/drop only as_test_* databases and roles. Missing databases are never skipped.",
    );
    let options = PgConnectOptions::from_str(&url)
        .expect("valid test maintenance URL")
        .disable_statement_logging();
    assert_eq!(
        options.get_database(),
        Some("postgres"),
        "test maintenance database must be postgres"
    );
    options
}

impl Database {
    pub async fn new() -> Self {
        let admin = admin_options();
        let mut connection =
            tokio::time::timeout(Duration::from_secs(5), PgConnection::connect_with(&admin))
                .await
                .expect("maintenance connect deadline")
                .expect("maintenance database available");
        let name = format!("as_test_{}", Uuid::new_v4().simple());
        let password = Uuid::new_v4().simple().to_string();
        // Identifiers/password here are generated ASCII hex, never external SQL input.
        sqlx::query(&format!("CREATE ROLE \"{name}\" LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS"))
            .execute(&mut connection).await.expect("create owned test role");
        let mut resource = OwnedDatabase {
            name: name.clone(),
            admin: admin.clone(),
            created: false,
        };
        sqlx::query(&format!("CREATE DATABASE \"{name}\" OWNER \"{name}\""))
            .execute(&mut connection)
            .await
            .expect("create owned test database");
        resource.created = true;
        sqlx::query(&format!(
            "REVOKE CONNECT ON DATABASE \"{name}\" FROM PUBLIC"
        ))
        .execute(&mut connection)
        .await
        .expect("deny other roles");
        let options = admin
            .database(&name)
            .username(&name)
            .password(&password)
            .options([("statement_timeout", "5000"), ("lock_timeout", "2000")]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .acquire_timeout(Duration::from_secs(2))
            .connect_with(options.clone())
            .await
            .expect("owned database connects");
        Self {
            pool,
            options,
            resource,
        }
    }

    pub fn config(&self) -> Config {
        let mut config = Config::from_lookup(|key| match key {
            "ATTRIBUTION_DATABASE_URL" => Some("postgres://unused@localhost/unused".into()),
            "ATTRIBUTION_PUBLIC_ORIGIN" => Some("https://attribution.example.test".into()),
            "ATTRIBUTION_EVENT_MAX_AGE_DAYS" => Some("7".into()),
            "ATTRIBUTION_REFUND_WINDOW_DAYS" => Some("365".into()),
            "ATTRIBUTION_CLICK_RETENTION_DAYS" => Some("38".into()),
            "ATTRIBUTION_OBSERVATION_RETENTION_DAYS" => Some("30".into()),
            "ATTRIBUTION_FINANCIAL_RETENTION_DAYS" => Some("400".into()),
            "ATTRIBUTION_EVENT_RETENTION_DAYS" => Some("8".into()),
            "ATTRIBUTION_AUDIT_RETENTION_DAYS" => Some("90".into()),
            _ => None,
        })
        .unwrap();
        config.database = self.options.clone();
        config
    }

    pub async fn admin(&self) -> PgConnection {
        PgConnection::connect_with(&self.resource.admin.clone().database(&self.resource.name))
            .await
            .expect("fixture admin connects")
    }
}

impl Drop for OwnedDatabase {
    fn drop(&mut self) {
        let name = self.name.clone();
        let admin = self.admin.clone();
        let created = self.created;
        // Own thread/runtime allows cleanup during panic or inside a Tokio test.
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("cleanup runtime");
            runtime.block_on(async move {
                let cleanup = async {
                    let mut connection = PgConnection::connect_with(&admin).await?;
                    if created {
                        sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                            .execute(&mut connection)
                            .await?;
                    }
                    sqlx::query(&format!("DROP ROLE IF EXISTS \"{name}\""))
                        .execute(&mut connection)
                        .await?;
                    Ok::<_, sqlx::Error>(())
                };
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(10), cleanup).await,
                    Ok(Ok(()))
                ) {
                    eprintln!("owned fixture cleanup failed for {name}");
                }
            });
        })
        .join();
    }
}

pub(crate) fn identity_http(
    pool: sqlx::PgPool,
    origin: &url::Url,
) -> crate::adapters::identity::http::IdentityHttp {
    use base64::Engine;
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32]);
    let keys =
        serde_json::json!({"active": 1, "keys": [{"version": 1, "key": secret}]}).to_string();
    let config = crate::infra::identity::IdentityConfig::from_lookup(origin, |name| match name {
        "ATTRIBUTION_OPENBOT_ORIGIN" => Some("http://127.0.0.1:8092".into()),
        "ATTRIBUTION_CLIENT_SECRET" => {
            Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([8u8; 32]))
        }
        "ATTRIBUTION_LIFECYCLE_SECRET" => Some(secret.clone()),
        "ATTRIBUTION_SESSION_KEYS" => Some(keys.clone()),
        _ => None,
    })
    .unwrap();
    crate::infra::identity::compose(pool, std::sync::Arc::new(config)).unwrap()
}
