//! Application boundary for actionable notification projection and reads.

use std::time::Duration;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::notification::{
        NotificationCensus, NotificationEvent, NotificationEventId, NotificationPage,
        NotificationProjection, NotificationProjectionResult,
    },
    transport::{ExecutionLease, NewStandaloneDelivery, WorkerId},
};

pub const NOTIFICATION_EVENT_CLAIM_BATCH: i64 = 32;
pub const NOTIFICATION_EVENT_LEASE_SECONDS: i64 = 30;
pub const NOTIFICATION_LIST_LIMIT: i64 = 100;

#[derive(Debug, Clone)]
pub struct ClaimedNotificationEvent {
    pub lease: ExecutionLease<NotificationEventId>,
    pub event: NotificationEvent,
}

#[derive(Debug)]
pub struct NotificationProjectionCommit<'a> {
    pub fence: &'a ExecutionLease<NotificationEventId>,
    pub expected: &'a NotificationProjection,
    pub email: Option<&'a NewStandaloneDelivery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationProjectionCommitOutcome {
    Applied,
    Stale,
    LeaseLost,
}

#[derive(Debug, Clone, Copy)]
pub struct NotificationProjectionFailure<'a> {
    pub fence: &'a ExecutionLease<NotificationEventId>,
    pub class: &'static str,
    pub detail: &'a str,
}

#[async_trait]
pub trait NotificationPersistence: Send + Sync {
    async fn claim_notification_events(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedNotificationEvent>>;

    /// Resolve current source responsibility and authorization. The completion call repeats this
    /// read under its write transaction, so a transfer or access revocation racing projection
    /// cannot enqueue a stale email.
    async fn resolve_notification_projection(
        &self,
        event: &NotificationEvent,
    ) -> AppResult<NotificationProjection>;

    async fn commit_notification_projection(
        &self,
        commit: NotificationProjectionCommit<'_>,
    ) -> AppResult<(
        NotificationProjectionCommitOutcome,
        NotificationProjectionResult,
    )>;

    async fn fail_notification_projection(
        &self,
        failure: NotificationProjectionFailure<'_>,
    ) -> AppResult<bool>;

    async fn reap_expired_notification_events(&self) -> AppResult<u64>;

    async fn notification_census(&self) -> AppResult<NotificationCensus>;

    async fn list_notifications(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: crate::entities::transport::PrincipalId,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<NotificationPage>;

    /// Resolve an opaque email deep-link id only when it belongs to the signed-in account. The
    /// caller uses the returned company to build the full current authorization context.
    async fn notification_company_for_user(
        &self,
        user_id: Uuid,
        notification_id: crate::entities::notification::NotificationId,
    ) -> AppResult<Option<Uuid>>;

    /// Resolve a deep link without mutating presentation state. Email navigation is a GET and
    /// therefore must remain safe; the in-app POST path uses `open_notification` to mark it read.
    async fn notification_href(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: crate::entities::transport::PrincipalId,
        visible_channel_ids: &[Uuid],
        notification_id: crate::entities::notification::NotificationId,
    ) -> AppResult<String>;

    /// Mark presentation state only after proving the alert still points to an authorized active
    /// source. The returned URL is derived from current identifiers, never stored content.
    async fn open_notification(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: crate::entities::transport::PrincipalId,
        visible_channel_ids: &[Uuid],
        notification_id: crate::entities::notification::NotificationId,
    ) -> AppResult<String>;
}
