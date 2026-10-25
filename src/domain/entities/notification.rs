//! Recipient-facing alerts projected from authoritative operational sources.
//!
//! A notification never owns the work it points at. `read_at` only records presentation state;
//! `state` is changed by reconciliation with the source workflow.

use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{correlation::CorrelationId, transport::uuid_id};

uuid_id!(NotificationId);
uuid_id!(NotificationEventId);

/// What a notification points at. A delivery is the only source (p08 D4.3): workflow decisions
/// notify by email, never with an in-app row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSourceKind {
    Delivery,
}

impl NotificationSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
        }
    }
}

impl FromStr for NotificationSourceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "delivery" => Ok(Self::Delivery),
            _ => Err(format!("invalid notification source kind '{value}'")),
        }
    }
}

impl fmt::Display for NotificationSourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationActionKind {
    DeliveryFailure,
}

impl NotificationActionKind {
    pub const ALL: [Self; 1] = [Self::DeliveryFailure];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeliveryFailure => "delivery_failure",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::DeliveryFailure => "Delivery needs failure recovery",
        }
    }

    pub const fn email_preference(self, preferences: NotificationPreferences) -> bool {
        match self {
            Self::DeliveryFailure => preferences.delivery_failure_email_enabled,
        }
    }
}

impl FromStr for NotificationActionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "delivery_failure" => Ok(Self::DeliveryFailure),
            _ => Err(format!("invalid notification action kind '{value}'")),
        }
    }
}

impl fmt::Display for NotificationActionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationState {
    Active,
    Resolved,
    Withdrawn,
}

impl NotificationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Resolved => "resolved",
            Self::Withdrawn => "withdrawn",
        }
    }
}

impl FromStr for NotificationState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "resolved" => Ok(Self::Resolved),
            "withdrawn" => Ok(Self::Withdrawn),
            _ => Err(format!("invalid notification state '{value}'")),
        }
    }
}

/// A person's email preferences (p08 D4.3): delivery failures assigned to them, and workflow
/// decisions that need their review. In-app alerts are always on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationPreferences {
    pub delivery_failure_email_enabled: bool,
    /// Honoured by the decision-notice composer, not by this notification model (P08-Q8).
    pub decision_email_enabled: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            delivery_failure_email_enabled: true,
            decision_email_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionableNotification {
    pub id: NotificationId,
    pub company_id: Uuid,
    pub company_label: String,
    pub channel_id: Uuid,
    pub channel_label: String,
    pub source_kind: NotificationSourceKind,
    pub source_id: Uuid,
    pub action_kind: NotificationActionKind,
    pub source_generation: u64,
    pub state: NotificationState,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub state_changed_at: DateTime<Utc>,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotificationPage {
    pub items: Vec<ActionableNotification>,
    pub unread_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEvent {
    pub id: NotificationEventId,
    pub notification_id: NotificationId,
    pub company_id: Uuid,
    pub source_kind: NotificationSourceKind,
    pub source_id: Uuid,
    pub action_kind: NotificationActionKind,
    pub source_generation: u64,
    pub actor_principal_id: Option<crate::entities::transport::PrincipalId>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecipient {
    pub principal_id: crate::entities::transport::PrincipalId,
    pub user_id: Uuid,
    pub email: crate::entities::value_objects::EmailAddress,
    pub company_label: String,
    pub channel_label: String,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub correlation_id: CorrelationId,
    pub email_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationDisposition {
    Active(NotificationRecipient),
    Resolved,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationProjection {
    pub event: NotificationEvent,
    pub disposition: NotificationDisposition,
}

impl NotificationProjection {
    pub fn should_email(&self) -> bool {
        let NotificationDisposition::Active(recipient) = &self.disposition else {
            return false;
        };
        recipient.email_enabled && self.event.actor_principal_id != Some(recipient.principal_id)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NotificationProjectionResult {
    pub created: u64,
    pub resolved: u64,
    pub withdrawn: u64,
    pub action_latency_seconds: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NotificationCensus {
    pub oldest_active_age_seconds: Option<f64>,
    pub pending_events: u64,
    pub dead_letter_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::transport::PrincipalId;

    #[test]
    fn self_triggered_actions_keep_in_app_but_suppress_email() {
        let principal = PrincipalId::random();
        let projection = NotificationProjection {
            event: NotificationEvent {
                id: NotificationEventId::random(),
                notification_id: NotificationId::random(),
                company_id: Uuid::new_v4(),
                source_kind: NotificationSourceKind::Delivery,
                source_id: Uuid::new_v4(),
                action_kind: NotificationActionKind::DeliveryFailure,
                source_generation: 1,
                actor_principal_id: Some(principal),
                occurred_at: Utc::now(),
            },
            disposition: NotificationDisposition::Active(NotificationRecipient {
                principal_id: principal,
                user_id: Uuid::new_v4(),
                email: "person@example.com".into(),
                company_label: "Example".into(),
                channel_label: "Support".into(),
                channel_id: Uuid::new_v4(),
                thread_id: None,
                run_id: None,
                correlation_id: CorrelationId::new(),
                email_enabled: true,
            }),
        };
        assert!(!projection.should_email());
        assert!(matches!(
            projection.disposition,
            NotificationDisposition::Active(_)
        ));
    }

    #[test]
    fn a_delivery_failure_emails_only_when_its_preference_is_on() {
        let enabled = NotificationPreferences::default();
        let disabled = NotificationPreferences {
            delivery_failure_email_enabled: false,
            ..enabled
        };
        for kind in NotificationActionKind::ALL {
            assert!(kind.email_preference(enabled), "{kind:?}");
            assert!(!kind.email_preference(disabled), "{kind:?}");
        }
    }

    /// The legacy runtime's sources and actions went with it (p08 D4.3).
    #[test]
    fn only_the_delivery_source_and_action_parse() {
        assert_eq!("delivery".parse(), Ok(NotificationSourceKind::Delivery));
        assert_eq!(
            "delivery_failure".parse(),
            Ok(NotificationActionKind::DeliveryFailure)
        );
        for retired in ["task", "handoff", "response_review", "delegation"] {
            assert!(
                retired.parse::<NotificationSourceKind>().is_err(),
                "{retired}"
            );
        }
        for retired in [
            "assignment",
            "response_review",
            "delegation_timeout",
            "task_failure",
        ] {
            assert!(
                retired.parse::<NotificationActionKind>().is_err(),
                "{retired}"
            );
        }
    }
}
