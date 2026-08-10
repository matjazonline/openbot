use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite, company_member::CompanyMember, user::User,
    },
    use_cases::company::CompanyPersistence,
};

#[async_trait]
pub trait CompanyInvitePersistence: Send + Sync {
    async fn create_invite(&self, company_id: Uuid, email: &str) -> AppResult<CompanyInvite>;
    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>>;
    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>>;
    async fn update_invite_email(&self, id: Uuid, new_email: &str) -> AppResult<CompanyInvite>;
    async fn update_invite_status(&self, id: Uuid, status: &str) -> AppResult<CompanyInvite>;
    async fn delete_invite(&self, id: Uuid) -> AppResult<()>;
    async fn list_invites_by_email(&self, email: &str) -> AppResult<Vec<CompanyInvite>>;
    async fn add_member(&self, company_id: Uuid, user_id: Uuid, role: &str) -> AppResult<CompanyMember>;
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>>;
    async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()>;
}

#[derive(Clone)]
pub struct CompanyInviteUseCases {
    company_persistence: Arc<dyn CompanyPersistence>,
    invite_persistence: Arc<dyn CompanyInvitePersistence>,
}

impl CompanyInviteUseCases {
    pub fn new(
        company_persistence: Arc<dyn CompanyPersistence>,
        invite_persistence: Arc<dyn CompanyInvitePersistence>,
    ) -> Self {
        Self {
            company_persistence,
            invite_persistence,
        }
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        let company = self
            .company_persistence
            .get_by_id(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found.".into()))?;

        if company.user_id != user_id {
            return Err(AppError::Internal(
                "Unauthorized: only the company owner can manage invites and team members.".into(),
            ));
        }

        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let email_trimmed = email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!("Creating invite for email {} to company {}", email_trimmed, company_id);
        self.invite_persistence
            .create_invite(company_id, &email_trimmed)
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_invites(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyInvite>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.invite_persistence.list_invites_by_company(company_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
    ) -> AppResult<Option<CompanyInvite>> {
        self.verify_company_owner(user_id, company_id).await?;
        let invite = self.invite_persistence.get_invite_by_id(invite_id).await?;
        if let Some(ref inv) = invite {
            if inv.company_id != company_id {
                return Ok(None);
            }
        }
        Ok(invite)
    }

    #[instrument(skip(self))]
    pub async fn update_company_invite_email(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
        new_email: &str,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))?;

        if invite.company_id != company_id {
            return Err(AppError::Internal("Invite does not belong to this company.".into()));
        }

        let email_trimmed = new_email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!("Updating invite {} email to {}", invite_id, email_trimmed);
        self.invite_persistence
            .update_invite_email(invite_id, &email_trimmed)
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))?;

        if invite.company_id != company_id {
            return Err(AppError::Internal("Invite does not belong to this company.".into()));
        }

