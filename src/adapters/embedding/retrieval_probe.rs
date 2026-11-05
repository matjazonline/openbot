//! Public quality fixtures use the production tokenizer and ranker. PostgreSQL separately checks
//! the fixture's exact-hit oracle; the image probe retains its network-none execution boundary.
use super::*;
use crate::application::knowledge::{chunk_document, search::*};
use crate::entities::{access_level::AccessLevel, value_objects::KnowledgeDocumentName};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    documents: Vec<Document>,
    questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    name: KnowledgeDocumentName,
    heading: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    query: String,
    exact_terms: Vec<String>,
    expected_file: Option<KnowledgeDocumentName>,
    exact_hits: Vec<KnowledgeDocumentName>,
}

#[derive(Serialize)]
pub(super) struct RetrievalCheck {
    threshold: f32,
    questions: usize,
    matched: usize,
    exact_first_checks: usize,
    exact_first_passed: usize,
    min_expected_similarity: f32,
    max_unrelated_similarity: f32,
    bounded_corpus_chunks: usize,
    outcomes: Vec<Outcome>,
}

#[derive(Serialize)]
struct Outcome {
    query: String,
    expected_file: Option<KnowledgeDocumentName>,
    returned_files: Vec<KnowledgeDocumentName>,
    expected_similarity: Option<f32>,
    matched: bool,
    exact_first: bool,
}

pub(super) fn check(model: &LocalTextEmbedder) -> anyhow::Result<RetrievalCheck> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../assets/knowledge/retrieval-fixture.json"
    ))?;
    if !(10..=20).contains(&fixture.questions.len()) {
        bail!("Retrieval quality fixture must contain 10–20 questions");
    }
    let documents = embed_documents(model, &fixture.documents)?;
    let mut outcomes = Vec::new();
    let mut min_expected = 1.0_f32;
    let mut max_unrelated = -1.0_f32;
    for question in &fixture.questions {
        let query = model.embed_query(&question.query)?;
        let outcome = answer(question, &documents, &query)?;
        if let Some(score) = outcome.expected_similarity {
            min_expected = min_expected.min(score);
        } else {
            for document in &documents {
                max_unrelated = max_unrelated.max(similarity(&query, &document.embedding));
            }
        }
        outcomes.push(outcome);
    }
    Ok(RetrievalCheck {
        threshold: MIN_MEANING_SIMILARITY,
        questions: outcomes.len(),
        matched: outcomes.iter().filter(|outcome| outcome.matched).count(),
        exact_first_checks: fixture
            .questions
            .iter()
            .filter(|q| !q.exact_hits.is_empty())
            .count(),
        exact_first_passed: fixture
            .questions
            .iter()
            .zip(&outcomes)
            .filter(|(q, outcome)| !q.exact_hits.is_empty() && outcome.exact_first)
            .count(),
        min_expected_similarity: min_expected,
        max_unrelated_similarity: max_unrelated,
        bounded_corpus_chunks: bounded_corpus(model)?,
        outcomes,
    })
}

fn embed_documents(
    model: &LocalTextEmbedder,
    documents: &[Document],
) -> anyhow::Result<Vec<StoredKnowledgeChunk>> {
    let mut chunks = Vec::new();
    for document in documents {
        let source = format!("# {}\n{}", document.heading, document.text);
        let mut prepared = chunk_document(&source, |text| model.passage_token_count(text))?;
        if prepared.len() != 1 {
            bail!("Quality fixture must identify one passage per file");
        }
        chunks.push(prepared.pop().unwrap());
    }
    let texts: Vec<_> = chunks.iter().map(|chunk| chunk.embedding_text()).collect();
    let mut vectors = Vec::new();
    for batch in texts.chunks(4) {
        vectors
            .extend(model.embed_passages(&batch.iter().map(String::as_str).collect::<Vec<_>>())?);
    }
    Ok(documents
        .iter()
        .zip(chunks)
        .zip(vectors)
        .enumerate()
        .map(
            |(index, ((document, chunk), embedding))| StoredKnowledgeChunk {
                id: Uuid::from_u128(index as u128 + 1),
                document_name: document.name.clone(),
                heading: chunk.heading,
                body: chunk.body,
                access_level: AccessLevel::PUBLIC,
                embedding,
                keyword_rank: None,
            },
        )
        .collect())
}

