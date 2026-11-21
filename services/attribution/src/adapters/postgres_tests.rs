use sqlx::{Connection, PgConnection};

use super::*;
use crate::test_support::Database;

#[tokio::test]
async fn attribution_storage_boundary_fresh_nonprivileged_database_migrates() {
    let db = Database::new().await;
    validate_database(&db.pool).await.unwrap();
    assert!(!ready(&db.pool).await);
    migrate(&db.pool).await.unwrap();
    validate_database(&db.pool).await.unwrap();
    assert!(ready(&db.pool).await);
    migrate(&db.pool).await.unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(applied, 1);
}

#[tokio::test]
async fn attribution_storage_boundary_other_role_cannot_connect_or_read() {
    let attribution = Database::new().await;
    let other = Database::new().await;
    migrate(&attribution.pool).await.unwrap();
    migrate(&other.pool).await.unwrap();
    for (target, outsider) in [(&attribution, &other), (&other, &attribution)] {
        let options = outsider
            .options
            .clone()
            .database(target.options.get_database().unwrap());
        assert!(
            PgConnection::connect_with(&options).await.is_err(),
            "other application's role cannot connect"
        );
        let mut admin = target.admin().await;
        sqlx::query(&format!(
            "GRANT CONNECT ON DATABASE \"{}\" TO \"{}\"",
            target.options.get_database().unwrap(),
            outsider.options.get_username()
        ))
        .execute(&mut admin)
        .await
        .unwrap();
        let mut connection = PgConnection::connect_with(&options).await.unwrap();
        let error = sqlx::query("SELECT application FROM attribution_schema_identity")
            .fetch_all(&mut connection)
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
        connection.close().await.unwrap();
        sqlx::query(&format!(
            "REVOKE CONNECT ON DATABASE \"{}\" FROM \"{}\"",
            target.options.get_database().unwrap(),
            outsider.options.get_username()
        ))
        .execute(&mut admin)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn attribution_startup_refuses_foreign_schema_and_privileged_role() {
    let db = Database::new().await;
    sqlx::query("CREATE TABLE foreign_application_data (id BIGINT)")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        validate_database(&db.pool)
            .await
            .unwrap_err()
            .to_string()
            .contains("another application's")
    );
    sqlx::query("DROP TABLE foreign_application_data")
        .execute(&db.pool)
        .await
        .unwrap();
    let mut admin = db.admin().await;
    sqlx::query(&format!(
        "ALTER ROLE \"{}\" CREATEDB",
        db.options.get_username()
    ))
    .execute(&mut admin)
    .await
    .unwrap();
    assert!(
        validate_database(&db.pool)
            .await
            .unwrap_err()
            .to_string()
            .contains("nonprivileged")
    );
    sqlx::query(&format!(
        "ALTER ROLE \"{}\" NOCREATEDB",
        db.options.get_username()
    ))
    .execute(&mut admin)
    .await
    .unwrap();
    validate_database(&db.pool).await.unwrap();
}

#[tokio::test]
async fn attribution_startup_migration_checksum_failure_is_not_ready_success() {
    let db = Database::new().await;
    migrate(&db.pool).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = decode('00','hex')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(migrate(&db.pool).await.is_err());
    // Startup must abort before binding; readiness alone is not migration validation.
    assert!(
        crate::infra::startup::serve(db.config(), tokio_util::sync::CancellationToken::new())
            .await
            .is_err()
    );
}
