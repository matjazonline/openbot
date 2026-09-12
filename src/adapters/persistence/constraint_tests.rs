//! Exercise constraints through writes, including SQL NULL's three-valued logic.

use serde_json::json;
use sqlx::{Acquire, PgConnection, Postgres, Transaction, postgres::PgArguments, query::Query};
use uuid::Uuid;

use super::test_support::test_pool;

struct Fixture {
    company: Uuid,
    channel: Uuid,
    agent: Uuid,
    principal: Uuid,
    task: Uuid,
}

struct JsonColumn {
    update: &'static str,
    constraint: &'static str,
    max_bytes: usize,
}

async fn fixture(connection: &mut PgConnection) -> Fixture {
    let user = Uuid::new_v4();
    let fixture = Fixture {
        company: Uuid::new_v4(),
        channel: Uuid::new_v4(),
        agent: Uuid::new_v4(),
        principal: Uuid::new_v4(),
        task: Uuid::new_v4(),
    };
    let suffix = user.simple().to_string();
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'test')",
    )
    .bind(user)
    .bind(&suffix)
    .bind(format!("{suffix}@example.com"))
    .execute(&mut *connection)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO companies (id, user_id, name, slug) VALUES ($1, $2, 'Constraints', $3)",
    )
    .bind(fixture.company)
    .bind(user)
    .bind(&suffix)
    .execute(&mut *connection)
    .await
    .unwrap();
    let provenance = json!({"actor_type": "system", "actor_id": null, "actor_name": "test"});
    sqlx::query("INSERT INTO channels (id, company_id, name, created_by) VALUES ($1, $2, 'Constraints', $3)")
        .bind(fixture.channel)
        .bind(fixture.company)
        .bind(&provenance)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agents (id, company_id, name, slug, created_by) VALUES ($1, $2, 'Constraints', 'constraints', $3)")
        .bind(fixture.agent)
        .bind(fixture.company)
        .bind(provenance)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals (id, company_id, kind, agent_id, display_label) VALUES ($1, $2, 'agent', $3, 'Constraints')")
        .bind(fixture.principal)
        .bind(fixture.company)
        .bind(fixture.agent)
        .execute(&mut *connection)
        .await
        .unwrap();
    // Completed so this fixture cannot be claimed and does not fence configuration edits.
    sqlx::query("INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type, status, owner_principal_id, owner_principal_kind) VALUES ($1, $2, $3, $1, 'constraint-test', 'completed', $4, 'agent')")
        .bind(fixture.task)
        .bind(fixture.company)
        .bind(fixture.channel)
        .bind(fixture.principal)
        .execute(connection)
        .await
        .unwrap();
    fixture
}

async fn rejects(
    transaction: &mut Transaction<'_, Postgres>,
    query: Query<'_, Postgres, PgArguments>,
    constraint: &str,
) {
    // Each invalid write gets a savepoint: its error must not abort the remaining matrix.
    let mut attempt = transaction.begin().await.unwrap();
    let error = query.execute(&mut *attempt).await.unwrap_err();
    let database_error = error
        .as_database_error()
        .expect("a database constraint rejects the write");
    assert_eq!(database_error.constraint(), Some(constraint), "{error}");
    attempt.rollback().await.unwrap();
}

