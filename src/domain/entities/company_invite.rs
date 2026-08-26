use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::company_member::CompanyAccessRole;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompanyInvite {
    pub id: Uuid,
    pub company_id: Uuid,
    pub company_name: Option<String>,
    pub email: String,
    /// The access the account receives when this invitation is accepted.
    #[serde(default)]
    pub role: CompanyAccessRole,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invite_without_a_stored_role_keeps_the_member_default() {
        let invite: CompanyInvite = serde_json::from_value(serde_json::json!({
            "id": Uuid::nil(),
            "company_id": Uuid::nil(),
            "company_name": "Acme",
            "email": "member@example.com",
            "status": "pending",
            "created_at": "2026-08-24T00:00:00Z"
        }))
        .unwrap();

        assert_eq!(invite.role, CompanyAccessRole::Member);
    }
}
