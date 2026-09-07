use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::get,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{approval::HumanApproval, transport::PrincipalId, user::Viewer},
    use_cases::{
        approval::ApprovalUseCases, channel::ChannelUseCases, company::CompanyUseCases,
        thread::ThreadUseCases,
    },
};

use super::company_load_error;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/approvals/{token}", get(approval_link_handler))
        .route("/ui/approvals/{approval_id}", get(open_assigned_approval))
        .route(
            "/companies/{company_id}/channels/{channel_id}/approvals",
            get(list_channel_approvals_handler),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalQuery {
    pub action: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AssignedApprovalQuery {
    company_id: Uuid,
}

async fn open_assigned_approval(
    State(approval_use_cases): State<Arc<ApprovalUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    viewer: Viewer,
    Path(approval_id): Path<Uuid>,
    Query(query): Query<AssignedApprovalQuery>,
) -> AppResult<Redirect> {
    let context = super::attention::read_context(
        &company_use_cases,
        &channel_use_cases,
        &thread_use_cases,
        &viewer,
        query.company_id,
    )
    .await?;
    let approval = approval_use_cases
        .get_approval_by_id(query.company_id, approval_id)
        .await?;
    let location = assigned_approval_location(
        approval.as_ref(),
        query.company_id,
        &context.visible_channel_ids,
        context.principal_id,
        context.access.membership.manages_company_operations(),
    )?;
    Ok(Redirect::to(&location))
}

fn assigned_approval_location(
    approval: Option<&HumanApproval>,
    company_id: Uuid,
    visible_channel_ids: &[Uuid],
    viewer_principal_id: PrincipalId,
    manages_company_operations: bool,
) -> AppResult<String> {
    let approval = approval
        .filter(|approval| approval.company_id == company_id)
        .filter(|approval| visible_channel_ids.contains(&approval.channel_id))
        .ok_or_else(|| AppError::NotFound("Approval not found.".into()))?;
    if approval.approver_principal_id == Some(viewer_principal_id) {
        return Ok(format!("/approvals/{}", approval.token));
    }
    if manages_company_operations {
        return Ok(format!(
            "/ui?company_id={}&channel_id={}&thread_id={}",
            approval.company_id, approval.channel_id, approval.thread_id
        ));
    }
    Err(AppError::NotFound("Approval not found.".into()))
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
    if let Err(error) = company_use_cases.owned_company(user.id, company_id).await {
        return Html(pages::error_alert(&company_load_error(&error)));
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

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::entities::approval::ApprovalStatus;

    use super::*;

    fn approval_fixture(
        company_id: Uuid,
        channel_id: Uuid,
        approver: PrincipalId,
    ) -> HumanApproval {
        HumanApproval {
            id: Uuid::new_v4(),
            company_id,
            channel_id,
            thread_id: Uuid::new_v4(),
            task_id: Some(Uuid::new_v4()),
            step_key: "deploy".into(),
            approver_email: "reviewer@example.com".into(),
            approver_principal_id: Some(approver),
            action_type: "deploy".into(),
            action_title: "Approve deployment".into(),
            action_summary: "Confirm deployment".into(),
            payload: serde_json::json!({}),
            token: "secret-token".into(),
            status: ApprovalStatus::Pending,
            expires_at: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn assigned_approval_location_is_tenant_and_channel_scoped() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let approver = PrincipalId::random();
        let approval = approval_fixture(Uuid::new_v4(), channel_id, approver);

        assert!(matches!(
            assigned_approval_location(Some(&approval), company_id, &[channel_id], approver, true,),
            Err(AppError::NotFound(_))
        ));

        let approval = approval_fixture(company_id, Uuid::new_v4(), approver);
        assert!(matches!(
            assigned_approval_location(Some(&approval), company_id, &[channel_id], approver, true,),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn only_the_assignee_receives_the_bearer_approval_link() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let approver = PrincipalId::random();
        let approval = approval_fixture(company_id, channel_id, approver);

        assert_eq!(
            assigned_approval_location(
                Some(&approval),
                company_id,
                &[channel_id],
                approver,
                false,
            )
            .unwrap(),
            "/approvals/secret-token"
        );
        assert!(matches!(
            assigned_approval_location(
                Some(&approval),
                company_id,
                &[channel_id],
                PrincipalId::random(),
                false,
            ),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(
            assigned_approval_location(
                Some(&approval),
                company_id,
                &[channel_id],
                PrincipalId::random(),
                true,
            )
            .unwrap(),
            format!(
                "/ui?company_id={company_id}&channel_id={channel_id}&thread_id={}",
                approval.thread_id
            )
        );
    }
}
