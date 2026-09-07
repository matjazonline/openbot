//! Supervised projector for durable actionable-notification events.

use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{
    app_error::{AppError, AppResult},
    application::notification::{
        ClaimedNotificationEvent, NOTIFICATION_EVENT_CLAIM_BATCH, NOTIFICATION_EVENT_LEASE_SECONDS,
        NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
        NotificationProjectionFailure,
    },
    domain::monitoring::MonitoringService,
    entities::{notification::NotificationDisposition, transport::DeliveryPurpose},
    infra::config::AppConfig,
    transport::{
        CanonicalContent, DeliveryComposer, DeliveryContext, EmailDeliveryContext, EmailThreading,
        StandaloneDeliveryRequest, WorkerId,
    },
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const ERROR_BACKOFF: Duration = Duration::from_secs(5);
const CENSUS_INTERVAL: Duration = Duration::from_secs(30);

pub struct NotificationWorker {
    persistence: Arc<dyn NotificationPersistence>,
    deliveries: DeliveryComposer,
    config: Arc<AppConfig>,
    monitoring: Arc<dyn MonitoringService>,
    worker_id: WorkerId,
}

impl NotificationWorker {
    pub fn new(
        persistence: Arc<dyn NotificationPersistence>,
        deliveries: DeliveryComposer,
        config: Arc<AppConfig>,
        monitoring: Arc<dyn MonitoringService>,
    ) -> Self {
        Self {
            persistence,
            deliveries,
            config,
            monitoring,
            worker_id: WorkerId::random(),
        }
    }

    pub async fn run(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        info!(worker_id = %self.worker_id, "Starting the actionable notification projector");
        let mut census = tokio::time::interval(CENSUS_INTERVAL);
        census.tick().await;
        loop {
            let pause = match self.drain_once().await {
                Ok(count) if count as i64 >= NOTIFICATION_EVENT_CLAIM_BATCH => Duration::ZERO,
                Ok(_) => POLL_INTERVAL,
                Err(error) => {
                    warn!(%error, "The notification projector could not claim work");
                    ERROR_BACKOFF
                }
            };
            tokio::select! {
                _ = shutdown.recv() => {
                    info!("Actionable notification projector shutting down");
                    return;
                }
                _ = tokio::time::sleep(pause) => {}
                _ = census.tick() => self.record_census().await,
            }
        }
    }

    pub(crate) async fn drain_once(&self) -> AppResult<usize> {
        let reaped = self.persistence.reap_expired_notification_events().await?;
        if reaped > 0 {
            self.monitoring.increment_counter(
                "notification_projection_leases_reaped_total",
                reaped,
                &[],
            );
        }
        let claimed = self
            .persistence
            .claim_notification_events(
                self.worker_id,
                Duration::from_secs(NOTIFICATION_EVENT_LEASE_SECONDS as u64),
                NOTIFICATION_EVENT_CLAIM_BATCH,
            )
            .await?;
        let count = claimed.len();
        for event in claimed {
            self.project(event).await;
        }
        Ok(count)
    }

    async fn project(&self, claimed: ClaimedNotificationEvent) {
        let projection = match self
            .persistence
            .resolve_notification_projection(&claimed.event)
            .await
        {
            Ok(projection) => projection,
            Err(error) => {
                self.fail(&claimed, "database", &error).await;
                return;
            }
        };
        let email = match self.compose_email(&projection) {
            Ok(email) => email,
            Err(error) => {
                self.fail(&claimed, "composition", &error).await;
                return;
            }
        };
        match self
            .persistence
            .commit_notification_projection(NotificationProjectionCommit {
                fence: &claimed.lease,
                expected: &projection,
                email: email.as_ref(),
            })
            .await
        {
            Ok((NotificationProjectionCommitOutcome::Applied, result)) => {
                self.monitoring.record_histogram(
                    "notification_event_projection_lag",
                    (chrono::Utc::now() - claimed.event.occurred_at)
                        .num_milliseconds()
                        .max(0) as f64,
                    &[("action", claimed.event.action_kind.as_str())],
                );
                if result.withdrawn > 0 {
                    self.monitoring.increment_counter(
                        "notifications_withdrawn_total",
                        result.withdrawn,
                        &[("action", claimed.event.action_kind.as_str())],
                    );
                }
                if let Some(seconds) = result.action_latency_seconds {
                    self.monitoring.record_histogram(
                        "notification_to_source_action_time",
                        seconds * 1_000.0,
                        &[("action", claimed.event.action_kind.as_str())],
                    );
                }
            }
            Ok((NotificationProjectionCommitOutcome::Stale, _)) => {}
            Ok((NotificationProjectionCommitOutcome::LeaseLost, _)) => warn!(
                event_id = %claimed.event.id,
                "Notification projection lost its execution fence"
            ),
            Err(error) => {
                self.monitoring.increment_counter(
                    "notification_enqueue_failures_total",
                    1,
                    &[("action", claimed.event.action_kind.as_str())],
                );
                self.fail(&claimed, "database", &error).await;
            }
        }
    }

    fn compose_email(
        &self,
        projection: &crate::entities::notification::NotificationProjection,
    ) -> AppResult<Option<crate::transport::NewStandaloneDelivery>> {
        if !projection.should_email() {
            return Ok(None);
        }
        let NotificationDisposition::Active(recipient) = &projection.disposition else {
            return Ok(None);
        };
        let subject = format!("Action required: {}", projection.event.action_kind.label());
        let link = format!(
            "{}/ui/notifications/{}/open",
            self.config.public_base_url(),
            projection.event.notification_id
        );
        let body = format!(
            "{} in {} / {}.\n\nOpen it securely: {}",
            projection.event.action_kind.label(),
            recipient.company_label,
            recipient.channel_label,
            link
        );
        let content = CanonicalContent::parse(subject, body)?;
        let source_key = format!(
            "actionable-notification:event:{}:user:{}",
            projection.event.id, recipient.user_id
        );
        self.deliveries
            .compose_standalone(StandaloneDeliveryRequest {
                correlation_id: recipient.correlation_id,
                purpose: DeliveryPurpose::Notification,
                source_key,
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: format!("notifications@{}", self.config.app_domain_name).into(),
                    from_name: Some("Mail Agents".into()),
                    recipient_to: recipient.email.clone(),
                    recipients_cc: Vec::new(),
                    threading: EmailThreading::Standalone,
                    relay: None,
                }),
            })
            .map(Some)
    }

    async fn fail(
        &self,
        claimed: &ClaimedNotificationEvent,
        class: &'static str,
        error: &AppError,
    ) {
        warn!(event_id = %claimed.event.id, %error, "Notification projection failed");
        let detail = error.to_string();
        if let Err(settle_error) = self
            .persistence
            .fail_notification_projection(NotificationProjectionFailure {
                fence: &claimed.lease,
                class,
                detail: &detail,
            })
            .await
        {
            warn!(event_id = %claimed.event.id, %settle_error, "Could not settle failed notification projection");
        }
    }

    async fn record_census(&self) {
        match self.persistence.notification_census().await {
            Ok(census) => {
                self.monitoring.record_gauge(
                    "active_notification_oldest_age_seconds",
                    census.oldest_active_age_seconds.unwrap_or(0.0),
                    &[],
                );
                self.monitoring.record_gauge(
                    "notification_projection_pending",
                    census.pending_events as f64,
                    &[],
                );
                self.monitoring.record_gauge(
                    "notification_projection_dead_letter",
                    census.dead_letter_events as f64,
                    &[],
                );
            }
            Err(error) => warn!(%error, "Could not sample actionable notification state"),
        }
    }
}
