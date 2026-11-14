use super::level::high_next;
use super::*;
use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
use crate::entities::access_level::AccessLevel;
use crate::services::harness::runs::{LogicalSeed, LogicalSeedPair};

#[path = "agent_history_freeze_fixture.rs"]
mod fixture;
use fixture::*;

fn forged_documents(
    valid: &Value,
    hidden: Value,
    cancelled: Value,
    foreign: Value,
) -> Vec<(&'static str, Value)> {
    let mut cases = Vec::new();
    for (label, metadata) in [
        ("hidden source", hidden),
        ("cancelled source", cancelled),
        ("foreign source", foreign),
    ] {
        let mut document = valid.clone();
        document["history"]["selected"][1] = metadata;
        cases.push((label, document));
    }
    let mut document = valid.clone();
    document["history"]["seed"][0]["input"] = json!({"forged":"input"});
    cases.push(("input", document));
    let mut document = valid.clone();
    document["history"]["seed"][0]["output"] = json!({"forged":"output"});
    cases.push(("output", document));
    let mut document = valid.clone();
    document["history"]["selected"][0]["access_level"] = json!(5);
    cases.push(("stamp", document));
    let mut document = valid.clone();
    document["history"]["selected"][1] = document["history"]["selected"][0].clone();
    document["history"]["seed"][1] = document["history"]["seed"][0].clone();
    cases.push(("duplicate", document));
    let mut document = valid.clone();
    document["history"]["selected"]
        .as_array_mut()
        .unwrap()
        .reverse();
    document["history"]["seed"]
        .as_array_mut()
        .unwrap()
        .reverse();
    cases.push(("order", document));
    let mut document = valid.clone();
    document["history"]["selected"] =
        Value::Array(vec![valid["history"]["selected"][0].clone(); 17]);
    document["history"]["seed"] = Value::Array(vec![valid["history"]["seed"][0].clone(); 17]);
    cases.push(("pair bound", document));
    let mut document = valid.clone();
    document["history"]["seed"][0]["input"] = json!("x".repeat(65536));
    cases.push(("byte bound", document));
    let mut document = valid.clone();
    document["history"]["seed"] = json!({"wrong":"shape"});
    cases.push(("seed shape", document));
    let mut document = valid.clone();
    document["context"] = json!({"forged":"current"});
    cases.push(("current input", document));
    let mut document = valid.clone();
    document["history"]["content_level"] = json!(5);
    cases.push(("classification", document));
    cases
}

#[tokio::test]
async fn agent_history_preparation_native_first_freeze_attests_sources_content_order_and_level() {
    let case = Box::pin(NativeCase::new()).await;
    let foreign_fixture = Fixture::in_database(case.f.s.fixture._db.clone(), "-foreign-seed").await;
    let foreign = Box::pin(HistoryThread::in_fixture(foreign_fixture)).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"foreign\"")]).await;
    register_scripted_agent_base_url(foreign.agent, &llm.base_url);
    let claim = foreign.next().await;
    assert_eq!(execute(&foreign, &claim).await, json!("foreign"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let valid = case
        .target
        .prepared
        .spec(case.target.claim.fence)
        .unwrap()
        .prepared;
    let cases = forged_documents(
        &valid,
        case.metadata(&case.hidden).await,
        case.metadata(&case.target.cancelled).await,
        case.metadata(&claim).await,
    );
    for (label, document) in cases {
        let error = case.write(&document).await.expect_err(label);
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514"),
            "{label}: {error}"
        );
    }
    assert_eq!(
        case.write(&valid).await.unwrap().rows_affected(),
        1,
        "positive first-freeze control"
    );
    assert_eq!(frozen(&case.f, &case.target.claim).await, valid);
    let mut changed = valid.clone();
    changed["history"]["omitted_eligible"] = json!(10);
    assert!(
        case.write(&changed).await.is_err(),
        "immutable after valid first freeze"
    );
    let restored = case
        .f
        .s
        .p()
        .prepare_agent(&case.target.claim, policy())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(
        restored
            .prepared
            .spec(case.target.claim.fence)
            .unwrap()
            .prepared,
        valid
    );
}

