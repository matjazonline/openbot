use super::*;
use crate::adapters::persistence::jsonb_test_support::LogCapture;
use crate::adapters::persistence::workflow::agent_history_retention_fixture as clock;
use crate::application::workflow::storage::WorkflowStorageMaintenance;
use tracing::instrument::WithSubscriber;

async fn row(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(p.pool()).await.unwrap()
}

async fn boundary(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('last',conversation.last_exchange_order,
        'expired',conversation.expired_count,'through',conversation.expired_through_order)
        FROM agent_history_conversations AS conversation JOIN agent_history_exchanges AS exchange
        ON exchange.company_id=conversation.company_id AND exchange.conversation_id=conversation.id
        WHERE exchange.company_id=$1 AND exchange.id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn agent_history_retention_competing_collectors_expire_json_null_once_and_keep_checkpoint_proof()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("null")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.complete(json!({"raw":"<script>"}), Value::Null).await;
    let p = f.f.s.p();
    let fresh = clock::candidate(p, claim.fence.scope).await;
    assert!(
        !p.expire_history(fresh).await.unwrap(),
        "fresh sparse stream stays retained"
    );
    let original = row(p, &claim).await;
    assert!(
        original["output_digest"].as_str().is_some(),
        "JSON null is produced output"
    );
    clock::age(p, claim.fence.scope).await;
    let before = row(p, &claim).await;
    let cp: Value = sqlx::query_scalar("SELECT to_jsonb(checkpoint.*) FROM task_harness_runs AS checkpoint WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(original["checkpoint_id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(p.pool()).await.unwrap();
    let candidate = clock::candidate(p, claim.fence.scope).await;
    let logs = LogCapture::default();
    let (first, second) =
        async { tokio::join!(p.expire_history(candidate), p.expire_history(candidate)) }
            .with_subscriber(logs.dispatch())
            .await;
    let events = logs.text();
    assert_eq!(events.matches("agent_history.expired").count(), 1);
    assert!(events.contains(&candidate.company.as_uuid().to_string()));
    assert!(events.contains(&candidate.exchange.as_uuid().to_string()));
    assert!(!events.contains("<script>") && !events.contains("raw"));
    assert_eq!(
        usize::from(first.unwrap()) + usize::from(second.unwrap()),
        1
    );
    let expired = row(p, &claim).await;
    assert_eq!(expired["state"], "completed");
    assert_eq!(expired["payload_retained"], false);
    assert!(!expired["payload_expired_at"].is_null());
    assert_eq!(expired["output"], Value::Null);
    assert_eq!(expired["frozen_preparation"], Value::Null);
    for field in [
        "id",
        "position",
        "input_digest",
        "provenance",
        "access_level",
        "preparation_digest",
        "output_digest",
        "checkpoint_id",
        "finished_at",
    ] {
        assert_eq!(expired[field], before[field], "{field}");
    }
    assert_eq!(
        boundary(p, &claim).await,
        json!({"last":1,"expired":1,"through":1})
    );
    let agreement: bool = sqlx::query_scalar("SELECT agent_history_checkpoint_matches(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(agreement);
    let unchanged: Value = sqlx::query_scalar("SELECT to_jsonb(checkpoint.*) FROM task_harness_runs AS checkpoint WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(cp["id"].as_str().unwrap().parse::<Uuid>().unwrap()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(unchanged, cp, "expiry owns only history payload copies");
    for assignment in [
        "payload_retained=true,payload_expired_at=NULL,output='null'::jsonb",
        "output_digest=sha256('forged'::bytea)",
        "state='failed'",
    ] {
        let sql = format!(
            "UPDATE agent_history_exchanges SET {assignment} WHERE company_id=$1 AND id=$2"
        );
        assert!(
            sqlx::query(&sql)
                .bind(claim.fence.scope.company.as_uuid())
                .bind(claim.fence.scope.execution.as_uuid())
                .execute(p.pool())
                .await
                .is_err()
        );
        assert_eq!(row(p, &claim).await, expired);
    }
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_retention_holes_and_source_cascade_keep_truthful_monotone_boundary() {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(
        (0..3)
            .map(|index| LlmTurn::text(format!("{index}")))
            .collect(),
    )
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let mut claims = Vec::new();
    for index in 0..3 {
        claims.push(f.complete(json!(index), json!(index)).await);
    }
    let p = f.f.s.p();
    clock::expire(p, claims[1].fence.scope).await;
    assert_eq!(
        boundary(p, &claims[2]).await,
        json!({"last":3,"expired":1,"through":0})
    );
    clock::expire(p, claims[0].fence.scope).await;
    assert_eq!(
        boundary(p, &claims[2]).await,
        json!({"last":3,"expired":2,"through":2})
    );
    clock::retire_source(p, claims[0].fence.scope).await;
    assert_eq!(
        boundary(p, &claims[2]).await,
        json!({"last":3,"expired":2,"through":2})
    );
    for assignment in [
        "expired_count=0",
        "expired_through_order=3",
        "last_exchange_order=4",
    ] {
        let sql =
            format!("UPDATE agent_history_conversations SET {assignment} WHERE company_id=$1");
        assert!(
            sqlx::query(&sql)
                .bind(claims[2].fence.scope.company.as_uuid())
                .execute(p.pool())
                .await
                .is_err()
        );
    }
    clock::expire(p, claims[2].fence.scope).await;
    assert_eq!(
        boundary(p, &claims[2]).await,
        json!({"last":3,"expired":3,"through":3})
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn agent_history_retention_rollback_and_deferred_abort_preserve_payload_digests_and_boundary()
{
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("{\"kept\":true}")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.complete(json!([null, 1]), json!({"kept":true})).await;
    let p = f.f.s.p();
    clock::age(p, claim.fence.scope).await;
    let before = row(p, &claim).await;
    let original_boundary = boundary(p, &claim).await;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(claim.fence.scope.company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let count = sqlx::query("UPDATE agent_history_exchanges SET payload_retained=false,payload_expired_at=clock_timestamp(),output=NULL,frozen_preparation=NULL WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid()).execute(&mut *tx).await.unwrap().rows_affected();
    assert_eq!(count, 1);
    tx.rollback().await.unwrap();
    assert_eq!(row(p, &claim).await, before);
    assert_eq!(boundary(p, &claim).await, original_boundary);
    sqlx::raw_sql("CREATE FUNCTION ah_expiry_abort() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced deferred expiry abort'; END $$;
        CREATE CONSTRAINT TRIGGER ah_expiry_abort AFTER UPDATE ON agent_history_exchanges DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ah_expiry_abort()")
        .execute(p.pool()).await.unwrap();
    let aborted_logs = LogCapture::default();
    assert!(
        p.expire_history(clock::candidate(p, claim.fence.scope).await)
            .with_subscriber(aborted_logs.dispatch())
            .await
            .is_err()
    );
    assert!(
        !aborted_logs.text().contains("agent_history.expired"),
        "a deferred abort must never emit a committed expiry event"
    );
    assert_eq!(row(p, &claim).await, before);
    assert_eq!(boundary(p, &claim).await, original_boundary);
    sqlx::raw_sql(
        "DROP TRIGGER ah_expiry_abort ON agent_history_exchanges; DROP FUNCTION ah_expiry_abort()",
    )
    .execute(p.pool())
    .await
    .unwrap();
    assert!(
        p.expire_history(clock::candidate(p, claim.fence.scope).await)
            .await
            .unwrap()
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_retention_direct_expiry_refuses_reverse_source_lock_without_deadlock_or_partial_boundary()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"retained\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.complete(json!("source-lock"), json!("retained")).await;
    let p = f.f.s.p();
    clock::age(p, claim.fence.scope).await;
    let before = row(p, &claim).await;
    let old_boundary = boundary(p, &claim).await;
    let mut source = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(claim.fence.scope.company.as_uuid())
        .bind(claim.fence.scope.run.as_uuid())
        .execute(&mut *source)
        .await
        .unwrap();
    let error=tokio::time::timeout(Duration::from_secs(1),sqlx::query("UPDATE agent_history_exchanges SET payload_retained=false,payload_expired_at=clock_timestamp(),output=NULL,frozen_preparation=NULL WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid()).execute(p.pool())).await.unwrap().unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    assert_eq!(row(p, &claim).await, before);
    assert_eq!(boundary(p, &claim).await, old_boundary);
    source.rollback().await.unwrap();
    assert!(
        p.expire_history(clock::candidate(p, claim.fence.scope).await)
            .await
            .unwrap()
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_retention_lifecycle_events_are_structured_and_never_contain_the_native_payload_or_credential()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text(r#"{"marker":"SECRET-HISTORY-OUTPUT"}"#)]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let logs = LogCapture::default();
    f.complete(
        json!({"marker":"SECRET-HISTORY-INPUT"}),
        json!({"marker":"SECRET-HISTORY-OUTPUT"}),
    )
    .with_subscriber(logs.dispatch())
    .await;
    let active = f.start(json!("SECRET-CURRENT-INPUT")).await;
    for _ in 0..2 {
        f.f.s
            .p()
            .prepare_agent(&active, policy())
            .with_subscriber(logs.dispatch())
            .await
            .unwrap()
            .expect_ready();
    }
    let events = logs.text();
    for event in [
        "agent_history.queued",
        "agent_history.acquired",
        "agent_history.selected",
        "agent_history.prepared",
        "agent_history.recovered",
        "agent_history.completed",
        "agent_history.released",
        "workflow.checkpoint_verified",
    ] {
        assert!(
            events.contains(event),
            "actual native lifecycle must emit {event}"
        );
    }
    assert!(events.contains("durability=\"staged\"") && events.contains("durability=\"verified\""));
    for secret in [
        "SECRET-HISTORY-INPUT",
        "SECRET-HISTORY-OUTPUT",
        "SECRET-CURRENT-INPUT",
        "fixture-key",
    ] {
        assert!(
            !events.contains(secret),
            "history diagnostics leaked {secret}"
        );
    }
    assert_eq!(llm.finish().await.unwrap(), 1);
}
