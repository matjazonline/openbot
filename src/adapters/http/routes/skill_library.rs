//! Company-owned and operator-owned skill libraries.

use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    domain::entities::{
        skill::{MAX_SKILL_INSTRUCTIONS_BYTES, Skill},
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    use_cases::{
        skill::{
            MAX_SKILL_PAGE_SIZE, SkillCursor, SkillPage, SkillPageRequest, SkillUseCases,
            SkillWrite,
        },
        user::UserUseCases,
    },
};

use super::{
    agent_library::require_operator,
    ui::{load_account, workspace_user},
};

/// Room for the largest valid skill in its widest encoding (JSON's `\uXXXX` spends six bytes on
/// one), plus the short fields.
const SKILL_FORM_BODY_LIMIT: usize = 6 * MAX_SKILL_INSTRUCTIONS_BYTES + 64 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ui/companies/{company_id}/skills",
            get(company_section).post(create_company_skill),
        )
        .route(
            "/ui/companies/{company_id}/skills/library",
            get(company_library_browser),
        )
        .route(
            "/ui/companies/{company_id}/skills/from-library",
            post(copy_company_skill),
        )
        .route(
            "/ui/companies/{company_id}/skills/{skill_id}",
            get(company_skill)
                .put(update_company_skill)
                .delete(delete_company_skill),
        )
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
            "/ui/skill-library",
            get(library_workspace).post(create_library_skill),
        )
        .route(
            "/ui/skill-library/{skill_id}",
            get(library_skill)
                .put(update_library_skill)
                .delete(delete_library_skill),
        )
        .layer(DefaultBodyLimit::max(SKILL_FORM_BODY_LIMIT))
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageQuery {
    pub before_at: Option<DateTime<Utc>>,
    pub before_id: Option<Uuid>,
    #[serde(default)]
    pub trail: String,
}

fn page_request(query: &PageQuery) -> AppResult<SkillPageRequest> {
    let before = match (query.before_at, query.before_id) {
        (None, None) => None,
        (Some(updated_at), Some(id)) => Some(SkillCursor { updated_at, id }),
        _ => return Err(AppError::BadRequest("A skill cursor is incomplete.".into())),
    };
    if query.trail.len() > 8 * 1024 || query.trail.split(',').count() > 32 {
        return Err(AppError::BadRequest(
            "The skill page trail is too long.".into(),
        ));
    }
    Ok(SkillPageRequest {
        before,
        limit: MAX_SKILL_PAGE_SIZE,
    })
}

fn encode_cursor(cursor: SkillCursor) -> String {
    format!("{}~{}", cursor.updated_at.to_rfc3339(), cursor.id)
}

