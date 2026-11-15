//! Native producer proof rejects an agent impersonating a sealed pure-workflow message.
use super::*;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};

async fn publication_effects(p: &PostgresPersistence, company: Uuid) -> Value {
    let mut result = serde_json::Map::new();
    for table in [
        "messages",
        "threads",
        "thread_messages",
        "external_messages",
        "message_deliveries",
        "message_delivery_parts",
        "workflow_message_destinations",
        "workflow_message_producer_seals",
        "workflow_action_intents",
        "workflow_action_dispatches",
        "workflow_action_receipts",
        "workflow_message_actions",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(company)
                .fetch_one(p.pool())
                .await
                .unwrap();
        result.insert(table.into(), count.into());
    }
    result.into()
}

async fn install_sender_probe(p: &PostgresPersistence, run: RunId, principal: Uuid) {
    sqlx::raw_sql(&format!(
        "CREATE FUNCTION reserve_workflow_sender() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         INSERT INTO workflow_message_producer_seals(company_id,message_id,producer_run_id) \
         VALUES(NEW.company_id,NEW.id,'{}'::uuid); RETURN NEW; END $$; \
         CREATE TRIGGER reserve_workflow_sender BEFORE INSERT ON messages FOR EACH ROW \
         EXECUTE FUNCTION reserve_workflow_sender(); \
         CREATE FUNCTION forge_workflow_sender() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
         NEW.author_principal_id='{principal}'::uuid; NEW.authored_identity_id=NULL; NEW.role='agent'; \
         RETURN NEW; END $$; CREATE TRIGGER forge_workflow_sender BEFORE INSERT ON messages \
         FOR EACH ROW EXECUTE FUNCTION forge_workflow_sender();", run.as_uuid()
    )).execute(p.pool()).await.unwrap();
}

#[tokio::test]
async fn relay_identity_native_forged_workflow_agent_author_rolls_back_publication() {
    let f = Box::pin(send(&[To::Channel])).await;
    let p = &f.fixture.persistence;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(p.pool())
        .await
        .unwrap();
    assert!(matches!(
        f.runtime().execute(&f.kind, &f.claim).await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    let company = f.scope().company.as_uuid();
    let source = invocations(f.pool(), f.scope().run).await[0];
    let pretender = AgentPersistence::create(
        p,
        company,
        AgentWrite {
            name: "Unrelated native agent".into(),
            slug: "unrelated-native-agent".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(company)
            .bind(pretender.id)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let before = publication_effects(p, company).await;
    // The SQL-only clone must reserve its explicit producer before INSERT, just as the real
    // writer does. Otherwise it intentionally seals an ordinary message, outside this guard.
    // Corrupt only its author; no native guard is disabled.
    install_sender_probe(p, f.scope().run, principal).await;
    let mut tx = f.pool().begin().await.unwrap();
    let cloned = super::sql_tests::sql_publication(&mut tx, source)
        .await
        .unwrap();
    let forged: bool = sqlx::query_scalar(
        "SELECT message.author_principal_id=$2 AND message.role='agent' AND producer.producer_run_id=$3 \
         FROM workflow_message_destinations AS destination JOIN messages AS message \
           ON message.company_id=destination.company_id AND message.id=destination.message_id \
         JOIN workflow_message_producer_seals AS producer \
           ON producer.company_id=message.company_id AND producer.message_id=message.id \
         WHERE destination.invocation_id=$1",
    ).bind(cloned).bind(principal).bind(f.scope().run.as_uuid()).fetch_one(&mut *tx).await.unwrap();
    assert!(
        forged,
        "the native boundary must see an actual forged author and explicit producer"
    );
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("workflow_message_author_guard")
    );
    assert_eq!(publication_effects(p, company).await, before);
    sqlx::raw_sql(
        "DROP TRIGGER forge_workflow_sender ON messages; DROP FUNCTION forge_workflow_sender();",
    )
    .execute(f.pool())
    .await
    .unwrap();
    let mut tx = f.pool().begin().await.unwrap();
    super::sql_tests::sql_publication(&mut tx, source)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        publication_effects(p, company).await["messages"],
        before["messages"].as_i64().unwrap() + 1
    );
    sqlx::raw_sql("DROP TRIGGER reserve_workflow_sender ON messages; DROP FUNCTION reserve_workflow_sender();")
        .execute(f.pool()).await.unwrap();
}
