use super::*;
use crate::entities::access_level::AccessLevel;
use uuid::Uuid;

fn vector(x: f32, y: f32) -> [f32; 384] {
    let mut vector = [0.0; 384];
    vector[0] = x;
    vector[1] = y;
    vector
}

fn chunk(id: u128, embedding: [f32; 384], keyword_rank: Option<f32>) -> StoredKnowledgeChunk {
    StoredKnowledgeChunk {
        id: Uuid::from_u128(id),
        document_name: "sales.md".into(),
        heading: Some("Sales > Prices".into()),
        body: format!("Passage {id}"),
        access_level: AccessLevel::L5,
        embedding,
        keyword_rank,
    }
}

#[test]
fn knowledge_search_threshold_drops_weak_matches_and_does_not_raise_empty_results() {
    let result = rank_chunks(
        vec![chunk(1, vector(0.6, 0.8), None)],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    assert_eq!(result.result(), &serde_json::json!({"passages":[]}));
    assert_eq!(result.highest(), None);
}

#[test]
fn knowledge_search_exact_hits_survive_below_the_meaning_threshold() {
    let result = rank_chunks(
        vec![chunk(1, vector(0.0, 1.0), Some(0.1))],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    assert_eq!(result.result()["passages"][0]["text"], "Passage 1");
    assert_eq!(result.highest(), Some(AccessLevel::L5));
}

#[test]
fn knowledge_search_rrf_promotes_the_exact_code_when_meaning_is_tied() {
    let mut wrong = chunk(1, vector(1.0, 0.0), None);
    wrong.body = "AX-4401 costs 99".into();
    let mut exact = chunk(2, vector(1.0, 0.0), Some(0.2));
    exact.body = "AX-4410 costs 199".into();
    let result = rank_chunks(
        vec![wrong, exact],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    assert_eq!(result.result()["passages"][0]["text"], "AX-4410 costs 199");
}

#[test]
fn knowledge_search_result_omits_levels_ids_and_filename_prefixes() {
    let result = rank_chunks(
        vec![chunk(1, vector(1.0, 0.0), None)],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        65536,
    )
    .unwrap();
    let passage = &result.result()["passages"][0];
    assert_eq!(passage["file"], "sales.md");
    let keys: Vec<_> = passage
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["file", "heading", "text"]);
    assert!(
        !result
            .result()
            .to_string()
            .contains(&Uuid::from_u128(1).to_string())
    );
}

#[test]
fn knowledge_search_result_bound_counts_escaped_json_and_preserves_utf8() {
    let mut large = chunk(1, vector(1.0, 0.0), None);
    large.body = "界\"\\\n".repeat(10000);
    let result = rank_chunks(
        vec![large],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        512,
    )
    .unwrap();
    assert!(result.result().to_string().len() <= 512);
    let text = result.result()["passages"][0]["text"].as_str().unwrap();
    assert!(!text.is_empty());
    assert!("界\"\\\n".repeat(10000).starts_with(text));
    assert_eq!(result.highest(), Some(AccessLevel::L5));
}

#[test]
fn knowledge_search_discarded_high_passage_does_not_raise_classification() {
    let mut public = chunk(1, vector(1.0, 0.0), None);
    public.access_level = AccessLevel::PUBLIC;
    public.body = "public ".repeat(1000);
    let mut private = chunk(2, vector(1.0, 0.0), None);
    private.access_level = AccessLevel::MAX;
    let result = rank_chunks(
        vec![public, private],
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::default(),
        256,
    )
    .unwrap();
    assert_eq!(result.result()["passages"].as_array().unwrap().len(), 1);
    assert_eq!(result.highest(), Some(AccessLevel::PUBLIC));
}

#[test]
fn knowledge_search_top_k_and_score_ties_are_deterministic() {
    let chunks = (1..=10)
        .rev()
        .map(|id| chunk(id, vector(1.0, 0.0), None))
        .collect();
    let result = rank_chunks(
        chunks,
        &vector(1.0, 0.0),
        KnowledgeSearchLimit::new(8).unwrap(),
        65536,
    )
    .unwrap();
    let passages = result.result()["passages"].as_array().unwrap();
    assert_eq!(passages.len(), 8);
    assert_eq!(passages[0]["text"], "Passage 1");
    assert_eq!(passages[7]["text"], "Passage 8");
}

#[test]
fn knowledge_search_invalid_vectors_fail_without_fabricating_an_empty_result() {
    assert!(
        rank_chunks(
            vec![chunk(1, vector(2.0, 0.0), None)],
            &vector(1.0, 0.0),
            KnowledgeSearchLimit::default(),
            65536
        )
        .is_err()
    );
    assert!(
        rank_chunks(
            vec![],
            &vector(f32::NAN, 0.0),
            KnowledgeSearchLimit::default(),
            65536
        )
        .is_err()
    );
}
