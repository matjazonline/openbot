use super::*;
use axum::{extract::Multipart, http::header};

pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<SkillUseCases>: axum::extract::FromRef<S>,
    Arc<UserUseCases>: axum::extract::FromRef<S>,
    Arc<AppConfig>: axum::extract::FromRef<S>,
{
    Router::new()
        .route(
            "/ui/companies/{company_id}/skills/import",
            post(company_import),
        )
        .route(
            "/ui/companies/{company_id}/skills/{skill_id}/bundle",
            get(company_download).post(company_replace),
        )
        .route(
            "/ui/companies/{company_id}/skills/{skill_id}/bundle/files",
            get(company_files),
        )
        .route("/ui/skill-library/import", post(library_import))
        .route(
            "/ui/skill-library/{skill_id}/bundle",
            get(library_download).post(library_replace),
        )
        .route(
            "/ui/skill-library/{skill_id}/bundle/files",
            get(library_files),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
}

async fn upload(mut multipart: Multipart) -> AppResult<SkillWrite> {
    let field = multipart
        .next_field()
        .await
        .map_err(|_| bad_upload())?
        .ok_or_else(bad_upload)?;
    if field.name() != Some("bundle") {
        return Err(bad_upload());
    }
    let bytes = field.bytes().await.map_err(|_| bad_upload())?;
    if multipart
        .next_field()
        .await
        .map_err(|_| bad_upload())?
        .is_some()
    {
        return Err(bad_upload());
    }
    super::archive::decode(&bytes)
}

fn bad_upload() -> AppError {
    AppError::BadRequest("Choose one skill ZIP, at most 1 MiB, in the bundle field.".into())
}
fn missing() -> AppError {
    AppError::NotFound("Skill not found in this library.".into())
}

async fn company_import(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    multipart: Multipart,
) -> AppResult<Response> {
    skills.verify_company_owner(user.id, company).await?;
    let result = match upload(multipart).await {
        Ok(write) => skills.create_company(user.id, company, write).await,
        Err(error) => Err(error),
    };
    company_result(
        &skills,
        CompanySkillOwner {
            actor: user.id,
            company,
        },
        None,
        result,
    )
    .await
}

async fn company_replace(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    multipart: Multipart,
) -> AppResult<Response> {
    skills.verify_company_owner(user.id, company).await?;
    let existing = skills
        .get_company(user.id, company, id)
        .await?
        .ok_or_else(missing)?;
    let result = match upload(multipart).await {
        Ok(write) => skills.update_company(user.id, company, id, write).await,
        Err(error) => Err(error),
    };
    company_result(
        &skills,
        CompanySkillOwner {
            actor: user.id,
            company,
        },
        Some(&existing),
        result,
    )
    .await
}

struct CompanySkillOwner {
    actor: Uuid,
    company: Uuid,
}

async fn company_result(
    skills: &SkillUseCases,
    owner: CompanySkillOwner,
    existing: Option<&Skill>,
    result: AppResult<Skill>,
) -> AppResult<Response> {
    let UploadResult {
        draft,
        error,
        notice,
    } = upload_result(existing, result)?;
    Ok(Html(
        company_page(
            skills,
            owner.actor,
            owner.company,
            &PageQuery::default(),
            draft.as_ref(),
            error.as_deref(),
            notice,
        )
        .await?,
    )
    .into_response())
}

async fn library_import(
    State(skills): State<Arc<SkillUseCases>>,
    caller: Caller,
    multipart: Multipart,
) -> AppResult<Response> {
    skills.verify_library_operator(&caller)?;
    let result = match upload(multipart).await {
        Ok(write) => skills.create_library(&caller, write).await,
        Err(error) => Err(error),
    };
    library_result(&skills, &caller, None, result).await
}

async fn library_replace(
    State(skills): State<Arc<SkillUseCases>>,
    caller: Caller,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> AppResult<Response> {
    skills.verify_library_operator(&caller)?;
    let existing = skills
        .library_skill_for(&caller, id)
        .await?
        .ok_or_else(missing)?;
    let result = match upload(multipart).await {
        Ok(write) => skills.update_library(&caller, id, write).await,
        Err(error) => Err(error),
    };
    library_result(&skills, &caller, Some(&existing), result).await
}

async fn library_result(
    skills: &SkillUseCases,
    caller: &Caller,
    existing: Option<&Skill>,
    result: AppResult<Skill>,
) -> AppResult<Response> {
    let UploadResult {
        draft,
        error,
        notice,
    } = upload_result(existing, result)?;
    Ok(Html(
        library_page(
            caller,
            skills,
            &PageQuery::default(),
            draft.as_ref(),
            error.as_deref(),
            notice,
        )
        .await?,
    )
    .into_response())
}

struct UploadResult {
    draft: Option<pages::SkillDraft>,
    error: Option<String>,
    notice: Option<&'static str>,
}

fn upload_result(existing: Option<&Skill>, result: AppResult<Skill>) -> AppResult<UploadResult> {
    match result {
        Ok(saved) => Ok(UploadResult {
            draft: Some(pages::SkillDraft::stored(&saved)),
            error: None,
            notice: Some("Skill directory imported."),
        }),
        Err(AppError::BadRequest(error) | AppError::Conflict(error)) => Ok(UploadResult {
            draft: existing.map(pages::SkillDraft::stored),
            error: Some(error),
            notice: None,
        }),
        Err(error) => Err(error),
    }
}

async fn company_download(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    download(
        skills
            .get_company(user.id, company, id)
            .await?
            .ok_or_else(missing)?,
    )
}
async fn library_download(
    State(skills): State<Arc<SkillUseCases>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    download(
        skills
            .library_skill_for(&caller, id)
            .await?
            .ok_or_else(missing)?,
    )
}
fn download(skill: Skill) -> AppResult<Response> {
    let bytes = super::archive::encode(&skill)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=skill-{}.zip", skill.id),
            ),
            (header::CACHE_CONTROL, "private, no-store".to_owned()),
        ],
        bytes,
    )
        .into_response())
}

async fn company_files(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let skill = skills
        .get_company(user.id, company, id)
        .await?
        .ok_or_else(missing)?;
    Ok(Html(pages::skill_bundle_files(&skill)))
}
async fn library_files(
    State(skills): State<Arc<SkillUseCases>>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> AppResult<Html<String>> {
    let skill = skills
        .library_skill_for(&caller, id)
        .await?
        .ok_or_else(missing)?;
    Ok(Html(pages::skill_bundle_files(&skill)))
}
