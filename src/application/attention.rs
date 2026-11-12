//! Application boundary for the human-operational queue and its business report.
//!
//! [`AttentionUseCases`] decides who may read which view; the HTTP routes only translate a request
//! into it and its answer into a page.

use std::{sync::Arc, time::Instant};

use async_trait::async_trait;
use uuid::Uuid;

use crate::app_error::AppResult;
use crate::application::ports::channel::ChannelPersistence;
use crate::domain::monitoring::MonitoringService;
use crate::entities::attention::AttentionCursor;
use crate::entities::attention::AttentionPage;
use crate::entities::attention::AttentionQuery;
use crate::entities::attention::AttentionView;
use crate::entities::attention::OperationalSummary;
use crate::entities::company::CompanyAccess;
use crate::entities::transport::PrincipalId;
use crate::entities::user::Viewer;
use crate::use_cases::channel::ChannelReaders;
use crate::use_cases::channel::readable_channels;
use crate::use_cases::company::CompanyPersistence;
use crate::use_cases::company::company_forbidden;
use crate::use_cases::company::readable_company;
use crate::use_cases::participant::ParticipantPersistence;

#[async_trait]
pub trait AttentionPersistence: Send + Sync {
    async fn list_attention(&self, query: AttentionQuery<'_>) -> AppResult<AttentionPage>;

    /// For owners and admins: company-associated sources are included.
    async fn operational_summary(
        &self,
        company_id: uuid::Uuid,
        visible_channel_ids: &[uuid::Uuid],
    ) -> AppResult<OperationalSummary>;
}

/// What a viewer may read in one company's queue: their access, the principal their work is
/// assigned to, and the channels they can see.
#[derive(Debug, Clone)]
pub struct AttentionReadContext {
    pub access: CompanyAccess,
    pub principal_id: PrincipalId,
    pub visible_channel_ids: Vec<Uuid>,
    /// The department each visible desk belongs to, for a one-department view.
    pub desk_departments: Vec<(Uuid, Option<crate::entities::department::DepartmentId>)>,
}

/// One page request of the queue, already parsed from the transport.
#[derive(Debug, Clone, Copy)]
pub struct AttentionListRequest<'a> {
    pub view: AttentionView,
    pub cursor: Option<&'a AttentionCursor>,
    pub limit: Option<usize>,
    /// Only this department's desks (01-departments 04C.1).
    pub department: Option<crate::entities::department::DepartmentId>,
}

/// A page of the queue together with the access it was read under, for the page shell.
#[derive(Debug, Clone)]
pub struct AttentionListing {
    pub access: CompanyAccess,
    pub page: AttentionPage,
    /// The desks the viewer can read, for naming their departments.
    pub readable_desks: Vec<Uuid>,
}

#[derive(Clone)]
pub struct AttentionUseCases {
    attention: Arc<dyn AttentionPersistence>,
    companies: Arc<dyn CompanyPersistence>,
    channels: Arc<dyn ChannelPersistence>,
    participants: Arc<dyn ParticipantPersistence>,
    monitoring: Arc<dyn MonitoringService>,
}

impl AttentionUseCases {
    pub fn new(
        attention: Arc<dyn AttentionPersistence>,
        companies: Arc<dyn CompanyPersistence>,
        channels: Arc<dyn ChannelPersistence>,
        participants: Arc<dyn ParticipantPersistence>,
        monitoring: Arc<dyn MonitoringService>,
    ) -> Self {
        Self {
            attention,
            companies,
            channels,
            participants,
            monitoring,
        }
    }

