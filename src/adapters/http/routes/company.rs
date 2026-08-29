use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{company::Company, memory::MemoryConnection, value_objects::AvatarUrl},
    services::memory_provider::{ConfiguredMemoryProviders, SelectedMemoryProvider},
    use_cases::{
        company::{CompanyUseCases, CompanyWrite},
        memory::MemoryUseCases,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/companies", get(list_companies).post(create_company))
        .route(
            "/companies/{id}",
            put(update_company).delete(delete_company),
        )
        .route("/companies/{id}/edit", get(edit_company_form))
        .route("/companies/{id}/cancel", get(cancel_company_edit))
        .route(
            "/api/companies",
            get(list_companies_json).post(create_company_json),
        )
        .route(
            "/api/companies/{id}",
            put(update_company_json).delete(delete_company_json),
        )
        .route(
            "/api/companies/{id}/memory",
            get(memory_status_json).post(retry_memory_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompanyForm {
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enable_llm_spam_guardrail: Option<bool>,
    /// The company's picture. A save carries what it was sent, so the edit form keeps the stored
    /// URL in a hidden field rather than dropping the picture on every rename.
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub memory_provider: Option<String>,
}

impl CompanyForm {
    /// The submitted company as a write, with the avatar parsed rather than taken as typed --
    /// it ends up in an `<img src>` on every page that shows the company.
    fn write(&self) -> AppResult<CompanyWrite> {
        Ok(CompanyWrite {
            name: self.name.clone(),
            slug: self.slug.clone(),
            api_key: self.api_key.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            enable_llm_spam_guardrail: self.enable_llm_spam_guardrail,
            memory_provider: None,
            avatar_url: parsed_avatar(self.avatar_url.as_deref())?,
        })
    }
}

/// A submitted avatar field, refused rather than stored when it is not a URL a page may render.
fn parsed_avatar(submitted: Option<&str>) -> AppResult<Option<AvatarUrl>> {
    match submitted {
        Some(value) => AvatarUrl::parse(value).map_err(AppError::BadRequest),
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompanyResponse {
    pub success: bool,
    pub company: Company,
    pub memory: Option<MemoryStatusResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStatusResponse {
    pub provider: String,
    pub readiness: String,
    pub last_error: Option<String>,
}

impl From<MemoryConnection> for MemoryStatusResponse {
    fn from(connection: MemoryConnection) -> Self {
        Self {
            provider: connection.provider.as_str().into(),
            readiness: connection.readiness.as_str().into(),
            last_error: connection.last_error,
        }
    }
}

/// Reject an unusable provider before the company write, so a bad selection never leaves a
/// half-applied company behind.
fn validate_memory_provider(
    value: Option<&str>,
    configured: &ConfiguredMemoryProviders,
) -> AppResult<()> {
    configured
        .select(value)
        .map(|_| ())
        .map_err(AppError::BadRequest)
}

async fn apply_memory_provider(
    memory: &MemoryUseCases,
    user_id: Uuid,
    company_id: Uuid,
    value: Option<&str>,
) -> AppResult<Option<MemoryConnection>> {
    match memory
        .configured()
        .select(value)
        .map_err(AppError::BadRequest)?
    {
        SelectedMemoryProvider::Provider(kind) => memory
            .select_provider(user_id, company_id, kind)
            .await
            .map(Some),
        SelectedMemoryProvider::Disabled => {
            memory.disable(user_id, company_id).await?;
            Ok(None)
        }
    }
}

/// GET /companies - Full HTML page listing all user companies (Protected).
#[instrument(skip(company_use_cases, memory_use_cases, user))]
async fn list_companies(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let companies = company_use_cases
        .list_user_companies(user.id)
        .await
        .unwrap_or_default();

    Html(pages::companies_page(
        &companies,
        memory_use_cases.configured(),
    ))
}

/// POST /companies - HTMX create company form submission (Protected).
#[instrument(skip(company_use_cases, memory_use_cases, user, form))]
async fn create_company(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<CompanyForm>,
) -> impl IntoResponse {
    let created = match validate_memory_provider(
        form.memory_provider.as_deref(),
        memory_use_cases.configured(),
    )
    .and_then(|()| form.write())
    {
        Ok(write) => company_use_cases.create_company(user.id, write).await,
        Err(err) => Err(err),
    };

    match created {
        Ok(company) => {
            if let Err(err) = apply_memory_provider(
                &memory_use_cases,
                user.id,
                company.id,
                form.memory_provider.as_deref(),
            )
            .await
            {
                return Html(pages::error_alert(&format!(
                    "Company created, but memory setup failed: {err}"
                )));
            }
            let companies = company_use_cases
                .list_user_companies(user.id)
                .await
                .unwrap_or_default();
            Html(pages::company_list_fragment(&companies))
        }
        Err(err) => {
            let error_html = pages::error_alert(&format!("Failed to create company: {err}"));
            let companies = company_use_cases
                .list_user_companies(user.id)
                .await
                .unwrap_or_default();
            Html(format!(
                "{}{}",
                error_html,
                pages::company_list_fragment(&companies)
            ))
        }
    }
}

/// GET /companies/{id}/edit - Returns inline edit form fragment (Protected).
#[instrument(skip(company_use_cases, memory_use_cases, _user))]
async fn edit_company_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Ok(Some(company)) = company_use_cases.get_company(id).await
        && company.user_id == _user.id
    {
        Html(pages::company_edit_fragment(
            &company,
            memory_use_cases.configured(),
        ))
    } else {
        Html(pages::error_alert("Company not found."))
    }
}

/// GET /companies/{id}/cancel - Cancels edit and returns single row fragment (Protected).
#[instrument(skip(company_use_cases, _user))]
async fn cancel_company_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Ok(Some(company)) = company_use_cases.get_company(id).await
        && company.user_id == _user.id
    {
        Html(pages::company_row_fragment(&company))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{id} - Handles HTMX company update form submission (Protected).
#[instrument(skip(company_use_cases, memory_use_cases, _user, form))]
async fn update_company(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<CompanyForm>,
) -> impl IntoResponse {
    let saved = match validate_memory_provider(
        form.memory_provider.as_deref(),
        memory_use_cases.configured(),
    )
    .and_then(|()| form.write())
    {
        Ok(write) => {
            company_use_cases
                .update_company_for_user(_user.id, id, write)
                .await
        }
        Err(err) => Err(err),
    };

    match saved {
        Ok(company) => match apply_memory_provider(
            &memory_use_cases,
            _user.id,
            company.id,
            form.memory_provider.as_deref(),
        )
        .await
        {
            Ok(_) => Html(pages::company_row_fragment(&company)),
            Err(err) => Html(pages::error_alert(&format!("Memory update failed: {err}"))),
        },
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{id} - Handles HTMX company deletion (Protected).
#[instrument(skip(company_use_cases, _user))]
async fn delete_company(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let _ = company_use_cases
        .delete_company_for_user(_user.id, id)
        .await;
    Html(String::new())
}

/// JSON API: List companies for active user (Protected).
async fn list_companies_json(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
) -> AppResult<impl IntoResponse> {
    let companies = company_use_cases.list_user_companies(user.id).await?;
    Ok((StatusCode::OK, Json(companies)))
}

/// JSON API: Create company (Protected).
async fn create_company_json(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    user: AuthenticatedUser,
    Json(payload): Json<CompanyForm>,
) -> AppResult<impl IntoResponse> {
    validate_memory_provider(
        payload.memory_provider.as_deref(),
        memory_use_cases.configured(),
    )?;
    let company = company_use_cases
        .create_company(user.id, payload.write()?)
        .await?;
    let memory = apply_memory_provider(
        &memory_use_cases,
        user.id,
        company.id,
        payload.memory_provider.as_deref(),
    )
    .await?
    .map(Into::into);
    Ok((
        StatusCode::CREATED,
        Json(CompanyResponse {
            success: true,
            company,
            memory,
        }),
    ))
}

/// JSON API: Update company (Protected).
async fn update_company_json(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<CompanyForm>,
) -> AppResult<impl IntoResponse> {
    validate_memory_provider(
        payload.memory_provider.as_deref(),
        memory_use_cases.configured(),
    )?;
    let company = company_use_cases
        .update_company_for_user(_user.id, id, payload.write()?)
        .await?;
    let memory = apply_memory_provider(
        &memory_use_cases,
        _user.id,
        company.id,
        payload.memory_provider.as_deref(),
    )
    .await?
    .map(Into::into);
    Ok((
        StatusCode::OK,
        Json(CompanyResponse {
            success: true,
            company,
            memory,
        }),
    ))
}

async fn memory_status_json(
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Option<MemoryStatusResponse>>> {
    Ok(Json(
        memory_use_cases.status(user.id, id).await?.map(Into::into),
    ))
}

async fn retry_memory_json(
    State(memory_use_cases): State<Arc<MemoryUseCases>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    memory_use_cases.retry(user.id, id).await?;
    Ok(StatusCode::ACCEPTED)
}

/// JSON API: Delete company (Protected).
async fn delete_company_json(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    company_use_cases
        .delete_company_for_user(_user.id, id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::entities::memory::MemoryProviderKind;

    /// A deployment with every provider available, so the forms under test render the full list.
    fn configured_providers() -> ConfiguredMemoryProviders {
        ConfiguredMemoryProviders::from_iter(MemoryProviderKind::ALL)
    }

    #[test]
    fn companies_page_renders_htmx_crud_elements() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test-corp".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let page_html = pages::companies_page(&[company.clone()], &configured_providers());
        assert!(page_html.contains("Test Corp"));
        assert!(page_html.contains("/test-corp"));
        assert!(page_html.contains("hx-post=\"/companies\""));
        assert!(page_html.contains("id=\"company-form-toggle\""));
        assert!(page_html.contains("id=\"company-form-card\" class=\"hidden"));
        assert!(page_html.contains("aria-expanded=\"false\""));
        assert!(page_html.contains("id=\"nav-channels\""));
        assert!(page_html.find(">Channels</a>") < page_html.find(">Agents</a>"));
        assert!(page_html.find(">Agents</a>") < page_html.find(">Account</summary>"));
        assert!(page_html.contains(">Account</summary>"));
        assert!(page_html.find(">Account</summary>") < page_html.find(">Companies</a>"));
        assert!(page_html.find(">Companies</a>") < page_html.find(">My Invites</a>"));
        assert!(page_html.contains("href=\"/ui/invites\""));
        assert!(page_html.contains("action=\"/logout\""));
        assert!(page_html.contains(r##"data-action="select-company""##));

        let edit_fragment = pages::company_edit_fragment(&company, &configured_providers());
        assert!(edit_fragment.contains("hx-put="));
        assert!(edit_fragment.contains("value=\"Test Corp\""));
    }

    #[test]
    fn a_classic_save_carries_the_picture_it_was_sent_and_refuses_one_no_page_could_render() {
        let form = |avatar_url: Option<&str>| CompanyForm {
            name: "Test Corp".to_string(),
            slug: "test-corp".to_string(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: avatar_url.map(str::to_string),
            memory_provider: None,
        };

        let kept = form(Some("https://cdn.example.com/acme.png"))
            .write()
            .expect("an http URL is a picture a page may show");
        assert_eq!(
            kept.avatar_url,
            Some(AvatarUrl::from("https://cdn.example.com/acme.png"))
        );

        // The page this form lives on has no picker, so it sends the stored URL in a hidden
        // field -- and a company saved from a form that carries none has no picture.
        assert_eq!(form(None).write().expect("no picture").avatar_url, None);
        assert_eq!(form(Some("")).write().expect("no picture").avatar_url, None);

        // A tampered field is refused rather than stored: it ends up in an `<img src>`.
        assert!(form(Some("javascript:alert(1)")).write().is_err());

        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Test Corp".to_string(),
            slug: "test-corp".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: Some(AvatarUrl::from("https://cdn.example.com/acme.png")),
            memory_provider: None,
            created_at: Utc::now(),
        };
        assert!(pages::company_edit_fragment(&company, &configured_providers()).contains(
            r#"<input type="hidden" name="avatar_url" value="https://cdn.example.com/acme.png">"#
        ));
    }

    #[test]
    fn cached_company_client_nav_and_row_rendering() {
        let cid = Uuid::new_v4();
        let company = Company {
            id: cid,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme-corp".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let row_html = pages::company_row_fragment(&company);
        assert!(row_html.contains(&format!(
            r##"data-action="select-company" data-company-id="{cid}""##
        )));
        assert!(row_html.contains(&format!("selected-badge-{}", cid)));

        let base_html = pages::base_layout("Test Title", "<p>Test Content</p>");
        assert!(base_html.contains("id=\"nav-channels\""));
        assert!(base_html.contains("action=\"/logout\""));
        assert!(!base_html.contains(">Sign In</a>"));
        assert!(!base_html.contains(">Sign Up</a>"));
        assert!(base_html.contains("/assets/app.js"));

        let script = pages::application_javascript();
        assert!(script.contains("localStorage.getItem('cached_company_id')"));
        assert!(script.contains("autoDetectAndSyncCompany"));
    }
}