fn answer(
    question: &Question,
    documents: &[StoredKnowledgeChunk],
    query: &[f32; 384],
) -> anyhow::Result<Outcome> {
    let parsed = KnowledgeSearchQuery::parse(&serde_json::json!({
        "query":question.query,"exact_terms":question.exact_terms,
    }))?;
    if parsed.exact_terms().is_empty() && !question.exact_hits.is_empty() {
        bail!("Exact-hit fixture has no actual extracted terms");
    }
    let chunks = documents
        .iter()
        .map(|chunk| StoredKnowledgeChunk {
            id: chunk.id,
            document_name: chunk.document_name.clone(),
            heading: chunk.heading.clone(),
            body: chunk.body.clone(),
            access_level: chunk.access_level,
            embedding: chunk.embedding,
            keyword_rank: question
                .exact_hits
                .contains(&chunk.document_name)
                .then_some(1.0),
        })
        .collect();
    let result = rank_chunks(
        chunks,
        query,
        KnowledgeSearchLimit::default(),
        result_bound(),
    )?;
    let returned_files: Vec<KnowledgeDocumentName> = result.result()["passages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|passage| KnowledgeDocumentName::from(passage["file"].as_str().unwrap()))
        .collect();
    let expected_similarity = question
        .expected_file
        .as_ref()
        .map(|expected| {
            documents
                .iter()
                .find(|document| &document.document_name == expected)
                .map(|document| similarity(query, &document.embedding))
                .ok_or_else(|| anyhow::anyhow!("Unknown expected quality fixture file"))
        })
        .transpose()?;
    let matched = question
        .expected_file
        .as_ref()
        .map_or(returned_files.is_empty(), |file| {
            returned_files.contains(file)
        });
    let exact_first = question.exact_hits.is_empty()
        || returned_files
            .first()
            .is_some_and(|file| question.exact_hits.contains(file));
    Ok(Outcome {
        query: question.query.clone(),
        expected_file: question.expected_file.clone(),
        returned_files,
        expected_similarity,
        matched,
        exact_first,
    })
}

fn bounded_corpus(model: &LocalTextEmbedder) -> anyhow::Result<usize> {
    use crate::entities::knowledge::{MAX_COMPANY_KNOWLEDGE_BYTES, MAX_COMPANY_KNOWLEDGE_CHUNKS};
    let count = MAX_COMPANY_KNOWLEDGE_CHUNKS;
    let query = model.embed_query("What is the return policy?")?;
    // Retain raw vector allocations alongside decoded candidates, conservatively covering the
    // transport-to-row conversion as well as the repeated heading/body memory of the single scan.
    let raw_vectors = vec![query.to_vec(); count];
    let body = "refund warranty ".repeat(75);
    if count * body.len() > MAX_COMPANY_KNOWLEDGE_BYTES {
        bail!("Capacity fixture exceeds its text quota");
    }
    let chunks = raw_vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| StoredKnowledgeChunk {
            id: Uuid::from_u128(index as u128 + 1),
            document_name: format!("capacity-{}.md", index % 10).into(),
            heading: Some("h".repeat(2048)),
            body: body.clone(),
            access_level: AccessLevel::PUBLIC,
            embedding: vector.as_slice().try_into().unwrap(),
            keyword_rank: Some(1.0),
        })
        .collect();
    let observation = rank_chunks(
        chunks,
        &query,
        KnowledgeSearchLimit::default(),
        result_bound(),
    )?;
    std::hint::black_box(&raw_vectors);
    if observation.result()["passages"].as_array().unwrap().len() != 5 {
        bail!("Bounded corpus probe did not rank the complete scan");
    }
    Ok(count)
}

fn result_bound() -> usize {
    crate::application::workflow::tools::MAX_TOOL_RESULT_BYTES
        .min(crate::services::harness::runs::MAX_RESULT_BYTES)
}

fn similarity(left: &[f32; 384], right: &[f32; 384]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}