    /// The viewer's read context in one company. Someone who is nothing to the company, or who
    /// has no person principal in it, is told the company does not exist; a lookup failure is an
    /// error, never a refusal.
    pub async fn read_context(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<AttentionReadContext> {
        let access = readable_company(self.companies.as_ref(), viewer.user_id, company_id).await?;
        let principal_id = self
            .participants
            .access_context_for_user(company_id, viewer.user_id)
            .await?
            .and_then(|context| context.principal_id)
            .ok_or_else(crate::use_cases::company::company_not_found)?;
        let readers = ChannelReaders {
            companies: self.companies.as_ref(),
            channels: self.channels.as_ref(),
            participants: self.participants.as_ref(),
        };
        let visible = readable_channels(&readers, viewer, company_id).await?;
        Ok(AttentionReadContext {
            access,
            principal_id,
            visible_channel_ids: visible.iter().map(|channel| channel.id).collect(),
            desk_departments: visible
                .iter()
                .map(|channel| (channel.id, channel.department_id))
                .collect(),
        })
    }

    /// One page of the queue. The team view is the managers'; an ordinary member can see the
    /// company, so asking for it is a refusal (`Forbidden`) rather than a not-found.
    pub async fn list(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
        request: AttentionListRequest<'_>,
    ) -> AppResult<AttentionListing> {
        let context = self.read_context(viewer, company_id).await?;
        let manager = context.access.membership.manages_company_operations();
        if request.view == AttentionView::TeamWork && !manager {
            return Err(company_forbidden(
                "Only the company owner or an admin can see the team's work.",
            ));
        }
        // A one-department view reads only that department's visible desks, and no
        // company-associated items, which belong to no desk.
        let department_desks: Vec<Uuid>;
        let (visible_channel_ids, company_items) = match request.department {
            None => (context.visible_channel_ids.as_slice(), manager),
            Some(department) => {
                department_desks = context
                    .desk_departments
                    .iter()
                    .filter(|(_, owner)| *owner == Some(department))
                    .map(|(desk, _)| *desk)
                    .collect();
                (department_desks.as_slice(), false)
            }
        };
        let started = Instant::now();
        let page = self
            .attention
            .list_attention(AttentionQuery {
                company_id: context.access.company.id,
                principal_id: context.principal_id,
                visible_channel_ids,
                manager,
                company_items,
                view: request.view,
                cursor: request.cursor,
                limit: request.limit.unwrap_or(AttentionQuery::DEFAULT_LIMIT),
            })
            .await?;
        self.record_page_metrics(request.view, &page, started);
        Ok(AttentionListing {
            access: context.access,
            page,
            readable_desks: context.visible_channel_ids,
        })
    }

    /// The company's operational summary, for its owner and admins; a member is refused.
    pub async fn operational_summary(
        &self,
        viewer: &Viewer,
        company_id: Uuid,
    ) -> AppResult<OperationalSummary> {
        let context = self.read_context(viewer, company_id).await?;
        if !context.access.membership.manages_company_operations() {
            return Err(company_forbidden(
                "Only the company owner or an admin can see the operational summary.",
            ));
        }
        let started = Instant::now();
        let summary = self
            .attention
            .operational_summary(company_id, &context.visible_channel_ids)
            .await?;
        self.monitoring.record_histogram(
            "operational_summary_query_duration",
            started.elapsed().as_secs_f64() * 1_000.0,
            &[],
        );
        Ok(summary)
    }

    fn record_page_metrics(&self, view: AttentionView, page: &AttentionPage, started: Instant) {
        let labels = [("view", view_label(view))];
        self.monitoring.record_histogram(
            "attention_query_duration",
            started.elapsed().as_secs_f64() * 1_000.0,
            &labels,
        );
        self.monitoring.record_gauge(
            "attention_working_set_size",
            page.working_set_size as f64,
            &labels,
        );
        if page.truncated {
            self.monitoring
                .increment_counter("attention_working_set_truncated", 1, &labels);
        }
    }
}

const fn view_label(view: AttentionView) -> &'static str {
    match view {
        AttentionView::MyWork => "my_work",
        AttentionView::Unassigned => "unassigned",
        AttentionView::TeamWork => "team_work",
    }
}
