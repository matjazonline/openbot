use super::*;

pub(super) fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<SkillUseCases>: axum::extract::FromRef<S>,
    Arc<UserUseCases>: axum::extract::FromRef<S>,
    Arc<AppConfig>: axum::extract::FromRef<S>,
{
    Router::new()
        .route(
            "/api/skill-library",
            get(list_library_json).post(create_library_json),
        )
        .route(
            "/api/skill-library/{skill_id}",
            get(get_library_json)
                .put(update_library_json)
                .delete(delete_library_json),
        )
        .route(
            "/api/companies/{company_id}/skills",
            get(company_list).post(company_create),
        )
        .route(
            "/api/companies/{company_id}/skills/{skill_id}",
            get(company_get).put(company_update).delete(company_delete),
        )
}

async fn company_list(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> AppResult<Json<SkillPageResponse>> {
    Ok(Json(
        skills
            .list_company_page(user.id, company, page_request(&query)?)
            .await?
            .into(),
    ))
}

async fn company_get(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Skill>> {
    Ok(Json(
        skills
            .get_company(user.id, company, id)
            .await?
            .ok_or_else(missing)?,
    ))
}

async fn company_create(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(payload): Json<SkillPayload>,
) -> AppResult<(StatusCode, Json<Skill>)> {
    Ok((
        StatusCode::CREATED,
        Json(
            skills
                .create_company(user.id, company, payload.into())
                .await?,
        ),
    ))
}

async fn company_update(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<SkillPayload>,
) -> AppResult<Json<Skill>> {
    Ok(Json(
        skills
            .update_company(user.id, company, id, payload.into())
            .await?,
    ))
}

async fn company_delete(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    skills.delete_company(user.id, company, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn missing() -> AppError {
    AppError::NotFound("Skill not found in this company.".into())
}