        info!("Deleting invite {} for company {}", invite_id, company_id);
        self.invite_persistence.delete_invite(invite_id).await
    }

    #[instrument(skip(self))]
    pub async fn list_user_invites(&self, user_email: &str) -> AppResult<Vec<CompanyInvite>> {
        self.invite_persistence.list_invites_by_email(user_email.trim()).await
    }

    #[instrument(skip(self, user))]
    pub async fn accept_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))?;

        if !invite.email.eq_ignore_ascii_case(&user.email) {
            return Err(AppError::Internal(
                "Invite email does not match active user email.".into(),
            ));
        }

        info!("User {} accepting invite {}", user.id, invite_id);
        let updated_invite = self
            .invite_persistence
            .update_invite_status(invite_id, "accepted")
            .await?;

        // Add user to company team
        let _ = self
            .invite_persistence
            .add_member(invite.company_id, user.id, "member")
            .await?;

        Ok(updated_invite)
    }

    #[instrument(skip(self, user))]
    pub async fn decline_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(|| AppError::Internal("Invite not found.".into()))?;

        if !invite.email.eq_ignore_ascii_case(&user.email) {
            return Err(AppError::Internal(
                "Invite email does not match active user email.".into(),
            ));
        }

        info!("User {} declining invite {}", user.id, invite_id);
        self.invite_persistence
            .update_invite_status(invite_id, "declined")
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_team_members(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyMember>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.invite_persistence.list_members_by_company(company_id).await
    }

    #[instrument(skip(self))]
    pub async fn remove_company_team_member(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
    ) -> AppResult<()> {
        self.verify_company_owner(user_id, company_id).await?;

        if user_id == member_user_id {
            return Err(AppError::Internal(
                "Cannot remove company owner from the team.".into(),
            ));
        }

        info!(
            "Removing user {} from company {} team",
            member_user_id, company_id
        );
        self.invite_persistence.remove_member(company_id, member_user_id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use chrono::Utc;
    use crate::entities::company::Company;
    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> {
            unimplemented!()
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }

        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }

        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }

        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _api_key: Option<&str>, _provider: Option<&str>, _model: Option<&str>) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockCompanyInvitePersistence {
        invites: Mutex<Vec<CompanyInvite>>,
        members: Mutex<Vec<CompanyMember>>,
    }

    #[async_trait]
    impl CompanyInvitePersistence for MockCompanyInvitePersistence {
        async fn create_invite(&self, company_id: Uuid, email: &str) -> AppResult<CompanyInvite> {
            let invite = CompanyInvite {
                id: Uuid::new_v4(),
                company_id,
                company_name: Some("Acme".to_string()),
                email: email.to_string(),
                status: "pending".to_string(),
                created_at: Utc::now().naive_utc(),
            };
            self.invites.lock().unwrap().push(invite.clone());
            Ok(invite)
        }

        async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>> {
            Ok(self
                .invites
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.id == id)
                .cloned())
        }

        async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>> {
            Ok(self
                .invites
                .lock()
                .unwrap()
                .iter()
                .filter(|i| i.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn update_invite_email(&self, id: Uuid, new_email: &str) -> AppResult<CompanyInvite> {
            let mut list = self.invites.lock().unwrap();
            let invite = list
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;
            invite.email = new_email.to_string();
            Ok(invite.clone())
        }

        async fn update_invite_status(&self, id: Uuid, status: &str) -> AppResult<CompanyInvite> {
            let mut list = self.invites.lock().unwrap();
            let invite = list
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;
            invite.status = status.to_string();
            Ok(invite.clone())
        }

        async fn delete_invite(&self, id: Uuid) -> AppResult<()> {
            self.invites.lock().unwrap().retain(|i| i.id != id);
            Ok(())
        }

        async fn list_invites_by_email(&self, email: &str) -> AppResult<Vec<CompanyInvite>> {
            Ok(self
                .invites
                .lock()
                .unwrap()
                .iter()
                .filter(|i| i.email.eq_ignore_ascii_case(email))
                .cloned()
                .collect())
        }

        async fn add_member(&self, company_id: Uuid, user_id: Uuid, role: &str) -> AppResult<CompanyMember> {
            let member = CompanyMember {
                id: Uuid::new_v4(),
                company_id,
                user_id,
                username: Some("inviteduser".to_string()),
                email: Some("invited@example.com".to_string()),
                role: role.to_string(),
                created_at: Utc::now().naive_utc(),
            };
            self.members.lock().unwrap().push(member.clone());
            Ok(member)
        }

        async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>> {
            Ok(self
                .members
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
            self.members
                .lock()
                .unwrap()
                .retain(|m| !(m.company_id == company_id && m.user_id == user_id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn company_invites_crud_and_accept_flow() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let invite_persistence = Arc::new(MockCompanyInvitePersistence {
            invites: Mutex::new(Vec::new()),
            members: Mutex::new(Vec::new()),
        });

        let use_cases = CompanyInviteUseCases::new(company_persistence, invite_persistence);

        // Owner creates invite
        let invite = use_cases
            .create_company_invite(owner_id, company_id, "user@example.com")
            .await
            .unwrap();
        assert_eq!(invite.email, "user@example.com");

        // Non-owner cannot create invite
        let err = use_cases
            .create_company_invite(Uuid::new_v4(), company_id, "other@example.com")
            .await;
        assert!(err.is_err());

        // Update invite email
        let updated = use_cases
            .update_company_invite_email(owner_id, company_id, invite.id, "newuser@example.com")
            .await
            .unwrap();
        assert_eq!(updated.email, "newuser@example.com");

        // List invites for user
        let user_invites = use_cases.list_user_invites("newuser@example.com").await.unwrap();
        assert_eq!(user_invites.len(), 1);

        // User accepts invite
        let user = User {
            id: Uuid::new_v4(),
            username: "newuser".to_string(),
            email: "newuser@example.com".to_string(),
            password_hash: "hash".to_string(),
            created_at: Utc::now().naive_utc(),
        };

        let accepted = use_cases.accept_invite(&user, invite.id).await.unwrap();
        assert_eq!(accepted.status, "accepted");

        // Verify member was added to team
        let members = use_cases.list_company_team_members(owner_id, company_id).await.unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, user.id);

        // Owner removes member from team
        use_cases
            .remove_company_team_member(owner_id, company_id, user.id)
            .await
            .unwrap();
        let members_after = use_cases.list_company_team_members(owner_id, company_id).await.unwrap();
        assert_eq!(members_after.len(), 0);
    }
}
