//! DB-backed `memory_startup_` and `memory_boundary_` cases. Require MEMORY_TEST_DATABASE_URL.
use super::{
    MIGRATOR,
    test_support::{admin_sql, drop_database, empty_database, test_db},
};
use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};

#[tokio::test]
async fn memory_startup_migration_without_pgvector_fails_explicitly() {
    let (name, options) = empty_database(false).await;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let error = MIGRATOR.run(&pool).await.unwrap_err().to_string();
    pool.close().await;
    drop_database(&name);
    assert!(error.contains("pgvector is not installed"), "{error}");
}

#[tokio::test]
async fn memory_startup_migration_applies_as_non_superuser_owner_and_is_idempotent() {
    let db = test_db().await;
    let superuser: bool =
        sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname = current_user")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert!(
        !superuser,
        "tests must exercise the production owner-role path"
    );
    MIGRATOR.run(db.pool()).await.unwrap();
    // Level and tenant-consistency constraints are live.
    let tenant = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO tenants (id, external_id) VALUES ($1, 'x')")
        .bind(tenant)
        .execute(db.pool())
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO tenants (id, external_id) VALUES ($1, 'bad/id')")
            .bind(uuid::Uuid::now_v7())
            .execute(db.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT 10::access_level")
            .execute(db.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT 9::access_level")
            .execute(db.pool())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn memory_boundary_separate_database_roles_cannot_cross() {
    let db = test_db().await;
    let outsider = format!("{}_other", db.name);
    let password = format!("{:x}", uuid::Uuid::new_v4().as_u128());
    admin_sql(&format!(
        "CREATE ROLE \"{outsider}\" LOGIN PASSWORD '{password}'"
    ))
    .await;
    let as_outsider = db
        .owner_options
        .clone()
        .username(&outsider)
        .password(&password);
    // PUBLIC has no CONNECT on a service database: another application's role is refused.
    let refused = PgConnection::connect_with(&as_outsider)
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("permission denied"), "{refused}");
    // Even with CONNECT, it cannot read the service's tables.
    admin_sql(&format!(
        "GRANT CONNECT ON DATABASE \"{}\" TO \"{outsider}\"",
        db.name
    ))
    .await;
    let mut conn = PgConnection::connect_with(&as_outsider).await.unwrap();
    let denied = sqlx::query("SELECT count(*) FROM tenants")
        .execute(&mut conn)
        .await
        .unwrap_err()
        .to_string();
    assert!(denied.contains("permission denied"), "{denied}");
    conn.close().await.unwrap();
    // Positive control: the owner role reads its own tables.
    assert!(
        sqlx::query("SELECT count(*) FROM tenants")
            .execute(db.pool())
            .await
            .is_ok()
    );
    admin_sql(&format!(
        "REVOKE CONNECT ON DATABASE \"{}\" FROM \"{outsider}\"",
        db.name
    ))
    .await;
    db.pool().close().await;
    drop_database(&db.name);
    admin_sql(&format!("DROP ROLE IF EXISTS \"{outsider}\"")).await;
}

/// m4: a source revision is frozen apart from the retention null-out, and a source link must
/// carry its source's real level.
#[tokio::test]
async fn memory_storage_sources_are_frozen_and_links_carry_the_source_level() {
    let db = test_db().await;
    let pool = db.pool();
    let run = |sql: &'static str| async move { sqlx::query(sql).execute(pool).await };
    run("INSERT INTO tenants (id, external_id) VALUES ('00000000-0000-7000-8000-000000000001', 't')")
        .await
        .unwrap();
    run(
        "INSERT INTO namespaces (tenant_id, id, external_id) VALUES \
         ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', 'ns')",
    )
    .await
    .unwrap();
    run(
        "INSERT INTO sessions (tenant_id, namespace_id, id, external_id) VALUES \
         ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', \
          '00000000-0000-7000-8000-000000000003', 's1'), \
         ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', \
          '00000000-0000-7000-8000-000000000004', 's2')",
    )
    .await
    .unwrap();
    run(
        "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, access_level, \
         session_id, content, content_sha256, occurred_at, expires_at) VALUES \
         ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', \
          '00000000-0000-7000-8000-000000000005', 'doc:1', '1', 5, \
          '00000000-0000-7000-8000-000000000003', 'text', sha256('text'), now(), \
          now() + interval '30 days')",
    )
    .await
    .unwrap();
    for (what, sql) in [
        (
            "re-homing to another session",
            "UPDATE sources SET session_id = '00000000-0000-7000-8000-000000000004'",
        ),
        (
            "changing the redaction version",
            "UPDATE sources SET redaction_version = 2",
        ),
        (
            "moving the expiry",
            "UPDATE sources SET expires_at = now() + interval '90 days'",
        ),
        ("relabelling", "UPDATE sources SET access_level = 1"),
        ("rewriting content", "UPDATE sources SET content = 'other'"),
    ] {
        let error = run(sql).await.unwrap_err().to_string();
        assert!(error.contains("immutable"), "{what}: {error}");
    }
    run("UPDATE sources SET content = NULL, content_nulled_at = now()")
        .await
        .expect("retention may null the content");
    run("INSERT INTO observations (tenant_id, namespace_id, id, kind, title, body, access_level, \
         producing_ceiling, watermark_kind, watermark_level, source_max_level, origin, occurred_at) \
         VALUES ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000002', \
          '00000000-0000-7000-8000-000000000006', 'fact', 't', 'b', 5, 6, 'exact', 5, 5, \
          'explicit', now())")
        .await
        .unwrap();
    let under = run(
        "INSERT INTO observation_links (tenant_id, observation_id, ordinal, source_id, source_level) \
         VALUES ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000006', 0, \
          '00000000-0000-7000-8000-000000000005', 2)",
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        under.contains("foreign key"),
        "an under-labelled link: {under}"
    );
    run("INSERT INTO observation_links (tenant_id, observation_id, ordinal, source_id, source_level) \
         VALUES ('00000000-0000-7000-8000-000000000001', '00000000-0000-7000-8000-000000000006', 0, \
          '00000000-0000-7000-8000-000000000005', 5)")
        .await
        .expect("a link at the source's real level");
    // Audit rows can carry the request id (D2).
    run("INSERT INTO audit_events (id, actor_kind, action, target_kind, target_id, outcome, \
         request_id) VALUES ('00000000-0000-7000-8000-000000000007', 'system', 'x', 'y', 'z', 'ok', \
         '00000000-0000-7000-8000-000000000008')")
        .await
        .unwrap();
}
