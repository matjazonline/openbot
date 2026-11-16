//! The deployment values application orchestration actually reads.
use crate::entities::value_objects::{MailDomain, PublicBaseUrl};
#[derive(Debug, Clone)]
pub struct ThreadSettings {
    pub mail_domain: MailDomain,
    pub max_spam_score: f64,
}
#[derive(Debug, Clone)]
pub struct NotificationMailSettings {
    pub mail_domain: MailDomain,
    pub public_base_url: PublicBaseUrl,
    pub escalation_ack_hours: crate::application::workflow::binding::EscalationAckHours,
}
