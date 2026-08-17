use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::value_objects::EmailAddress;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thread {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub participant_emails: Vec<EmailAddress>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}
