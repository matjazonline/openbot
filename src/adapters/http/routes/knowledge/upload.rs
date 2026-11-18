use super::*;
use axum::extract::{FromRequest, Multipart};
use std::time::Duration;
pub(super) const BODY_LIMIT: usize = MAX_COMPANY_KNOWLEDGE_BYTES + 64 * 1024;
const READ_DEADLINE: Duration = Duration::from_secs(30);
pub(super) async fn receive(
    s: &Services,
    scope: KnowledgeScope,
    request: Request,
) -> Result<Vec<KnowledgeDocumentLayer>, Box<Response>> {
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
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(IntoResponse::into_response)?;
    let received = tokio::time::timeout(READ_DEADLINE, async {
        let unit = unit(&mut multipart).await?;
        let target = crate::application::knowledge::KnowledgeUploadTarget { scope, unit };
        s.library
            .documents()
            .preflight_upload(target, &[])
            .await
            .map_err(IntoResponse::into_response)?;
        Ok::<_, Box<Response>>((target, files(multipart).await?))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::REQUEST_TIMEOUT,
            "The upload took too long; choose the files again",
        )
            .into_response()
    })??;
    s.library
        .upload(received.0, received.1)
        .await
        .map_err(mutation_error)
        .map_err(Box::new)
}

async fn unit(
    multipart: &mut Multipart,
) -> Result<crate::entities::department::DepartmentId, Box<Response>> {
    let invalid = || {
        Box::new(
            AppError::BadRequest("Choose a department: unit_id must precede the files".into())
                .into_response(),
        )
    };
    let mut field = multipart
        .next_field()
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or_else(invalid)?;
    if field.name() != Some("unit_id") || field.file_name().is_some() {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(36);
    while let Some(chunk) = field.chunk().await.map_err(IntoResponse::into_response)? {
        if chunk.len() > 36usize.saturating_sub(bytes.len()) {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    Uuid::parse_str(raw)
        .map(crate::entities::department::DepartmentId::new)
        .map_err(|_| invalid())
}
pub(super) async fn files(
    mut multipart: Multipart,
) -> Result<Vec<KnowledgeUploadFile>, Box<Response>> {
    let mut files = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(IntoResponse::into_response)?
    {
        if field.name() != Some("files") {
            return Err(Box::new(
                AppError::BadRequest(
                    "Only file fields are accepted; the level comes from each filename".into(),
                )
                .into_response(),
            ));
        }
        if files.len() == MAX_KNOWLEDGE_FILES {
            return Err(Box::new(
                AppError::BadRequest("Pick at most ten files".into()).into_response(),
            ));
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
                return Err(Box::new(
                    AppError::BadRequest("A selected file exceeds 1 MiB".into()).into_response(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        files.push(
            KnowledgeUploadFile::from_upload(&filename, bytes)
                .map_err(IntoResponse::into_response)?,
        );
    }
    if files.is_empty() {
        return Err(Box::new(
            AppError::BadRequest("Choose at least one file".into()).into_response(),
        ));
    }
    Ok(files)
}
