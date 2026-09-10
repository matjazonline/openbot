//! Runtime facts are supplied by the application, never inferred from model arguments.
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::transport::RecipientRole;

pub trait RuntimeClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl RuntimeClock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
pub struct RuntimeContext {
    pub company_id: Uuid,
    pub agent_id: Uuid,
    pub recipient_role: RecipientRole,
    pub timezone: chrono_tz::Tz,
}
