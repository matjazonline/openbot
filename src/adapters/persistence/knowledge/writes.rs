use super::*;
use sqlx::QueryBuilder;
use uuid::Uuid;
pub(super) fn validate(documents: &[PreparedKnowledgeDocument]) -> AppResult<()> {
    let mut total = 0;
    for document in documents {
        total += document.chunks.len();
        if document.chunks.is_empty()
            || document.chunks.len() > MAX_CHUNKS_PER_FILE
            || document.embedding_model.is_empty()
            || document.embedding_model.len() > 256
        {
            return Err(AppError::BadRequest(
                "The indexed file exceeds the library limits".into(),
            ));
        }
        for chunk in &document.chunks {
            if chunk
                .chunk
                .heading
                .as_ref()
                .is_some_and(|heading| heading.len() > MAX_HEADING_BYTES)
                || chunk.chunk.body.len() > MAX_KNOWLEDGE_FILE_BYTES
            {
                return Err(AppError::BadRequest(
                    "The indexed file exceeds the library limits".into(),
                ));
            }
        }
    }
    if total > MAX_CHUNKS_PER_REQUEST {
        return Err(AppError::BadRequest(
            "The upload has too many chunks".into(),
        ));
    }
    Ok(())
}
pub(super) async fn replace(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
    document: &PreparedKnowledgeDocument,
) -> AppResult<Uuid> {
    let file = &document.file;
    let id=sqlx::query_scalar::<_,Uuid>("INSERT INTO knowledge_documents (id,company_id,name,access_level,content,content_sha256,uploaded_by) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (company_id,name_key,access_level) DO UPDATE SET name=EXCLUDED.name,content=EXCLUDED.content,content_sha256=EXCLUDED.content_sha256,uploaded_by=EXCLUDED.uploaded_by,revision=knowledge_documents.revision+1,updated_at=clock_timestamp() RETURNING id")
        .bind(Uuid::new_v4()).bind(scope.company.as_uuid()).bind(file.file_name().name().as_str()).bind(file.file_name().level().as_i16()).bind(file.text()).bind(file.sha256().as_slice()).bind(scope.actor_id)
        .fetch_one(&mut **tx).await.map_err(authority::database)?;
    sqlx::query("DELETE FROM knowledge_chunks WHERE company_id=$1 AND document_id=$2")
        .bind(scope.company.as_uuid())
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(authority::database)?;
    for (offset, batch) in document.chunks.chunks(64).enumerate() {
        let mut query = QueryBuilder::new(
            "INSERT INTO knowledge_chunks (id,company_id,document_id,access_level,ordinal,heading,body,embedding_model,embedding) ",
        );
        query.push_values(batch.iter().enumerate(), |mut row, (index, chunk)| {
            row.push_bind(Uuid::new_v4())
                .push_bind(scope.company.as_uuid())
                .push_bind(id)
                .push_bind(file.file_name().level().as_i16())
                .push_bind((offset * 64 + index) as i32)
                .push_bind(&chunk.chunk.heading)
                .push_bind(&chunk.chunk.body)
                .push_bind(document.embedding_model.as_str())
                .push_bind(chunk.embedding.to_vec());
        });
        query
            .build()
            .execute(&mut **tx)
            .await
            .map_err(authority::database)?;
    }
    Ok(id)
}
