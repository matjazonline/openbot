use std::time::Duration;

use anyhow::{Result, bail};
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::infra::config::Config;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub type Tx = sqlx::Transaction<'static, sqlx::Postgres>;

pub(crate) fn failure(error: sqlx::Error) -> crate::application::identity::Failure {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        crate::application::identity::Failure::Conflict
    } else {
        crate::application::identity::Failure::Unavailable
    }
}

pub async fn connect(config: &Config) -> Result<PgPool> {
    let connection = PgPoolOptions::new()
        .max_connections(config.pool_size)
        .acquire_timeout(Duration::from_secs(2))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(config.database.clone());
    let pool = tokio::time::timeout(Duration::from_secs(5), connection)
        .await
        .map_err(|_| anyhow::anyhow!("database connection timed out"))?
        .map_err(|_| anyhow::anyhow!("database connection failed"))?;
    validate_database(&pool).await?;
    Ok(pool)
}

pub async fn validate_database(pool: &PgPool) -> Result<()> {
    let dedicated: bool = sqlx::query_scalar(
        "SELECT NOT (role.rolsuper OR role.rolcreatedb OR role.rolcreaterole OR role.rolreplication OR role.rolbypassrls) AND database.datdba = role.oid FROM pg_roles AS role JOIN pg_database AS database ON database.datname = current_database() WHERE role.rolname = current_user",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| anyhow::anyhow!("database ownership check failed"))?;
    if !dedicated {
        bail!(
            "database role must be its nonprivileged owner (no SUPERUSER, CREATEDB, CREATEROLE, REPLICATION or BYPASSRLS)"
        );
    }
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename::text FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename LIMIT 1025",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| anyhow::anyhow!("database schema check failed"))?;
    if tables.len() > 1024 {
        bail!("database schema exceeds the supported table bound");
    }
    if tables
        .iter()
        .any(|table| table == "attribution_schema_identity")
    {
        let application = identity(pool)
            .await
            .map_err(|_| anyhow::anyhow!("database application identity check unavailable"))?;
        if application.as_deref() != Some("attribution_service") {
            bail!("database application identity does not match attribution_service");
        }
    } else if tables.iter().any(|table| table != "_sqlx_migrations") {
        bail!("refusing to initialize a database containing another application's tables");
    }
    Ok(())
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|_| anyhow::anyhow!("attribution migration failed; incompatible schemas require an operator-managed database reset"))
}

pub async fn ready(pool: &PgPool) -> bool {
    matches!(
        identity(pool).await,
        Ok(Some(application)) if application == "attribution_service"
    )
}

fn identity(pool: &PgPool) -> impl Future<Output = Result<Option<String>, sqlx::Error>> + '_ {
    sqlx::query_scalar!(
        "SELECT application FROM attribution_schema_identity WHERE singleton = TRUE"
    )
    .fetch_optional(pool)
}

#[cfg(test)]
#[path = "postgres_tests.rs"]
mod tests;
