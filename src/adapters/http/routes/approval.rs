use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    use_cases::{approval::ApprovalUseCases, company::CompanyUseCases},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/approvals/{token}", get(approval_link_handler))
        .route(
            "/companies/{company_id}/channels/{channel_id}/approvals",
            get(list_channel_approvals_handler),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalQuery {
    pub action: Option<String>,
}

async fn approval_link_handler(
    State(approval_use_cases): State<Arc<ApprovalUseCases>>,
    Path(token): Path<String>,
    Query(query): Query<ApprovalQuery>,
) -> impl IntoResponse {
    if let Some(ref act) = query.action {
        match approval_use_cases.process_link_action(&token, act).await {
            Ok((approval, msg)) => {
                let page_title = match approval.status {
                    crate::entities::approval::ApprovalStatus::Approved => "Action Confirmed",
                    crate::entities::approval::ApprovalStatus::Rejected => "Action Rejected",
                    crate::entities::approval::ApprovalStatus::Expired => "Link Expired",
                    _ => "Approval Processing",
                };
                Html(pages::approval_result_page(page_title, &approval, &msg))
            }
            Err(err) => Html(pages::error_alert(&format!(
                "Approval processing error: {err}"
            ))),
        }
    } else {
        match approval_use_cases.get_approval_by_token(&token).await {
            Ok(Some(approval)) => Html(pages::approval_details_page(&approval)),
            Ok(None) => Html(pages::error_alert("Approval request token not found.")),
            Err(err) => Html(pages::error_alert(&format!("Approval error: {err}"))),
        }
    }
}

async fn list_channel_approvals_handler(
    State(approval_use_cases): State<Arc<ApprovalUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let is_owner = company_use_cases
        .get_company(company_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|company| company.user_id == user.id);
    if !is_owner {
        return Html(pages::error_alert("Company not found."));
    }

    match approval_use_cases
        .list_channel_approvals(company_id, channel_id)
        .await
    {
        Ok(list) => Html(pages::channel_approvals_fragment(&list)),
        Err(err) => Html(pages::error_alert(&format!(
            "Failed to list approvals: {err}"
        ))),
    }
}
