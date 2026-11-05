use super::*;
use crate::{application::knowledge::search::*, entities::access_level::AccessLevel};
use serde_json::{Value, json};

// This tests the internal SQL port. Workflow tests separately prove the server-origin ceiling.
pub(super) fn request(
    company: CompanyId,
    ceiling: AccessLevel,
    input: Value,
) -> VisibleKnowledgeQuery {
    VisibleKnowledgeQuery {
        company,
        ceiling,
        model: "test-model".into(),
        exact_terms: KnowledgeSearchQuery::parse(&input).unwrap().exact_terms(),
    }
}

fn vector(x: f32, y: f32) -> [f32; 384] {
    let mut vector = [0.0; 384];
    vector[0] = x;
    vector[1] = y;
    vector
}

#[tokio::test]
async fn knowledge_search_sql_filters_company_model_and_ceiling_before_both_ranks() {
    let database = own_database().await.unwrap();
    let f = Fixture::new(database.pool.clone()).await;
    f.replace(f.owner, "0_sales.md", "AX-4401 public list price 100")
        .await;
    f.replace(f.owner, "5_sales.md", "AX-4410 private discount floor 60")
        .await;
    let mut wrong_model = prepared("0_other.md", "AX-4410 from another model");
    wrong_model.embedding_model = "other-model".into();
    f.store.replace(f.owner, &[wrong_model]).await.unwrap();
    let foreign = Fixture::new(database.pool.clone()).await;
    foreign
        .replace(foreign.owner, "0_foreign.md", "AX-4410 FOREIGN CANARY")
        .await;

    let input = json!({"query":"What is the price of AX-4410?"});
    let public = f
        .store
        .visible(request(f.owner.company, AccessLevel::PUBLIC, input.clone()))
        .await
        .unwrap();
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].body, "AX-4401 public list price 100");
    assert_eq!(public[0].keyword_rank, None);
    let result = rank_chunks(
        public,
        &vector(0.0, 1.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    assert_eq!(result.result(), &json!({"passages":[]}));
    assert_eq!(result.highest(), None);

    let both = f
        .store
        .visible(request(f.owner.company, AccessLevel::L5, input))
        .await
        .unwrap();
    assert_eq!(both.len(), 2);
    assert!(
        both.iter()
            .find(|chunk| chunk.access_level == AccessLevel::L5)
            .unwrap()
            .keyword_rank
            .is_some()
    );
    let result = rank_chunks(
        both,
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    let passages = result.result()["passages"].as_array().unwrap();
    assert_eq!(passages.len(), 2);
    assert_eq!(passages[0]["text"], "AX-4410 private discount floor 60");
    assert!(passages.iter().all(|passage| passage["file"] == "sales.md"));
    assert!(!result.result().to_string().contains("FOREIGN CANARY"));
    assert!(!result.result().to_string().contains("another model"));
    assert_eq!(result.highest(), Some(AccessLevel::L5));
}

#[tokio::test]
async fn knowledge_search_sql_simple_exact_terms_and_digit_fallback_work_in_non_english_text() {
    let database = own_database().await.unwrap();
    let f = Fixture::new(database.pool.clone()).await;
    f.replace(
        f.owner,
        "0_каталог.md",
        "Артикул AX-4410: Super Plus. Garantía ampliada.",
    )
    .await;
    f.replace(
        f.owner,
        "0_other.md",
        "Артикул AX-4401: Super Basic. Garantía normal.",
    )
    .await;
    for input in [
        json!({"query":"Сколько стоит AX-4410?","exact_terms":[]}),
        json!({"query":"Que cubre?","exact_terms":["garantía ampliada"]}),
        json!({"query":"Tell me more","exact_terms":["Super Plus"]}),
    ] {
        let chunks = f
            .store
            .visible(request(f.owner.company, AccessLevel::PUBLIC, input))
            .await
            .unwrap();
        let result = rank_chunks(
            chunks,
            &vector(0.0, 1.0),
            KnowledgeSearchLimit::default(),
            65536,
        )
        .unwrap();
        let passages = result.result()["passages"].as_array().unwrap();
        assert_eq!(passages.len(), 1);
        assert_eq!(passages[0]["file"], "каталог.md");
        assert!(passages[0]["text"].as_str().unwrap().contains("AX-4410"));
    }
}

#[tokio::test]
async fn knowledge_e2e_quality_fixture_exact_hit_oracle_matches_real_postgres_search() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Fixture::new(database.pool.clone()).await;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../assets/knowledge/retrieval-fixture.json"
    ))
    .unwrap();
    for document in fixture["documents"].as_array().unwrap() {
        let name = format!("0_{}", document["name"].as_str().unwrap());
        let body = format!(
            "# {}\n{}",
            document["heading"].as_str().unwrap(),
            document["text"].as_str().unwrap()
        );
        f.replace(f.owner, &name, &body).await;
    }
    let questions = fixture["questions"].as_array().unwrap();
    assert!((10..=20).contains(&questions.len()));
    for question in questions {
        let input = json!({"query":question["query"],"exact_terms":question["exact_terms"]});
        let rows = f
            .store
            .visible(request(f.owner.company, AccessLevel::L9, input))
            .await
            .unwrap();
        let mut actual: Vec<_> = rows
            .into_iter()
            .filter(|chunk| chunk.keyword_rank.is_some())
            .map(|chunk| chunk.document_name.to_string())
            .collect();
        let mut expected: Vec<_> = question["exact_hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected, "{}", question["query"]);
    }
}
