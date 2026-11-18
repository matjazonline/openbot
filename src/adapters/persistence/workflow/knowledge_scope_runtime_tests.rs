use super::*;

#[path = "knowledge_scope_proof_tests.rs"]
mod proofs;

use crate::{
    application::{
        knowledge::{
            MoveKnowledgeDocument,
            search::{KnowledgeChunks, VisibleKnowledgeQuery},
        },
        ports::department::DepartmentWrite,
    },
    entities::department::DepartmentId,
};

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Proof {
    unit_ids: Vec<Uuid>,
    source_items: Value,
    content_level: i16,
    result: Value,
}

async fn proof(f: &SearchFixture) -> Proof {
    let scope = f.claim.fence.scope;
    sqlx::query_as("SELECT queried_unit_ids AS unit_ids,source_items,content_level::smallint AS content_level,result
        FROM workflow_knowledge_consumptions WHERE company_id=$1 AND run_id=$2 AND job_id=$3 ORDER BY attempt_number LIMIT 1")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.job.0).fetch_one(f.p().pool()).await.unwrap()
}

async fn search(
    f: &SearchFixture,
    call: &crate::application::workflow::knowledge::KnowledgeToolCall,
) -> Value {
    let AgentToolObservation::Completed(result) = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, copy_call(call))
        .await
        .unwrap()
    else {
        panic!("local search must complete")
    };
    result
}

async fn forget_read_rows(f: &SearchFixture) {
    // This fixture owns its database. Restore its guard before exercising the real consumer.
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("ALTER TABLE workflow_run_scopes DISABLE TRIGGER workflow_scope_row_guard")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2 AND access='read'",
    )
    .bind(f.claim.fence.scope.company.as_uuid())
    .bind(f.claim.fence.scope.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE workflow_run_scopes ENABLE TRIGGER workflow_scope_row_guard")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let enabled:bool=sqlx::query_scalar("SELECT tgenabled='O' FROM pg_trigger WHERE tgrelid='workflow_run_scopes'::regclass AND tgname='workflow_scope_row_guard'")
        .fetch_one(f.p().pool()).await.unwrap();
    assert!(enabled);
}

