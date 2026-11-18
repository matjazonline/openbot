//! Trust-boundary negatives use a real queried observation and the actual SQL guard.
use super::*;
use crate::application::{
    knowledge::search::{KnowledgeObservation, rank_chunks},
    workflow::knowledge::{KnowledgeSearchContext, KnowledgeSearchObservation},
};

async fn observed(
    f: &SearchFixture,
    call: &crate::application::workflow::knowledge::KnowledgeToolCall,
) -> KnowledgeSearchObservation {
    let KnowledgeSearchContext::Pending { access, units } =
        f.p().knowledge_context(f.claim.fence, call).await.unwrap()
    else {
        panic!("pending available scope")
    };
    let query = crate::application::knowledge::search::KnowledgeSearchQuery::parse(&call.arguments)
        .unwrap();
    let embedding = f.library.embed_query(query.question()).await.unwrap();
    let chunks = f
        .p()
        .visible(VisibleKnowledgeQuery {
            company: access.company,
            ceiling: access.ceiling,
            units: units.clone(),
            model: embedding.model().clone(),
            exact_terms: query.exact_terms(),
        })
        .await
        .unwrap();
    let observation = rank_chunks(
        chunks,
        embedding.vector(),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    assert!(!observation.items().is_empty());
    KnowledgeSearchObservation {
        queried_units: units,
        observation,
    }
}

#[tokio::test]
async fn knowledge_scope_search_observation_refuses_foreign_subset_item_and_level_without_partial_effects()
 {
    let f = Box::pin(SearchFixture::with_lead_desk_tools(
        AccessLevel::L5,
        &[crate::entities::agent_capability::AgentBuiltinTool::SearchKnowledge],
    ))
    .await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 floor"})).await;
    let real = observed(&f, &call).await;
    let before = f.snapshot().await;
    let result = real.observation.result().clone();
    for rejected in rejected_observations(&real) {
        assert!(
            f.p()
                .observe_knowledge(f.claim.fence, &call, rejected)
                .await
                .is_err()
        );
        assert_eq!(f.snapshot().await, before);
    }
    assert_eq!(
        f.p()
            .observe_knowledge(f.claim.fence, &call, real)
            .await
            .unwrap(),
        result
    );
    assert_eq!(search(&f, &call).await, result);
    assert_eq!(
        f.model.query_count(),
        1,
        "recorded result replays without another embedding"
    );
    let saved = proof(&f).await;
    assert!(!saved.source_items.as_array().unwrap().is_empty());
    check_sql_guard(&f).await;
    check_sql_shape(&f, &saved).await;
    f.library.shutdown().await.unwrap();
}

fn rejected_observations(real: &KnowledgeSearchObservation) -> Vec<KnowledgeSearchObservation> {
    let result = real.observation.result().clone();
    let items = real.observation.items().to_vec();
    let foreign = DepartmentId::new(Uuid::new_v4());
    let mut subset = real.queried_units.clone();
    subset.retain(|unit| items.iter().any(|item| item.unit_id == *unit));
    assert!(subset.len() < real.queried_units.len());
    let mut wrong_item = items.clone();
    wrong_item[0].unit_id = foreign;
    vec![
        KnowledgeSearchObservation {
            queried_units: vec![foreign],
            observation: KnowledgeObservation::new(
                result.clone(),
                real.observation.highest(),
                items.clone(),
            ),
        },
        KnowledgeSearchObservation {
            queried_units: subset,
            observation: KnowledgeObservation::new(
                result.clone(),
                real.observation.highest(),
                items.clone(),
            ),
        },
        KnowledgeSearchObservation {
            queried_units: real.queried_units.clone(),
            observation: KnowledgeObservation::new(
                result.clone(),
                real.observation.highest(),
                wrong_item,
            ),
        },
        KnowledgeSearchObservation {
            queried_units: real.queried_units.clone(),
            observation: KnowledgeObservation::new(result.clone(), Some(AccessLevel::L9), items),
        },
        KnowledgeSearchObservation {
            queried_units: real.queried_units.clone(),
            observation: KnowledgeObservation::new(result.clone(), None, vec![]),
        },
    ]
}

async fn check_sql_guard(f: &SearchFixture) {
    let scope = f.claim.fence.scope;
    let row:Value=sqlx::query_scalar("SELECT to_jsonb(proof) FROM workflow_knowledge_consumptions AS proof WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    let mut altered = row.clone();
    let used: Vec<_> = row["source_items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["unit_id"].clone())
        .collect();
    altered["queried_unit_ids"]
        .as_array_mut()
        .unwrap()
        .retain(|unit| used.contains(unit));
    assert_ne!(altered["queried_unit_ids"], row["queried_unit_ids"]);
    let mut tx = f.p().pool().begin().await.unwrap();
    // Owned fault setup only: put back the guard before exercising the forged direct write.
    sqlx::query("ALTER TABLE workflow_knowledge_consumptions DISABLE TRIGGER workflow_knowledge_consumption_immutable")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM workflow_knowledge_consumptions WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE workflow_knowledge_consumptions ENABLE TRIGGER workflow_knowledge_consumption_immutable")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_knowledge_consumptions
        (company_id,run_id,execution_id,job_id,attempt_number,execution_generation,worker_id,checkpoint_id,invocation_id,content_level,result,queried_unit_ids,source_items)
        SELECT proof.company_id,proof.run_id,proof.execution_id,proof.job_id,proof.attempt_number,proof.execution_generation,proof.worker_id,proof.checkpoint_id,proof.invocation_id,proof.content_level,proof.result,proof.queried_unit_ids,proof.source_items
        FROM jsonb_populate_record(NULL::workflow_knowledge_consumptions,$1) AS proof")
        .bind(altered).execute(&mut *tx).await.unwrap();
    let error = sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:knowledge_consumption_live")
    );
    tx.rollback().await.unwrap();
    let retained:Value=sqlx::query_scalar("SELECT to_jsonb(proof) FROM workflow_knowledge_consumptions AS proof WHERE company_id=$1 AND run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(
        retained, row,
        "failed direct SQL leaves the actual proof intact"
    );
}

async fn check_sql_shape(f: &SearchFixture, saved: &Proof) {
    let check = |items: Value, units: Vec<Uuid>, level: i16| async move {
        sqlx::query_scalar::<_, bool>("SELECT knowledge_source_items_valid($1,$2,$3,$4)")
            .bind(&saved.result)
            .bind(items)
            .bind(units)
            .bind(level)
            .fetch_one(f.p().pool())
            .await
            .unwrap()
    };
    assert!(
        check(
            saved.source_items.clone(),
            saved.unit_ids.clone(),
            saved.content_level
        )
        .await
    );
    let mut foreign = saved.source_items.clone();
    foreign[0]["unit_id"] = json!(Uuid::new_v4());
    let mut malformed = saved.source_items.clone();
    malformed[0]["chunk_id"] = json!("invalid UUID");
    let mut extra = saved.source_items.clone();
    extra[0]["extra"] = json!(true);
    for items in [
        foreign,
        malformed,
        extra,
        json!([]),
        json!({"invalid":"object"}),
    ] {
        assert!(!check(items, saved.unit_ids.clone(), saved.content_level).await);
    }
    let mut duplicate = saved.unit_ids.clone();
    duplicate.push(saved.unit_ids[0]);
    assert!(!check(saved.source_items.clone(), duplicate, saved.content_level).await);
    assert!(!check(saved.source_items.clone(), saved.unit_ids.clone(), 9).await);
    let multidimensional: bool =
        sqlx::query_scalar("SELECT knowledge_source_items_valid($1,$2,ARRAY[ARRAY[$3::uuid]],$4)")
            .bind(&saved.result)
            .bind(&saved.source_items)
            .bind(saved.unit_ids[0])
            .bind(saved.content_level)
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert!(!multidimensional);
}
