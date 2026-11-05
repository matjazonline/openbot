use super::*;
use crate::{
    application::knowledge::search::{
        KnowledgeChunks, StoredKnowledgeChunk, VisibleKnowledgeQuery,
    },
    entities::{access_level::AccessLevel, value_objects::KnowledgeDocumentName},
};
use std::time::Duration;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct ChunkRow {
    id: Uuid,
    document_name: String,
    heading: Option<String>,
    body: String,
    access_level: i16,
    embedding: Vec<f32>,
    keyword_rank: Option<f32>,
}

// These are internal candidates for a fenced workflow read, never a human viewer projection.
// The canonical observation and its classification must commit before the agent receives them.
const VISIBLE: &str = "WITH exact_terms AS (
    SELECT phraseto_tsquery('simple', term) AS query
      FROM unnest($4::text[]) AS terms(term)
), exact_query AS (
    SELECT string_agg('(' || query::text || ')', ' | ')
        FILTER (WHERE query::text <> '')::tsquery AS query FROM exact_terms
)
SELECT chunk.id, document.name AS document_name, chunk.heading, chunk.body,
       chunk.access_level::smallint AS access_level, chunk.embedding,
       CASE WHEN exact_query.query IS NOT NULL AND chunk.search_text @@ exact_query.query
            THEN ts_rank_cd(chunk.search_text, exact_query.query) END AS keyword_rank
  FROM knowledge_chunks AS chunk
  JOIN knowledge_documents AS document
    ON (document.company_id, document.id, document.access_level)
       = (chunk.company_id, chunk.document_id, chunk.access_level)
 CROSS JOIN exact_query
 WHERE chunk.company_id=$1 AND chunk.embedding_model=$2 AND chunk.access_level<=$3
 ORDER BY chunk.id
 LIMIT 8193";

#[async_trait]
impl KnowledgeChunks for PostgresPersistence {
    async fn visible(&self, query: VisibleKnowledgeQuery) -> AppResult<Vec<StoredKnowledgeChunk>> {
        let exact: Vec<_> = query.exact_terms.iter().map(|term| term.as_str()).collect();
        if exact.iter().map(|term| term.chars().count()).sum::<usize>() > 1512 {
            return Err(unavailable());
        }
        let read = async {
            let mut tx = self.pool().begin().await?;
            sqlx::query("SET LOCAL statement_timeout='1500ms'")
                .execute(&mut *tx)
                .await?;
            let rows = sqlx::query_as::<_, ChunkRow>(VISIBLE)
                .bind(query.company.as_uuid())
                .bind(query.model.as_str())
                .bind(query.ceiling.as_i16())
                .bind(exact)
                .fetch_all(&mut *tx)
                .await?;
            if rows.len() > MAX_COMPANY_KNOWLEDGE_CHUNKS {
                return Err(unavailable());
            }
            let chunks = rows
                .into_iter()
                .map(decode)
                .collect::<AppResult<Vec<_>>>()?;
            tx.commit().await?;
            Ok(chunks)
        };
        tokio::time::timeout(Duration::from_secs(2), read)
            .await
            .map_err(|_| {
                AppError::Timeout("Knowledge search exceeded its database deadline".into())
            })?
    }
}

fn decode(row: ChunkRow) -> AppResult<StoredKnowledgeChunk> {
    Ok(StoredKnowledgeChunk {
        id: row.id,
        document_name: KnowledgeDocumentName::from(row.document_name),
        heading: row.heading,
        body: row.body,
        access_level: AccessLevel::try_from(row.access_level).map_err(|_| unavailable())?,
        embedding: row.embedding.try_into().map_err(|_| unavailable())?,
        keyword_rank: row.keyword_rank,
    })
}

fn unavailable() -> AppError {
    AppError::ServiceUnavailable("Knowledge search could not read the company library".into())
}