#[tokio::test]
async fn knowledge_scope_search_missing_rows_records_only_refusal_without_embedding_or_company_fallback()
 {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 floor"})).await;
    forget_read_rows(&f).await;
    let result = search(&f, &call).await;
    assert_eq!(result["error"]["code"], "knowledge.scope_unavailable");
    assert!(result.get("passages").is_none());
    assert_eq!(f.model.query_count(), 0);
    let saved = proof(&f).await;
    assert!(saved.unit_ids.is_empty());
    assert_eq!(saved.source_items, json!([]));
    assert_eq!(saved.content_level, 0);
    assert_eq!(f.snapshot().await["content"], 0);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_scope_search_empty_success_keeps_queried_units_separate_from_unavailable_scope()
{
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"empty library"})).await;
    assert_eq!(search(&f, &call).await, json!({"passages":[]}));
    let saved = proof(&f).await;
    assert_eq!(saved.unit_ids.len(), 1);
    assert_eq!(saved.source_items, json!([]));
    assert_eq!(saved.content_level, 0);
    assert_eq!(f.model.query_count(), 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_scope_search_real_items_keep_historical_units_after_move_and_fresh_queries_use_current_owner()
 {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 floor"})).await;
    let result = search(&f, &call).await;
    let saved = proof(&f).await;
    assert_eq!(saved.unit_ids.len(), 1);
    assert_eq!(saved.content_level, 5);
    let item_count = saved.source_items.as_array().unwrap().len();
    assert!(item_count > 0);
    assert_eq!(item_count, result["passages"].as_array().unwrap().len());
    let matches:i64=sqlx::query_scalar("SELECT count(*) FROM jsonb_to_recordset($1) AS item(document_id uuid,chunk_id uuid,unit_id uuid,access_level smallint)
        JOIN knowledge_chunks AS chunk ON chunk.id=item.chunk_id AND chunk.document_id=item.document_id
        AND chunk.unit_id=item.unit_id AND chunk.access_level=item.access_level WHERE chunk.company_id=$2")
        .bind(&saved.source_items).bind(f.claim.fence.scope.company.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(matches, item_count as i64);
    let moved = move_documents(&f).await;
    assert_eq!(search(&f, &call).await, result);
    assert_eq!(proof(&f).await, saved);
    assert_eq!(f.model.query_count(), 1);
    assert!(fresh(&f, &saved.unit_ids).await.is_empty());
    assert_eq!(fresh(&f, &[moved.as_uuid()]).await.len(), 2);
    assert_eq!(f.snapshot().await["content"], 5);
    forget_read_rows(&f).await;
    assert!(
        f.knowledge()
            .call_knowledge_tool(f.claim.fence, copy_call(&call))
            .await
            .is_err()
    );
    assert_eq!(proof(&f).await, saved);
    assert_eq!(f.model.query_count(), 1);
    f.library.shutdown().await.unwrap();
}

async fn move_documents(f: &SearchFixture) -> DepartmentId {
    let scope = KnowledgeScope {
        company: f.fixture.target.company,
        actor_id: f.fixture.target.actor.user_id(),
    };
    let target = crate::adapters::persistence::test_support::department_use_cases(f.p())
        .create_department(
            scope.actor_id,
            scope.company.as_uuid(),
            DepartmentWrite {
                name: "Moved".into(),
                slug: "moved".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let layers = f
        .library
        .documents()
        .list(scope, None)
        .await
        .unwrap()
        .layers;
    f.library
        .documents()
        .move_document(MoveKnowledgeDocument {
            scope,
            document_id: layers[0].id,
            expected_revision: layers[0].revision,
            target,
        })
        .await
        .unwrap();
    target
}

async fn fresh(
    f: &SearchFixture,
    units: &[Uuid],
) -> Vec<crate::application::knowledge::search::StoredKnowledgeChunk> {
    f.p()
        .visible(VisibleKnowledgeQuery {
            company: f.fixture.target.company,
            ceiling: AccessLevel::L5,
            model: f.library.embedder().model_id(),
            exact_terms: Vec::new(),
            units: units.iter().copied().map(DepartmentId::new).collect(),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn knowledge_scope_search_cached_model_continuation_requires_available_consumed_scope() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 floor"})).await;
    search(&f, &call).await;
    let before = f.snapshot().await;
    forget_read_rows(&f).await;
    let llm = scripted_llm(Vec::new()).await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let output = Box::pin(
        f.runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &f.claim),
    )
    .await;
    assert!(
        output.completed_result().is_err(),
        "cached knowledge with unavailable authority must not reach the model"
    );
    assert_eq!(llm.finish().await.unwrap(), 0);
    assert_eq!(f.snapshot().await, before);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_scope_search_cached_model_continuation_refuses_deleted_report_owner() {
    let f = Box::pin(SearchFixture::with_lead_desk_tools(
        AccessLevel::L5,
        &[crate::entities::agent_capability::AgentBuiltinTool::SearchKnowledge],
    ))
    .await;
    let report: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND slug='report-a'")
            .bind(f.fixture.target.company.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    let report = DepartmentId::new(report);
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.fixture.target.company.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    let owner = KnowledgeScope {
        company: f.fixture.target.company,
        actor_id: f.fixture.target.actor.user_id(),
    };
    f.library
        .upload(
            crate::application::knowledge::KnowledgeUploadTarget {
                scope: owner,
                unit: report,
            },
            vec![
                KnowledgeUploadFile::from_upload("5_report.md", b"REPORT SECRET".to_vec()).unwrap(),
            ],
        )
        .await
        .unwrap();
    let call = f.prepare(json!({"query":"report secret"})).await;
    let result = search(&f, &call).await;
    assert_eq!(result["passages"][0]["text"], "REPORT SECRET");
    let saved = proof(&f).await;
    assert_eq!(
        saved.source_items[0]["unit_id"],
        report.as_uuid().to_string()
    );
    let layer = f
        .library
        .documents()
        .list(owner, Some(report))
        .await
        .unwrap()
        .layers
        .remove(0);
    f.library
        .documents()
        .move_document(MoveKnowledgeDocument {
            scope: owner,
            document_id: layer.id,
            expected_revision: layer.revision,
            target: DepartmentId::new(root),
        })
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE company_id=$1 AND department_id=$2")
        .bind(owner.company.as_uuid())
        .bind(report.as_uuid())
        .execute(f.p().pool())
        .await
        .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .delete_department(owner.actor_id, owner.company.as_uuid(), report)
        .await
        .unwrap();
    let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_run_scopes WHERE company_id=$1 AND run_id=$2 AND unit_id=$3 AND access='read')")
        .bind(owner.company.as_uuid()).bind(f.claim.fence.scope.run.as_uuid()).bind(report.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert!(
        retained,
        "legitimate owner deletion preserves immutable historical scope IDs"
    );
    let llm = scripted_llm(Vec::new()).await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let output = Box::pin(
        f.runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &f.claim),
    )
    .await;
    assert!(output.completed_result().is_err());
    assert_eq!(llm.finish().await.unwrap(), 0);
    assert_eq!(proof(&f).await, saved);
    f.library.shutdown().await.unwrap();
}
