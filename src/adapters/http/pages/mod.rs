//! Server-rendered HTML for the whole app.
//!
//! One module per page area; everything is re-exported here so call sites stay
//! `pages::<name>`. Shared chrome (layout, alerts, markdown) lives in [`layout`].

use chrono::{DateTime, Utc};
use pulldown_cmark::{Options, Parser, html};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entities::{
    agent::Agent,
    approval::{ApprovalStatus, HumanApproval},
    channel::Channel,
    company::{Company, CompanyModelConnection},
    company_invite::CompanyInvite,
    company_member::{CompanyAccessRole, CompanyMember, CompanyMembership},
    correlation::CorrelationId,
    delivery::{DeliveryEntry, DeliveryFilter},
    memory::{
        MEMORY_READINESS_TIMEOUT_ERROR, MemoryConnection, MemoryConnectionReadiness,
        MemoryPersistenceMode, MemoryProviderKind, MemoryProvisioningPhase, MemoryRecallMode,
        default_memory_max_results,
    },
    message::{AttachmentMetadata, MessageDirection, MessageRole},
    message_view::{EmailReplyContext, MessageAuditView, ThreadMessageView},
    task::{
        BackgroundTask, ChainStage, TaskAttemptRecord, TaskAttemptRecordStatus, TaskBoardFilter,
        TaskChainBoard, TaskChainCard, TaskChainCounts, TaskChainDetail, TaskChainTaskDetail,
        TaskFilter, TaskStatus, ThreadActivity,
    },
    thread::Thread,
    transport::{DeliveryPartStatus, DeliveryPurpose, DeliveryStatus, TransportKind},
    user::User,
    value_objects::{AvatarUrl, EmailAddress, ModelName},
};
use crate::services::memory_provider::ConfiguredMemoryProviders;
use crate::use_cases::user::{AccountChangeKind, PendingChange};

fn parse_platform_address(
    value: &str,
    app_domain_name: &str,
) -> Option<(crate::entities::value_objects::CompanySlug, String)> {
    crate::adapters::protocols::email::EmailChannelSelectorParser::new(app_domain_name)
        .parse_platform_address(value)
}

mod agent_library_multi_select;
mod agent_settings;
mod agents;
mod approvals;
mod auth;
mod avatar_picker;
mod channel_settings;
mod channels;
mod chart;
mod companies;
mod company_settings;
mod dashboard;
mod dashboard_query_health;
mod dashboard_runtime;
mod deliveries;
mod fragment;
mod icon;
mod invite_settings;
mod layout;
mod mailbox;
mod model_connection;
mod onboarding;
mod profile;
mod schedules;
mod simulation;
mod skeleton;
mod task_board;
mod task_monitor;
mod tasks;
mod team_settings;
mod thread_activity;

pub use agent_library_multi_select::*;
pub use agent_settings::*;
pub use agents::*;
pub use approvals::*;
pub use auth::*;
pub use avatar_picker::*;
pub use channel_settings::*;
pub use channels::*;
pub use companies::*;
pub use company_settings::*;
pub use dashboard::*;
pub use deliveries::*;
pub use fragment::*;
pub use icon::*;
pub use invite_settings::*;
pub use layout::*;
pub use mailbox::*;
pub(crate) use model_connection::*;
pub use onboarding::*;
pub use profile::*;
pub use schedules::*;
pub use simulation::*;
pub(crate) use skeleton::{
    LIST_SKELETON, PANE_SKELETON, PANELS_SKELETON, THREAD_COLUMN_SKELETON, THREAD_ROWS_SKELETON,
    panels_placeholder, skeleton_script,
};
pub use task_board::*;
pub use task_monitor::*;
pub use tasks::*;
pub use team_settings::*;
pub use thread_activity::*;

/// Every timestamp the app stores is UTC. These helpers preserve that instant in a semantic
/// `datetime` attribute; the page shell replaces the UTC fallback text with the reader's local
/// date/time once it reaches their browser.
///
/// Pick by how much precision the reader actually needs: [`format_date`] for things that happened
/// on a day (signups, invites), [`format_date_time`] for things you scan by recency (threads,
/// messages), [`format_time`] for the queue views where seconds matter.
pub(crate) fn format_date(at: DateTime<Utc>) -> String {
    local_time(at, "date", "%b %d, %Y UTC")
}

pub(crate) fn format_date_time(at: DateTime<Utc>) -> String {
    local_time(at, "date-time", "%b %d, %Y %H:%M UTC")
}

pub(crate) fn format_time(at: DateTime<Utc>) -> String {
    local_time(at, "time", "%b %d, %H:%M:%S UTC")
}

fn local_time(at: DateTime<Utc>, precision: &str, fallback_format: &str) -> String {
    format!(
        r#"<time datetime="{}" data-local-time="{}">{}</time>"#,
        at.to_rfc3339(),
        precision,
        at.format(fallback_format)
    )
}

/// Converts semantic UTC timestamps after the initial load and after partial HTMX responses.
/// `Intl.DateTimeFormat` deliberately receives no `timeZone`, so it uses the browser's local one.
pub(crate) const LOCAL_TIME_SCRIPT: &str = r##"
        function localizeTimes(root) {
            var scope = root && root.querySelectorAll ? root : document;
            var times = Array.from(scope.querySelectorAll('time[data-local-time]:not([data-localized])'));
            if (scope.matches && scope.matches('time[data-local-time]:not([data-localized])')) {
                times.unshift(scope);
            }
            times.forEach(function (el) {
                var at = new Date(el.dateTime);
                if (Number.isNaN(at.getTime())) return;

                var precision = el.dataset.localTime;
                var options = precision === 'date'
                    ? { year: 'numeric', month: 'short', day: 'numeric' }
                    : precision === 'time'
                        ? { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit', timeZoneName: 'short' }
                        : { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', timeZoneName: 'short' };

                el.textContent = new Intl.DateTimeFormat(undefined, options).format(at);
                el.title = at.toISOString();
                el.dataset.localized = 'true';
            });
        }

        document.addEventListener('DOMContentLoaded', function () { localizeTimes(document); });
        document.addEventListener('htmx:afterSettle', function (event) { localizeTimes(event.target); });
        localizeTimes(document);
"##;

#[cfg(test)]
mod tests;