#[tokio::test]
async fn agent_history_preparation_seed_mismatch_refuses_checkpoint_and_pruned_sources_replay_exactly()
 {
    let case = Box::pin(NativeCase::new()).await;
    let claim = &case.target.claim;
    let valid = case.target.prepared.spec(claim.fence).unwrap().prepared;
    case.write(&valid).await.unwrap();
    let mut forged = case.target.prepared.spec(claim.fence).unwrap();
    forged.seed = LogicalSeed::new(vec![LogicalSeedPair {
        input: json!("substitute"),
        output: json!(null),
    }])
    .unwrap();
    assert!(
        case.f
            .s
            .p()
            .open_workflow_checkpoint(claim.fence, &forged, policy())
            .await
            .is_err()
    );
    for source in &case.public {
        crate::adapters::persistence::workflow::agent_history_retention_fixture::retire(
            case.f.s.p(),
            source.fence.scope,
        )
        .await;
    }
    let restored = case
        .f
        .s
        .p()
        .prepare_agent(claim, policy())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(restored.prepared.spec(claim.fence).unwrap().prepared, valid);
    let llm = scripted_llm(vec![LlmTurn::text("\"replayed\"")]).await;
    register_scripted_agent_base_url(case.f.agent, &llm.base_url);
    assert_eq!(execute(&case.f, claim).await, json!("replayed"));
    assert_eq!(frozen(&case.f, claim).await, valid);
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_level_hidden_rows_are_filtered_before_candidate_limit_and_omission_count() {
    let case = Box::pin(NativeCase::with_hidden(18)).await;
    let launch = case
        .f
        .s
        .p()
        .prepare_agent(&case.target.claim, policy())
        .await
        .unwrap()
        .expect_ready();
    let history = launch.prepared.frozen_history().unwrap();
    assert_eq!(history.selected.len(), 2);
    assert_eq!(history.omitted_eligible, 0);
    assert_eq!(history.selected_level, AccessLevel::PUBLIC);
    assert_eq!(history.content_level, AccessLevel::PUBLIC);
    assert_eq!(
        history
            .seed
            .pairs()
            .iter()
            .map(|pair| pair.output.clone())
            .collect::<Vec<_>>(),
        vec![json!("public-a"), json!("public-b")]
    );
    assert_eq!(
        history
            .selected
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        case.public
            .iter()
            .map(|claim| AgentExchangeId::for_execution(claim.fence.scope.execution))
            .collect::<Vec<_>>()
    );
    explain_selection(&case).await;
}

async fn new_high_owner(case: &NativeCase) -> ClaimedWorkflow {
    cancel(&case.f, &case.target.claim, "release-unfrozen-target").await;
    high_next(&case.f).await
}

#[tokio::test]
async fn agent_history_level_selection_raises_once_for_competing_preparations_and_replay() {
    let case = Box::pin(NativeCase::new()).await;
    let claim = new_high_owner(&case).await;
    let level: i16 = sqlx::query_scalar(
        "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(claim.fence.scope.company.as_uuid())
    .bind(claim.fence.scope.run.as_uuid())
    .fetch_one(case.f.s.pool())
    .await
    .unwrap();
    assert_eq!(level, 0);
    sqlx::raw_sql("CREATE TABLE history_raise_probe(run_id uuid); CREATE FUNCTION history_raise_probe() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN INSERT INTO history_raise_probe VALUES(NEW.id); RETURN NEW; END $$; CREATE TRIGGER history_raise_probe AFTER UPDATE OF content_level ON workflow_runs FOR EACH ROW EXECUTE FUNCTION history_raise_probe();")
        .execute(case.f.s.pool()).await.unwrap();
    let p = case.f.s.p();
    let (a, b) = tokio::join!(
        p.prepare_agent(&claim, policy()),
        p.prepare_agent(&claim, policy())
    );
    let a = a.unwrap().expect_ready();
    let b = b.unwrap().expect_ready();
    assert_eq!(
        a.prepared.spec(claim.fence).unwrap().prepared,
        b.prepared.spec(claim.fence).unwrap().prepared
    );
    let history = a.prepared.frozen_history().unwrap();
    assert_eq!(history.selected_level, AccessLevel::L5);
    assert_eq!(history.content_level, AccessLevel::L5);
    assert_eq!(history.selected.len(), 3);
    let before = frozen(&case.f, &claim).await;
    p.prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(frozen(&case.f, &claim).await, before);
    let observations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM history_raise_probe WHERE run_id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(case.f.s.pool())
            .await
            .unwrap();
    assert_eq!(
        observations, 1,
        "first freeze raises in one transaction; replay never re-raises"
    );
}

#[tokio::test]
async fn agent_history_preparation_failed_commit_rolls_back_snapshot_selection_taint_and_order() {
    let case = Box::pin(NativeCase::new()).await;
    let claim = new_high_owner(&case).await;
    let before = freeze_state(&case, &claim).await;
    sqlx::raw_sql("CREATE FUNCTION history_freeze_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected frozen preparation commit failure' USING ERRCODE='23514'; END $$; CREATE CONSTRAINT TRIGGER history_freeze_fault AFTER INSERT OR UPDATE ON agent_history_exchanges DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION history_freeze_fault();")
        .execute(case.f.s.pool()).await.unwrap();
    assert!(case.f.s.p().prepare_agent(&claim, policy()).await.is_err());
    assert_eq!(freeze_state(&case, &claim).await, before);
    sqlx::raw_sql("DROP TRIGGER history_freeze_fault ON agent_history_exchanges; DROP FUNCTION history_freeze_fault();")
        .execute(case.f.s.pool()).await.unwrap();
    let launch = case
        .f
        .s
        .p()
        .prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    assert_eq!(
        launch.prepared.frozen_history().unwrap().selected_level,
        AccessLevel::L5
    );
    assert_eq!(freeze_state(&case, &claim).await["content"], 5);
}

async fn freeze_state(case: &NativeCase, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('content',run.content_level,'position',conversation.last_exchange_order,'exchanges',(SELECT count(*) FROM agent_history_exchanges AS exchange WHERE exchange.company_id=run.company_id AND exchange.run_id=run.id),'checkpoints',(SELECT count(*) FROM task_harness_runs AS checkpoint WHERE checkpoint.company_id=run.company_id AND checkpoint.workflow_run_id=run.id)) FROM workflow_runs AS run JOIN agent_history_conversations AS conversation ON conversation.company_id=run.company_id AND conversation.thread_id=run.thread_id AND conversation.agent_id=$3 WHERE run.company_id=$1 AND run.id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.run.as_uuid()).bind(case.f.agent)
        .fetch_one(case.f.s.pool()).await.unwrap()
}

async fn explain_selection(case: &NativeCase) {
    use crate::adapters::persistence::workflow::agent_history::{
        SELECTION_CANDIDATES_SQL, SELECTION_COUNT_SQL,
    };
    let scope = case.target.claim.fence.scope;
    let (conversation,position):(Uuid,i64)=sqlx::query_as("SELECT conversation_id,position FROM agent_history_exchanges WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(case.f.s.pool()).await.unwrap();
    let query = format!("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {SELECTION_CANDIDATES_SQL}");
    let metadata: Value = sqlx::query_scalar(&query)
        .bind(scope.company.as_uuid())
        .bind(conversation)
        .bind(position)
        .bind(0_i16)
        .bind(17_i64)
        .fetch_one(case.f.s.pool())
        .await
        .unwrap();
    assert_eq!(metadata[0]["Plan"]["Actual Rows"].as_f64(), Some(2.0));
    let query = format!("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {SELECTION_COUNT_SQL}");
    let count: Value = sqlx::query_scalar(&query)
        .bind(scope.company.as_uuid())
        .bind(conversation)
        .bind(position)
        .bind(0_i16)
        .fetch_one(case.f.s.pool())
        .await
        .unwrap();
    eprintln!(
        "history_selection_plan: {}",
        json!({"metadata":metadata,"eligible_count":count,"limit":17,"visible_pairs":2,"hidden_pairs":18,"scope":"owned test fixture; count scan not constant-time"})
    );
}

/// The history freeze lowers the reading run when a selected exchange was inexact: the
/// `bool_and` beside `selected_level` (phase 05 E3). The selected exchanges here came from
/// message-triggered runs, which a human message makes inexact.
#[tokio::test]
async fn memory_access_level_history_freeze_lowers_for_inexact_exchanges() {
    use crate::adapters::persistence::workflow::knowledge::exactness_fixture::{exact, mark_exact};
    let case = Box::pin(NativeCase::new()).await;
    let claim = new_high_owner(&case).await;
    let scope = claim.fence.scope;
    let pool = case.f.s.pool();
    mark_exact(pool, scope.company, scope.run).await;
    let launch = case
        .f
        .s
        .p()
        .prepare_agent(&claim, policy())
        .await
        .unwrap()
        .expect_ready();
    let selected: Vec<Uuid> = launch
        .prepared
        .frozen_history()
        .unwrap()
        .selected
        .iter()
        .map(|item| item.id.as_uuid())
        .collect();
    assert!(!selected.is_empty());
    let bits: Vec<bool> = sqlx::query_scalar(
        "SELECT content_exact FROM agent_history_exchanges WHERE company_id=$1 AND id=ANY($2)",
    )
    .bind(scope.company.as_uuid())
    .bind(&selected)
    .fetch_all(pool)
    .await
    .unwrap();
    assert!(bits.iter().all(|bit| !bit), "{bits:?}");
    assert!(!exact(pool, scope.company, scope.run).await);
}
#[tokio::test]
async fn doop_agent_database_first_freeze_binds_current_version_and_trusted_mcp_policy() {
    use crate::application::workflow::agent::McpErrorPolicy;
    for mcp_policy in [None, Some(McpErrorPolicy::FailStep)] {
        let case = Box::pin(NativeCase::with_policy(1, mcp_policy)).await;
        let valid = case
            .target
            .prepared
            .spec(case.target.claim.fence)
            .unwrap()
            .prepared;
        let row_before = case.raw_target().await;
        let state_before = freeze_state(&case, &case.target.claim).await;
        let mut old = valid.clone();
        old["version"] = json!(5);
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("mcp_error_policy");
        let mut invalid = valid.clone();
        invalid["mcp_error_policy"] = json!("ignore");
        let mut changed = valid.clone();
        changed["mcp_error_policy"] = match mcp_policy {
            None => json!("fail_step"),
            Some(_) => json!("observe"),
        };
        for document in [old, missing, invalid, changed] {
            let error = case
                .write(&document)
                .await
                .expect_err("forged first freeze");
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("23514")
            );
            assert_eq!(case.raw_target().await, row_before);
            assert_eq!(freeze_state(&case, &case.target.claim).await, state_before);
        }
        assert_eq!(case.write(&valid).await.unwrap().rows_affected(), 1);
        let restored = case
            .f
            .s
            .p()
            .prepare_agent(&case.target.claim, policy())
            .await
            .unwrap()
            .expect_ready();
        assert_eq!(
            restored
                .prepared
                .spec(case.target.claim.fence)
                .unwrap()
                .prepared,
            valid
        );
    }
}
#[tokio::test]
async fn doop_agent_current_preparation_reaches_ready_before_model_io() {
    let f = Box::pin(HistoryThread::new()).await;
    let claim = f.next().await;
    let launch =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    assert_eq!(
        launch.prepared.spec(claim.fence).unwrap().prepared["version"],
        7
    );
}
