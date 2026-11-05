use super::{
    KnowledgeObservation, KnowledgeSearchLimit, MIN_MEANING_SIMILARITY, StoredKnowledgeChunk,
};
use crate::app_error::{AppError, AppResult};
use serde::Serialize;

struct Score {
    index: usize,
    value: f64,
}

#[derive(Serialize)]
struct Passage {
    file: String,
    heading: Option<String>,
    text: String,
}

#[derive(Serialize, Default)]
struct ResultBody {
    passages: Vec<Passage>,
}

/// Security filtering belongs to the single SQL scan; both ranks use exactly these candidates.
pub fn rank_chunks(
    chunks: Vec<StoredKnowledgeChunk>,
    query: &[f32; 384],
    limit: KnowledgeSearchLimit,
    byte_limit: usize,
) -> AppResult<KnowledgeObservation> {
    require_unit(query)?;
    let mut meaning = Vec::new();
    let mut exact = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        require_unit(&chunk.embedding)?;
        let similarity: f32 = chunk.embedding.iter().zip(query).map(|(a, b)| a * b).sum();
        if similarity >= MIN_MEANING_SIMILARITY {
            meaning.push(Score {
                index,
                value: f64::from(similarity),
            });
        }
        if let Some(rank) = chunk.keyword_rank {
            if !rank.is_finite() || rank < 0.0 {
                return Err(invalid());
            }
            exact.push(Score {
                index,
                value: f64::from(rank),
            });
        }
    }
    sort_scores(&mut meaning, &chunks);
    sort_scores(&mut exact, &chunks);
    let mut scores = vec![0.0; chunks.len()];
    for list in [&meaning, &exact] {
        for (offset, entry) in list.iter().enumerate() {
            scores[entry.index] += 1.0 / (61.0 + offset as f64);
        }
    }
    let mut merged: Vec<_> = scores
        .into_iter()
        .enumerate()
        .filter(|(_, value)| *value > 0.0)
        .map(|(index, value)| Score { index, value })
        .collect();
    sort_scores(&mut merged, &chunks);
    bounded_result(&chunks, &merged, limit, byte_limit)
}

fn sort_scores(scores: &mut [Score], chunks: &[StoredKnowledgeChunk]) {
    scores.sort_by(|left, right| {
        right
            .value
            .total_cmp(&left.value)
            .then_with(|| chunks[left.index].id.cmp(&chunks[right.index].id))
    });
}

fn bounded_result(
    chunks: &[StoredKnowledgeChunk],
    ranked: &[Score],
    limit: KnowledgeSearchLimit,
    byte_limit: usize,
) -> AppResult<KnowledgeObservation> {
    let mut body = ResultBody::default();
    if encoded_len(&body)? > byte_limit {
        return Err(invalid());
    }
    let mut highest = None;
    for entry in ranked.iter().take(limit.get()) {
        let chunk = &chunks[entry.index];
        body.passages.push(Passage {
            file: chunk.document_name.to_string(),
            heading: chunk.heading.clone(),
            text: prefix(&chunk.body, byte_limit).to_owned(),
        });
        fit_last(&mut body, byte_limit)?;
        if body
            .passages
            .last()
            .is_some_and(|passage| passage.text.is_empty())
        {
            body.passages.pop();
            continue;
        }
        highest = Some(highest.map_or(
            chunk.access_level,
            |old: crate::entities::access_level::AccessLevel| old.max(chunk.access_level),
        ));
    }
    Ok(KnowledgeObservation::new(
        serde_json::to_value(body).map_err(|_| invalid())?,
        highest,
    ))
}

fn fit_last(body: &mut ResultBody, byte_limit: usize) -> AppResult<()> {
    if encoded_len(body)? <= byte_limit {
        return Ok(());
    }
    let Some(last) = body.passages.last_mut() else {
        return Err(invalid());
    };
    let original = std::mem::take(&mut last.text);
    let mut low = 0;
    let mut high = original.chars().count();
    while low < high {
        let count = low + (high - low).div_ceil(2);
        body.passages.last_mut().unwrap().text = original.chars().take(count).collect();
        if encoded_len(body)? <= byte_limit {
            low = count;
        } else {
            high = count - 1;
        }
    }
    body.passages.last_mut().unwrap().text = original.chars().take(low).collect();
    // If even the citation cannot fit, the caller drops this passage and its classification.
    if encoded_len(body)? > byte_limit {
        body.passages.last_mut().unwrap().text.clear();
    }
    Ok(())
}

fn prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn encoded_len(body: &ResultBody) -> AppResult<usize> {
    serde_json::to_vec(body)
        .map(|bytes| bytes.len())
        .map_err(|_| invalid())
}

fn require_unit(vector: &[f32; 384]) -> AppResult<()> {
    let squared: f64 = vector.iter().map(|value| f64::from(*value).powi(2)).sum();
    if vector.iter().all(|value| value.is_finite()) && (squared - 1.0).abs() <= 0.001 {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn invalid() -> AppError {
    AppError::ServiceUnavailable("Knowledge search could not produce a bounded result".into())
}

#[cfg(test)]
#[path = "ranking_tests.rs"]
mod tests;