#[tokio::test]
async fn task_owner_requires_a_complete_pair_and_a_same_company_principal() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut transaction = pool.begin().await.unwrap();
    let local = fixture(&mut transaction).await;
    let other = fixture(&mut transaction).await;
    const UPDATE: &str = "UPDATE background_tasks SET owner_principal_id = $2, owner_principal_kind = $3 WHERE id = $1";
    for (principal, kind) in [
        (Some(local.principal), None),
        (Some(Uuid::new_v4()), None),
        (Some(other.principal), None),
        (None, Some("agent")),
        (None, Some("person")),
        (Some(local.principal), Some("external")),
    ] {
        rejects(
            &mut transaction,
            sqlx::query(UPDATE)
                .bind(local.task)
                .bind(principal)
                .bind(kind),
            "background_tasks_owner_shape_check",
        )
        .await;
    }
    for principal in [Uuid::new_v4(), other.principal] {
        rejects(
            &mut transaction,
            sqlx::query(UPDATE)
                .bind(local.task)
                .bind(principal)
                .bind("agent"),
            "background_tasks_owner_principal_fk",
        )
        .await;
    }
    for principal in [None, Some(local.principal)] {
        let kind = principal.map(|_| "agent");
        sqlx::query(UPDATE)
            .bind(local.task)
            .bind(principal)
            .bind(kind)
            .execute(&mut *transaction)
            .await
            .unwrap();
        let owner: (Option<Uuid>, Option<String>) = sqlx::query_as(
            "SELECT owner_principal_id, owner_principal_kind FROM background_tasks WHERE id = $1",
        )
        .bind(local.task)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(owner, (principal, kind.map(str::to_owned)));
    }
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn outreach_creator_requires_a_complete_pair_and_a_same_company_agent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut transaction = pool.begin().await.unwrap();
    let local = fixture(&mut transaction).await;
    let other = fixture(&mut transaction).await;
    let outreach = Uuid::new_v4();
    sqlx::query("INSERT INTO task_outreaches (id, company_id, task_id, status, required_threshold_percent, expires_at, outreach_key, subject, body) VALUES ($1, $2, $3, 'completed', 100, CURRENT_TIMESTAMP + INTERVAL '1 day', 'constraint-test', 'Test', 'Test')")
        .bind(outreach).bind(local.company).bind(local.task)
        .execute(&mut *transaction).await.unwrap();
    const UPDATE: &str = "UPDATE task_outreaches SET created_by_principal_id = $2, created_by_principal_kind = $3 WHERE id = $1";
    for (principal, kind) in [
        (Some(local.principal), None),
        (Some(Uuid::new_v4()), None),
        (Some(other.principal), None),
        (None, Some("agent")),
        (Some(local.principal), Some("person")),
    ] {
        rejects(
            &mut transaction,
            sqlx::query(UPDATE)
                .bind(outreach)
                .bind(principal)
                .bind(kind),
            "task_outreaches_creator_shape_check",
        )
        .await;
    }
    for principal in [Uuid::new_v4(), other.principal] {
        rejects(
            &mut transaction,
            sqlx::query(UPDATE)
                .bind(outreach)
                .bind(principal)
                .bind("agent"),
            "task_outreaches_creator_fk",
        )
        .await;
    }
    for principal in [Some(local.principal), None] {
        let kind = principal.map(|_| "agent");
        sqlx::query(UPDATE)
            .bind(outreach)
            .bind(principal)
            .bind(kind)
            .execute(&mut *transaction)
            .await
            .unwrap();
        let creator: (Option<Uuid>, Option<String>) = sqlx::query_as(
            "SELECT created_by_principal_id, created_by_principal_kind FROM task_outreaches WHERE id = $1")
            .bind(outreach).fetch_one(&mut *transaction).await.unwrap();
        assert_eq!(creator, (principal, kind.map(str::to_owned)));
    }
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn agent_json_requires_an_explicit_numeric_version_and_preserves_valid_documents() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let mut transaction = pool.begin().await.unwrap();
    let local = fixture(&mut transaction).await;
    for column in [
        JsonColumn {
            update: "UPDATE agents SET config_json = $2 WHERE id = $1",
            constraint: "agents_config_v1_shape_check",
            max_bytes: 65_536,
        },
        JsonColumn {
            update: "UPDATE agents SET native_tool_policy = $2 WHERE id = $1",
            constraint: "agents_native_tool_policy_shape",
            max_bytes: 16_384,
        },
    ] {
        for document in [
            json!({}),
            json!({"version": null}),
            json!({"version": "1"}),
            json!({"version": 1.0}),
            json!({"version": 2}),
            json!({"version": true}),
            json!(null),
            json!([]),
            json!("1"),
            json!(1),
            json!({"version": 1, "oversized": "x".repeat(column.max_bytes)}),
        ] {
            rejects(
                &mut transaction,
                sqlx::query(column.update).bind(local.agent).bind(document),
                column.constraint,
            )
            .await;
        }
        sqlx::query(column.update)
            .bind(local.agent)
            .bind(json!({"version": 1}))
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    let documents: (serde_json::Value, serde_json::Value) =
        sqlx::query_as("SELECT config_json, native_tool_policy FROM agents WHERE id = $1")
            .bind(local.agent)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(documents, (json!({"version": 1}), json!({"version": 1})));
    // An omitted optional configuration remains distinct from an explicit JSON null.
    sqlx::query("UPDATE agents SET config_json = NULL WHERE id = $1")
        .bind(local.agent)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let configuration: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT config_json FROM agents WHERE id = $1")
            .bind(local.agent)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
    assert_eq!(configuration, None);
    transaction.rollback().await.unwrap();
}
