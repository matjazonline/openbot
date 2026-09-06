use std::sync::Arc;

use axum::{
    Form, Router,
    extract::{Path, Query, State},
    response::Html,
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::{
        http::{app_state::AppState, auth::AuthenticatedUser, pages},
        protocols::email::EmailIdentity,
    },
    app_error::{AppError, AppResult},
    entities::{
        response_draft::{ExternalResponseReview, ResponseDraftId},
        transport::PrincipalId,
    },
    use_cases::{
        company::CompanyUseCases,
        response_review::{ResponseReviewUseCases, ReviewAction, ReviewCommand},
        thread::{ResponseReviewEditDraft, ThreadUseCases},
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/reviews", get(list_reviews))
        .route("/reviews/{draft_id}", get(review_detail))
        .route("/reviews/approve", axum::routing::post(approve))
        .route("/reviews/reject", axum::routing::post(reject))
        .route("/reviews/reassign", axum::routing::post(reassign))
        .route("/reviews/edit", axum::routing::post(edit))
        .route("/reviews/policy", get(review_policy))
        .route(
            "/reviews/policy/company",
            axum::routing::post(update_company_policy),
        )
        .route(
            "/reviews/policy/channel",
            axum::routing::post(update_channel_policy),
        )
}

#[derive(Debug, Deserialize)]
struct CompanyQuery {
    company_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct PolicyQuery {
    company_id: Uuid,
    channel_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CompanyPolicyForm {
    company_id: Uuid,
    channel_id: Uuid,
    policy: ExternalResponseReview,
}

#[derive(Debug, Deserialize)]
struct ChannelPolicyForm {
    company_id: Uuid,
    channel_id: Uuid,
    policy_override: String,
    preferred_reviewer_principal_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReviewActionForm {
    company_id: Uuid,
    draft_id: Uuid,
    expected_draft_version: u32,
    command_id: Uuid,
    rationale: Option<String>,
    feedback: Option<String>,
    reviewer_principal_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct ReviewEditForm {
    company_id: Uuid,
    draft_id: Uuid,
    expected_draft_version: u32,
    command_id: Uuid,
    subject: String,
    body: String,
    recipient_to: String,
    recipients_cc: Option<String>,
}

async fn reviewer_principal(
    companies: &CompanyUseCases,
    threads: &ThreadUseCases,
    user: &AuthenticatedUser,
    company_id: Uuid,
) -> AppResult<PrincipalId> {
    let access = companies
        .company_access(user.id, company_id)
        .await?
        .filter(|access| access.membership.is_team())
        .ok_or_else(crate::use_cases::company::company_not_found)?;
    threads
        .principal_access_for_user(access.company.id, user.id)
        .await?
        .and_then(|context| context.principal_id)
        .ok_or_else(|| AppError::NotFound("Response review not found.".into()))
}

async fn authorize_policy_manager(
    companies: &CompanyUseCases,
    user: &AuthenticatedUser,
    company_id: Uuid,
) -> AppResult<()> {
    let allowed = companies
        .company_access(user.id, company_id)
        .await?
        .is_some_and(|access| access.membership.manages_company_operations());
    if !allowed {
        return Err(crate::use_cases::company::company_not_found());
    }
    Ok(())
}

async fn review_policy(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<PolicyQuery>,
) -> AppResult<Html<String>> {
    authorize_policy_manager(&companies, &user, query.company_id).await?;
    let policy = reviews
        .policy(query.company_id, query.channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Channel not found.".into()))?;
    Ok(Html(pages::response_review_policy_page(
        query.company_id,
        query.channel_id,
        &policy,
    )))
}

async fn update_company_policy(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<CompanyPolicyForm>,
) -> AppResult<Html<String>> {
    authorize_policy_manager(&companies, &user, form.company_id).await?;
    reviews
        .set_company_policy(form.company_id, form.policy)
        .await?;
    Ok(Html(pages::response_review_policy_saved(
        form.company_id,
        form.channel_id,
    )))
}

async fn update_channel_policy(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ChannelPolicyForm>,
) -> AppResult<Html<String>> {
    authorize_policy_manager(&companies, &user, form.company_id).await?;
    let policy_override = match form.policy_override.as_str() {
        "inherit" => None,
        value => Some(value.parse().map_err(AppError::BadRequest)?),
    };
    reviews
        .set_channel_policy(
            form.company_id,
            form.channel_id,
            policy_override,
            form.preferred_reviewer_principal_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    Uuid::parse_str(value)
                        .map(PrincipalId::new)
                        .map_err(|_| AppError::BadRequest("Invalid reviewer principal ID.".into()))
                })
                .transpose()?,
        )
        .await?;
    Ok(Html(pages::response_review_policy_saved(
        form.company_id,
        form.channel_id,
    )))
}

async fn list_reviews(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let principal = reviewer_principal(&companies, &threads, &user, query.company_id).await?;
    let details = reviews.list_pending(query.company_id, principal).await?;
    Ok(Html(pages::response_review_list_page(
        &details,
        query.company_id,
    )))
}

async fn review_detail(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Path(draft_id): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    let principal = reviewer_principal(&companies, &threads, &user, query.company_id).await?;
    let detail = reviews
        .get(query.company_id, ResponseDraftId::new(draft_id), principal)
        .await?
        .ok_or_else(|| AppError::NotFound("Response review not found.".into()))?;
    Ok(Html(pages::response_review_detail_page(&detail)))
}

async fn approve(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ReviewActionForm>,
) -> AppResult<Html<String>> {
    execute(
        &reviews,
        &companies,
        &threads,
        &user,
        &form,
        ReviewAction::Approve {
            rationale: form.rationale.clone(),
        },
        "The exact approved version was published and queued once.",
    )
    .await
}

async fn reject(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ReviewActionForm>,
) -> AppResult<Html<String>> {
    execute(
        &reviews,
        &companies,
        &threads,
        &user,
        &form,
        ReviewAction::Reject {
            feedback: form.feedback.clone().unwrap_or_default(),
        },
        "The response was rejected and the feedback was recorded.",
    )
    .await
}

async fn reassign(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ReviewActionForm>,
) -> AppResult<Html<String>> {
    let reviewer = form
        .reviewer_principal_id
        .map(PrincipalId::new)
        .ok_or_else(|| AppError::BadRequest("A new reviewer is required.".into()))?;
    execute(
        &reviews,
        &companies,
        &threads,
        &user,
        &form,
        ReviewAction::Reassign {
            reviewer_principal_id: reviewer,
        },
        "The review was reassigned without changing task ownership.",
    )
    .await
}

async fn edit(
    State(reviews): State<Arc<ResponseReviewUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(threads): State<Arc<ThreadUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<ReviewEditForm>,
) -> AppResult<Html<String>> {
    let actor = reviewer_principal(&companies, &threads, &user, form.company_id).await?;
    let draft_id = ResponseDraftId::new(form.draft_id);
    let detail = reviews
        .get(form.company_id, draft_id, actor)
        .await?
        .filter(|detail| detail.draft.version == form.expected_draft_version)
        .ok_or_else(|| {
            AppError::Conflict("The draft version is stale; refresh and try again.".into())
        })?;
    let publication = reviews
        .publication(
            form.company_id,
            draft_id,
            form.expected_draft_version,
            actor,
        )
        .await?
        .ok_or_else(|| AppError::Conflict("The draft is no longer editable.".into()))?;
    let recipient_to = parse_email(&form.recipient_to)?;
    let recipients_cc = form
        .recipients_cc
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_email)
        .collect::<AppResult<Vec<_>>>()?;
    let replacement = threads
        .prepare_response_review_edit(ResponseReviewEditDraft {
            draft_id,
            current_version: form.expected_draft_version,
            company_id: form.company_id,
            channel_id: detail.draft.channel_id,
            thread_id: detail.draft.thread_id,
            task_id: detail.draft.task_id,
            source_handoff_generation: detail.draft.source_handoff_generation,
            actor_principal_id: actor,
            subject: &form.subject,
            text_body: &form.body,
            recipient_to,
            recipients_cc,
            evidence: detail
                .evidence
                .into_iter()
                .map(|item| item.evidence)
                .collect(),
            current_publication: &publication,
        })
        .await?;
    reviews
        .execute(ReviewCommand {
            company_id: form.company_id,
            draft_id,
            expected_draft_version: form.expected_draft_version,
            command_id: form.command_id,
            actor_principal_id: actor,
            action: ReviewAction::Edit {
                replacement: Box::new(replacement),
            },
        })
        .await?;
    Ok(Html(pages::response_review_action_result(
        "A new immutable version was created; the previous approval target is invalid.",
        form.company_id,
    )))
}

fn parse_email(value: &str) -> AppResult<crate::entities::value_objects::EmailAddress> {
    let identity = EmailIdentity::parse(crate::entities::value_objects::EmailAddress::from(value))
        .map_err(|_| AppError::BadRequest(format!("Invalid email address: {value}")))?;
    Ok(crate::entities::value_objects::EmailAddress::from(
        identity.subject().as_str(),
    ))
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    reviews: &ResponseReviewUseCases,
    companies: &CompanyUseCases,
    threads: &ThreadUseCases,
    user: &AuthenticatedUser,
    form: &ReviewActionForm,
    action: ReviewAction,
    message: &str,
) -> AppResult<Html<String>> {
    let actor = reviewer_principal(companies, threads, user, form.company_id).await?;
    reviews
        .execute(ReviewCommand {
            company_id: form.company_id,
            draft_id: ResponseDraftId::new(form.draft_id),
            expected_draft_version: form.expected_draft_version,
            command_id: form.command_id,
            actor_principal_id: actor,
            action,
        })
        .await?;
    Ok(Html(pages::response_review_action_result(
        message,
        form.company_id,
    )))
}
