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
    pub role: CompanyAccessRole,
    pub access_level: crate::entities::access_level::AccessLevel,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_level_invite_payload_requires_current_fields() {
        let current = serde_json::json!({
            "id": Uuid::new_v4(), "company_id": Uuid::new_v4(), "company_name": null,
            "email": "invite@example.test", "role": "member", "access_level": 0,
            "status": "pending", "created_at": chrono::Utc::now(),
        });
        let invite: CompanyInvite = serde_json::from_value(current.clone()).unwrap();
        assert_eq!(invite.role, CompanyAccessRole::Member);
        assert_eq!(
            invite.access_level,
            crate::entities::access_level::AccessLevel::PUBLIC
        );
        for field in ["role", "access_level"] {
            let mut missing = current.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<CompanyInvite>(missing).is_err(),
                "{field}"
            );
        }
        let mut invalid = current;
        invalid["access_level"] = serde_json::json!(10);
        assert!(serde_json::from_value::<CompanyInvite>(invalid).is_err());
    }
}