fn query_value(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn decode_cursor(value: &str) -> Option<SkillCursor> {
    let (at, id) = value.split_once('~')?;
    Some(SkillCursor {
        updated_at: DateTime::parse_from_rfc3339(at).ok()?.with_timezone(&Utc),
        id: Uuid::parse_str(id).ok()?,
    })
}

fn page_urls(base: &str, query: &PageQuery, page: &SkillPage) -> (Option<String>, Option<String>) {
    let mut trail = query
        .trail
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let previous = if query.before_at.is_some() {
        let prior = trail.pop();
        Some(match prior.as_deref().and_then(decode_cursor) {
            Some(cursor) => format!(
                "{base}&before_at={}&before_id={}&trail={}",
                query_value(&cursor.updated_at.to_rfc3339()),
                cursor.id,
                query_value(&trail.join(",")),
            ),
            None => format!("{base}&trail="),
        })
    } else {
        None
    };
    let next = page.next.map(|cursor| {
        if let (Some(at), Some(id)) = (query.before_at, query.before_id) {
            trail.push(encode_cursor(SkillCursor { updated_at: at, id }));
        } else {
            trail.push("first".to_string());
        }
        format!(
            "{base}&before_at={}&before_id={}&trail={}",
            query_value(&cursor.updated_at.to_rfc3339()),
            cursor.id,
            query_value(&trail.join(",")),
        )
    });
    (previous, next)
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillForm {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    instructions: String,
}

impl SkillForm {
    fn into_draft(self, id: Option<Uuid>) -> pages::SkillDraft {
        pages::SkillDraft {
            id,
            slug: self.slug,
            name: self.name,
            description: self.description,
            instructions: self.instructions,
        }
    }
}

fn write_from_draft(draft: &pages::SkillDraft) -> SkillWrite {
    SkillWrite {
        required_tools: None,
        slug: draft.slug.clone(),
        name: draft.name.clone(),
        description: draft.description.clone(),
        instructions: draft.instructions.clone(),
        created_by: None,
    }
}

async fn company_page(
    skills: &SkillUseCases,
    user_id: Uuid,
    company_id: Uuid,
    query: &PageQuery,
    draft: Option<&pages::SkillDraft>,
    error: Option<&str>,
    notice: Option<&str>,
) -> AppResult<String> {
    let page = skills
        .list_company_page(user_id, company_id, page_request(query)?)
        .await?;
    let base = format!("/ui/companies?company_id={company_id}&tab=skills");
    let (previous, next) = page_urls(&base, query, &page);
    Ok(pages::company_skills_section(&pages::SkillSection {
        company_id: Some(company_id),
        skills: &page.items,
        next_url: next.as_deref(),
        previous_url: previous.as_deref(),
        draft,
        error,
        notice,
        editable: true,
    }))
}

async fn library_page(
    skills: &SkillUseCases,
    query: &PageQuery,
    draft: Option<&pages::SkillDraft>,
    error: Option<&str>,
    notice: Option<&str>,
) -> AppResult<String> {
    let page = skills.list_library_page(page_request(query)?).await?;
    let (previous, next) = page_urls("/ui/skill-library?view=skills", query, &page);
    Ok(pages::company_skills_section(&pages::SkillSection {
        company_id: None,
        skills: &page.items,
        next_url: next.as_deref(),
        previous_url: previous.as_deref(),
        draft,
        error,
        notice,
        editable: true,
    }))
}

async fn company_section(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> AppResult<Html<String>> {
    skills.verify_company_owner(user.id, company_id).await?;
    Ok(Html(
        company_page(&skills, user.id, company_id, &query, None, None, None).await?,
    ))
}

async fn company_skill(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, skill_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    skills.verify_company_owner(user.id, company_id).await?;
    let skill = skills
        .get_company(user.id, company_id, skill_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Skill not found in this company.".into()))?;
    let draft = pages::SkillDraft::stored(&skill);
    Ok(Html(
        company_page(
            &skills,
            user.id,
            company_id,
            &PageQuery::default(),
            Some(&draft),
            None,
            None,
        )
        .await?,
    ))
}

async fn create_company_skill(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<SkillForm>,
) -> AppResult<Response> {
    save_company(&skills, user.id, company_id, None, form).await
}

async fn update_company_skill(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, skill_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SkillForm>,
) -> AppResult<Response> {
    // Scope before parsing a potentially invalid draft, so another tenant's id is never reflected
    // back as an editable form.
    skills
        .get_company(user.id, company_id, skill_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Skill not found in this company.".into()))?;
    save_company(&skills, user.id, company_id, Some(skill_id), form).await
}

async fn save_company(
    skills: &SkillUseCases,
    user_id: Uuid,
    company_id: Uuid,
    skill_id: Option<Uuid>,
    form: SkillForm,
) -> AppResult<Response> {
    let draft = form.into_draft(skill_id);
    let write = write_from_draft(&draft);
    let saved = match skill_id {
        Some(id) => skills.update_company(user_id, company_id, id, write).await,
        None => skills.create_company(user_id, company_id, write).await,
    };
    match saved {
        Ok(_) => Ok(Html(
            company_page(
                skills,
                user_id,
                company_id,
                &PageQuery::default(),
                None,
                None,
                Some("Skill saved."),
            )
            .await?,
        )
        .into_response()),
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => Ok(Html(
            company_page(
                skills,
                user_id,
                company_id,
                &PageQuery::default(),
                Some(&draft),
                Some(&message),
                None,
            )
            .await?,
        )
        .into_response()),
        Err(error) => Err(error),
    }
}

async fn delete_company_skill(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, skill_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    skills.delete_company(user.id, company_id, skill_id).await?;
    Ok(Html(
        company_page(
            &skills,
            user.id,
            company_id,
            &PageQuery::default(),
            None,
            None,
            Some("Skill deleted."),
        )
        .await?,
    ))
}

#[derive(Deserialize)]
struct LibraryPick {
    skill_id: Uuid,
}

async fn copy_company_skill(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(pick): Form<LibraryPick>,
) -> AppResult<Html<String>> {
    skills
        .create_skill_from_library(user.id, company_id, pick.skill_id)
        .await?;
    Ok(Html(
        company_page(
            &skills,
            user.id,
            company_id,
            &PageQuery::default(),
            None,
            None,
            Some("Library skill copied into this company."),
        )
        .await?,
    ))
}

async fn company_library_browser(
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    // This owner-scoped read deliberately uses the company list first as the authorization check;
    // the global projection itself carries no operator-only edit controls.
    skills.verify_company_owner(user.id, company_id).await?;
    let page = skills
        .list_library_page(SkillPageRequest {
            before: None,
            limit: MAX_SKILL_PAGE_SIZE,
        })
        .await?;
    Ok(Html(pages::library_skill_browser(company_id, &page.items)))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SkillPayload {
    slug: String,
    name: String,
    description: String,
    instructions: String,
}

impl From<SkillPayload> for SkillWrite {
    fn from(payload: SkillPayload) -> Self {
        Self {
            slug: payload.slug,
            name: payload.name,
            description: payload.description,
            instructions: payload.instructions,
            // Required workflow tools are edited in phase 09; keep the stored list.
            required_tools: None,
            created_by: None,
        }
    }
}

async fn operator(
    user: &AuthenticatedUser,
    users: &UserUseCases,
    config: &AppConfig,
) -> AppResult<()> {
    require_operator(user, users, config).await
}

async fn list_library_json(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Query(query): Query<PageQuery>,
) -> AppResult<Json<SkillPageResponse>> {
    operator(&user, &users, &config).await?;
    Ok(Json(SkillPageResponse::from(
        skills.list_library_page(page_request(&query)?).await?,
    )))
}

#[derive(Serialize)]
struct SkillPageResponse {
    items: Vec<Skill>,
    next: Option<SkillCursorResponse>,
}
#[derive(Serialize)]
struct SkillCursorResponse {
    updated_at: DateTime<Utc>,
    id: Uuid,
}
impl From<SkillPage> for SkillPageResponse {
    fn from(page: SkillPage) -> Self {
        Self {
            items: page.items,
            next: page.next.map(|cursor| SkillCursorResponse {
                updated_at: cursor.updated_at,
                id: cursor.id,
            }),
        }
    }
}

async fn get_library_json(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
) -> AppResult<Json<Skill>> {
    operator(&user, &users, &config).await?;
    skills
        .get_library(skill_id)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::NotFound("Library skill not found.".into()))
}

async fn create_library_json(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Json(payload): Json<SkillPayload>,
) -> AppResult<(StatusCode, Json<Skill>)> {
    operator(&user, &users, &config).await?;
    Ok((
        StatusCode::CREATED,
        Json(skills.create_library(payload.into()).await?),
    ))
}

async fn update_library_json(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
    Json(payload): Json<SkillPayload>,
) -> AppResult<Json<Skill>> {
    operator(&user, &users, &config).await?;
    Ok(Json(skills.update_library(skill_id, payload.into()).await?))
}

async fn delete_library_json(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    operator(&user, &users, &config).await?;
    skills.delete_library(skill_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn library_workspace(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> AppResult<Html<String>> {
    operator(&user, &users, &config).await?;
    let account = load_account(&users, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let shell_user = workspace_user(&account, &email, &config);
    let section = library_page(&skills, &query, None, None, None).await?;
    if crate::adapters::http::auth::is_htmx_request(&headers) {
        return Ok(Html(section));
    }
    let content = format!(
        r#"<main class="flex-1 overflow-auto p-6"><div class="mx-auto max-w-6xl"><div class="mb-5"><h1 class="text-2xl font-bold">Skill library</h1><p class="text-sm opacity-70">Global definitions companies can copy and adapt.</p></div>{section}</div></main>"#
    );
    Ok(Html(pages::ui_shell(&pages::UiShell {
        title: "Skill library",
        user: &shell_user,
        company: None,
        section: pages::UiSection::Dashboard,
        content: &content,
    })))
}

async fn library_skill(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    operator(&user, &users, &config).await?;
    let skill = skills
        .get_library(skill_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library skill not found.".into()))?;
    let draft = pages::SkillDraft::stored(&skill);
    Ok(Html(
        library_page(&skills, &PageQuery::default(), Some(&draft), None, None).await?,
    ))
}

async fn create_library_skill(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<SkillForm>,
) -> AppResult<Response> {
    operator(&user, &users, &config).await?;
    save_library(&skills, None, form).await
}

async fn update_library_skill(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
    Form(form): Form<SkillForm>,
) -> AppResult<Response> {
    operator(&user, &users, &config).await?;
    skills
        .get_library(skill_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library skill not found.".into()))?;
    save_library(&skills, Some(skill_id), form).await
}

async fn save_library(
    skills: &SkillUseCases,
    skill_id: Option<Uuid>,
    form: SkillForm,
) -> AppResult<Response> {
    let draft = form.into_draft(skill_id);
    let write = write_from_draft(&draft);
    let saved = match skill_id {
        Some(id) => skills.update_library(id, write).await,
        None => skills.create_library(write).await,
    };
    match saved {
        Ok(_) => Ok(Html(
            library_page(
                skills,
                &PageQuery::default(),
                None,
                None,
                Some("Skill saved."),
            )
            .await?,
        )
        .into_response()),
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => Ok(Html(
            library_page(
                skills,
                &PageQuery::default(),
                Some(&draft),
                Some(&message),
                None,
            )
            .await?,
        )
        .into_response()),
        Err(error) => Err(error),
    }
}

async fn delete_library_skill(
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(skill_id): Path<Uuid>,
) -> AppResult<Html<String>> {
    operator(&user, &users, &config).await?;
    skills.delete_library(skill_id).await?;
    Ok(Html(
        library_page(
            &skills,
            &PageQuery::default(),
            None,
            None,
            Some("Skill deleted."),
        )
        .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body limit must admit every skill validation accepts, in its widest encoding.
    #[test]
    fn the_body_limit_admits_the_largest_valid_skill_as_json() {
        let payload = SkillPayload {
            slug: "s".repeat(120),
            name: "\u{1}".repeat(120),
            description: "\u{1}".repeat(500),
            instructions: "\u{1}".repeat(MAX_SKILL_INSTRUCTIONS_BYTES),
        };
        let body = serde_json::to_vec(&payload).unwrap();
        assert!(body.len() <= SKILL_FORM_BODY_LIMIT, "{} bytes", body.len());
    }
}
