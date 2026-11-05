use super::*;
use axum::extract::{FromRequest, Multipart};
use std::time::Duration;
pub(super) const BODY_LIMIT: usize = MAX_COMPANY_KNOWLEDGE_BYTES + 64 * 1024;
const READ_DEADLINE: Duration = Duration::from_secs(30);
pub(super) async fn receive(
    s: &Services,
    scope: KnowledgeScope,
    request: Request,
) -> Result<Vec<KnowledgeDocumentLayer>, Response> {
    // Resolve account/company membership and admission before constructing a multipart reader.
    s.library
        .documents()
        .access(scope)
        .await
        .map_err(IntoResponse::into_response)?;
    let _reader = s.library.admit_body(scope).map_err(|refused| {
        let status = match refused {
            UploadAdmissionRefused::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            UploadAdmissionRefused::Busy => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, refused.to_string()).into_response()
    })?;
    let multipart = Multipart::from_request(request, &())
        .await
        .map_err(IntoResponse::into_response)?;
    let files = tokio::time::timeout(READ_DEADLINE, files(multipart))
        .await
        .map_err(|_| {
            (
                StatusCode::REQUEST_TIMEOUT,
                "The upload took too long; choose the files again",
            )
                .into_response()
        })??;
    s.library.upload(scope, files).await.map_err(mutation_error)
}
pub(super) async fn files(mut multipart: Multipart) -> Result<Vec<KnowledgeUploadFile>, Response> {
    let mut files = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(IntoResponse::into_response)?
    {
        if field.name() != Some("files") {
            return Err(AppError::BadRequest(
                "Only file fields are accepted; the level comes from each filename".into(),
            )
            .into_response());
        }
        if files.len() == MAX_KNOWLEDGE_FILES {
            return Err(AppError::BadRequest("Pick at most ten files".into()).into_response());
        }
        let filename = field
            .file_name()
            .ok_or_else(|| {
                AppError::BadRequest("Choose a named UTF-8 .txt or .md file".into()).into_response()
            })?
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(IntoResponse::into_response)? {
            if chunk.len() > MAX_KNOWLEDGE_FILE_BYTES.saturating_sub(bytes.len()) {
                return Err(
                    AppError::BadRequest("A selected file exceeds 1 MiB".into()).into_response()
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        files.push(
            KnowledgeUploadFile::from_upload(&filename, bytes)
                .map_err(IntoResponse::into_response)?,
        );
    }
    if files.is_empty() {
        return Err(AppError::BadRequest("Choose at least one file".into()).into_response());
    }
    Ok(files)
}
