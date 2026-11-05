use super::*;
use crate::entities::{access_level::AccessLevel, value_objects::KnowledgeDocumentName};
use std::collections::HashMap;
use uuid::Uuid;
#[derive(sqlx::FromRow)]
struct LayerRow {
    id: Uuid,
    name: String,
    access_level: i16,
    revision: i64,
    text_bytes: i64,
}
impl LayerRow {
    fn decode(self) -> AppResult<KnowledgeDocumentLayer> {
        let access_level = AccessLevel::try_from(self.access_level)
            .map_err(|error| AppError::Internal(error.into()))?;
        let parsed = KnowledgeFileName::parse(&format!("{}_{}", self.access_level, self.name))
            .map_err(|_| AppError::Internal("Stored knowledge filename is invalid".into()))?;
        if parsed.name().as_str() != self.name
            || !parsed.is_supported_text()
            || self.revision < 1
            || self.text_bytes < 0
            || self.text_bytes > MAX_KNOWLEDGE_FILE_BYTES as i64
        {
            return Err(AppError::Internal(
                "Stored knowledge metadata is invalid".into(),
            ));
        }
        Ok(KnowledgeDocumentLayer {
            id: self.id,
            name: parsed.name().clone(),
            access_level,
            revision: self.revision,
            text_bytes: self.text_bytes as u64,
        })
    }
}
const SELECT: &str = "SELECT document.id,document.name,document.access_level::smallint AS access_level,document.revision,octet_length(document.content)::bigint AS text_bytes FROM knowledge_documents AS document";
pub(super) async fn visible(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    level: AccessLevel,
) -> AppResult<Vec<KnowledgeDocumentLayer>> {
    let query = format!(
        "{SELECT} WHERE document.company_id=$1 AND document.access_level<=$2 ORDER BY document.name_key,document.access_level LIMIT 4097"
    );
    decode(
        sqlx::query_as::<_, LayerRow>(&query)
            .bind(scope.company.as_uuid())
            .bind(level.as_i16())
            .fetch_all(&mut **tx)
            .await
            .map_err(authority::database)?,
    )
}
pub(super) async fn by_ids(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    ids: &[Uuid],
) -> AppResult<Vec<KnowledgeDocumentLayer>> {
    let query = format!(
        "{SELECT} WHERE document.company_id=$1 AND document.id=ANY($2) ORDER BY document.name_key,document.access_level LIMIT 10"
    );
    decode(
        sqlx::query_as::<_, LayerRow>(&query)
            .bind(scope.company.as_uuid())
            .bind(ids)
            .fetch_all(&mut **tx)
            .await
            .map_err(authority::database)?,
    )
}
pub(super) async fn one_visible(
    tx: &mut Transaction<'_, Postgres>,
    command: DeleteKnowledgeLayer,
    level: AccessLevel,
) -> AppResult<KnowledgeDocumentLayer> {
    let query = format!(
        "{SELECT} WHERE document.company_id=$1 AND document.id=$2 AND document.access_level<=$3"
    );
    sqlx::query_as::<_, LayerRow>(&query)
        .bind(command.scope.company.as_uuid())
        .bind(command.document_id)
        .bind(level.as_i16())
        .fetch_optional(&mut **tx)
        .await
        .map_err(authority::database)?
        .ok_or_else(|| AppError::NotFound("File not found".into()))?
        .decode()
}
fn decode(rows: Vec<LayerRow>) -> AppResult<Vec<KnowledgeDocumentLayer>> {
    if rows.len() > MAX_COMPANY_KNOWLEDGE_LAYERS {
        return Err(quota());
    }
    rows.into_iter().map(LayerRow::decode).collect()
}
fn quota() -> AppError {
    AppError::BadRequest(
        "The company knowledge limit would be exceeded; remove files or upload less text".into(),
    )
}
pub(super) async fn project_quota(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    layers: &[KnowledgeLayerSize],
) -> AppResult<()> {
    let query = format!("{SELECT} WHERE document.company_id=$1 LIMIT 4097");
    let current = decode(
        sqlx::query_as::<_, LayerRow>(&query)
            .bind(scope.company.as_uuid())
            .fetch_all(&mut **tx)
            .await
            .map_err(authority::database)?,
    )?;
    let mut projected: HashMap<(KnowledgeDocumentName, AccessLevel), u64> = current
        .into_iter()
        .map(|row| ((row.name, row.access_level), row.text_bytes))
        .collect();
    for layer in layers {
        if layer.text_bytes > MAX_KNOWLEDGE_FILE_BYTES as u64 {
            return Err(quota());
        }
        projected.insert((layer.name.clone(), layer.access_level), layer.text_bytes);
    }
    if projected.len() > MAX_COMPANY_KNOWLEDGE_LAYERS
        || projected.values().sum::<u64>() > MAX_COMPANY_KNOWLEDGE_BYTES as u64
    {
        return Err(quota());
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
struct Usage {
    layers: i64,
    bytes: i64,
    chunks: i64,
}
pub(super) async fn check_quota(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
) -> AppResult<()> {
    let usage = sqlx::query_as::<_, Usage>(
        "SELECT count(*) AS layers,
        coalesce(sum(octet_length(document.content)),0)::bigint AS bytes,
        (SELECT count(*) FROM knowledge_chunks AS chunk WHERE chunk.company_id=$1) AS chunks
        FROM knowledge_documents AS document WHERE document.company_id=$1",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(authority::database)?;
    if usage.layers > MAX_COMPANY_KNOWLEDGE_LAYERS as i64
        || usage.bytes > MAX_COMPANY_KNOWLEDGE_BYTES as i64
        || usage.chunks > MAX_COMPANY_KNOWLEDGE_CHUNKS as i64
    {
        return Err(quota());
    }
    Ok(())
}
