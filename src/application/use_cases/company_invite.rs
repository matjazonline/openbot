use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        user::User,
    },
    use_cases::company::{CompanyPersistence, company_not_found, owned_company},
};

/// An invite belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign invite from a nonexistent one. See [`owned_company`].
fn invite_not_found() -> AppError {
    AppError::NotFound("Invite not found in this company.".into())
}

#[async_trait]
pub trait CompanyInvitePersistence: Send + Sync {
    async fn create_invite(
        &self,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite>;
    async fn get_invite_by_id(&self, id: Uuid) -> AppResult<Option<CompanyInvite>>;
    async fn list_invites_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyInvite>>;
    async fn update_invite(
        &self,
        id: Uuid,
        new_email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite>;
    async fn delete_invite(&self, id: Uuid) -> AppResult<()>;
    async fn list_invites_by_email(&self, email: &str) -> AppResult<Vec<CompanyInvite>>;
    async fn accept_pending_invite(
        &self,
        invite_id: Uuid,
        user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>>;
    async fn decline_pending_invite(
        &self,
        invite_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>>;
    async fn list_members_by_company(&self, company_id: Uuid) -> AppResult<Vec<CompanyMember>>;
    async fn update_member_role(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<Option<CompanyMember>>;
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
        owned_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn create_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        email: &str,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let email_trimmed = email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!(
            "Creating invite for email {} to company {}",
            email_trimmed, company_id
        );
        self.invite_persistence
            .create_invite(company_id, &email_trimmed, role)
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_invites(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyInvite>> {
        self.verify_company_owner(user_id, company_id).await?;
        self.invite_persistence
            .list_invites_by_company(company_id)
            .await
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
        if let Some(ref inv) = invite
            && inv.company_id != company_id
        {
            return Ok(None);
        }
        Ok(invite)
    }

    #[instrument(skip(self))]
    pub async fn update_company_invite(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        invite_id: Uuid,
        new_email: &str,
        role: Option<CompanyAccessRole>,
    ) -> AppResult<CompanyInvite> {
        self.verify_company_owner(user_id, company_id).await?;

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(invite_not_found)?;

        if invite.company_id != company_id {
            return Err(invite_not_found());
        }
        if invite.status != "pending" {
            return Err(AppError::BadRequest(
                "Only a pending invitation can be changed.".into(),
            ));
        }

        let email_trimmed = new_email.trim().to_lowercase();
        if email_trimmed.is_empty() || !email_trimmed.contains('@') {
            return Err(AppError::Internal(
                "Please provide a valid email address.".into(),
            ));
        }

        info!("Updating invite {} email to {}", invite_id, email_trimmed);
        self.invite_persistence
            .update_invite(invite_id, &email_trimmed, role.unwrap_or(invite.role))
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
            .ok_or_else(invite_not_found)?;

        if invite.company_id != company_id {
            return Err(invite_not_found());
        }

        info!("Deleting invite {} for company {}", invite_id, company_id);
        self.invite_persistence.delete_invite(invite_id).await
    }

    #[instrument(skip(self))]
    pub async fn list_user_invites(&self, user_email: &str) -> AppResult<Vec<CompanyInvite>> {
        self.invite_persistence
            .list_invites_by_email(user_email.trim())
            .await
    }

    /// Whether an account has an invitation that still needs an answer.
    #[instrument(skip(self))]
    pub async fn has_pending_user_invites(&self, user_email: &str) -> AppResult<bool> {
        Ok(self
            .list_user_invites(user_email)
            .await?
            .iter()
            .any(|invite| invite.status == "pending"))
    }

    #[instrument(skip(self, user))]
    pub async fn accept_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        info!("User {} accepting invite {}", user.id, invite_id);
        if let Some(invite) = self
            .invite_persistence
            .accept_pending_invite(invite_id, user.id, &user.email)
            .await?
        {
            return Ok(invite);
        }

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(invite_not_found)?;
        if !invite.email.eq_ignore_ascii_case(&user.email) {
            return Err(AppError::Internal(
                "Invite email does not match active user email.".into(),
            ));
        }
        if invite.status == "accepted" {
            return Ok(invite);
        }
        Err(AppError::Internal(format!(
            "Invite was already processed as '{}'.",
            invite.status
        )))
    }

    #[instrument(skip(self, user))]
    pub async fn decline_invite(&self, user: &User, invite_id: Uuid) -> AppResult<CompanyInvite> {
        info!("User {} declining invite {}", user.id, invite_id);
        if let Some(invite) = self
            .invite_persistence
            .decline_pending_invite(invite_id, &user.email)
            .await?
        {
            return Ok(invite);
        }

        let invite = self
            .invite_persistence
            .get_invite_by_id(invite_id)
            .await?
            .ok_or_else(invite_not_found)?;
        if !invite.email.eq_ignore_ascii_case(&user.email) {
            return Err(AppError::Internal(
                "Invite email does not match active user email.".into(),
            ));
        }
        if invite.status == "declined" {
            return Ok(invite);
        }
        Err(AppError::Internal(format!(
            "Invite was already processed as '{}'.",
            invite.status
        )))
    }

    #[instrument(skip(self))]
    pub async fn list_company_team_members(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyMember>> {
        // The one place a non-owner has legitimate access, so it cannot delegate to
        // `owned_company` outright — but a caller who is neither owner nor member is told exactly
        // what a stranger asking about a nonexistent company is told.
        match owned_company(self.company_persistence.as_ref(), user_id, company_id).await {
            Ok(_) => {}
            Err(AppError::NotFound(_)) => {
                let members = self
                    .invite_persistence
                    .list_members_by_company(company_id)
                    .await?;
                return if members.iter().any(|m| m.user_id == user_id) {
                    Ok(members)
                } else {
                    Err(company_not_found())
                };
            }
            Err(err) => return Err(err),
        }

        self.invite_persistence
            .list_members_by_company(company_id)
            .await
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
        self.invite_persistence
            .remove_member(company_id, member_user_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn update_company_team_member_role(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
        role: CompanyAccessRole,
    ) -> AppResult<CompanyMember> {
        self.verify_company_owner(user_id, company_id).await?;

        if user_id == member_user_id {
            return Err(AppError::Internal(
                "The company owner's access role cannot be changed.".into(),
            ));
        }

        info!(
            "Updating user {} to role {} in company {}",
            member_user_id, role, company_id
        );
        self.invite_persistence
            .update_member_role(company_id, member_user_id, role)
            .await?
            .ok_or_else(|| AppError::NotFound("Team member not found".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::company::Company;
    use crate::entities::company_member::CompanyMembership;
    use crate::use_cases::company::CompanyWrite;
    use chrono::Utc;
    use std::sync::Mutex;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
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

        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }

        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        /// Model connections are not part of what these tests drive; a call here is a wiring mistake
        /// rather than a state worth simulating.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection path")
        }
    }

    struct MockCompanyInvitePersistence {
        invites: Mutex<Vec<CompanyInvite>>,
        members: Mutex<Vec<CompanyMember>>,
    }

    #[async_trait]
    impl CompanyInvitePersistence for MockCompanyInvitePersistence {
        async fn create_invite(
            &self,
            company_id: Uuid,
            email: &str,
            role: CompanyAccessRole,
        ) -> AppResult<CompanyInvite> {
            let invite = CompanyInvite {
                id: Uuid::new_v4(),
                company_id,
                company_name: Some("Acme".to_string()),
                email: email.to_string(),
                role,
                status: "pending".to_string(),
                created_at: Utc::now(),
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

        async fn update_invite(
            &self,
            id: Uuid,
            new_email: &str,
            role: CompanyAccessRole,
        ) -> AppResult<CompanyInvite> {
            let mut list = self.invites.lock().unwrap();
            let invite = list
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;
            invite.email = new_email.to_string();
            invite.role = role;
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

        async fn accept_pending_invite(
            &self,
            invite_id: Uuid,
            user_id: Uuid,
            user_email: &str,
        ) -> AppResult<Option<CompanyInvite>> {
            let mut invites = self.invites.lock().unwrap();
            let Some(invite) = invites.iter_mut().find(|i| {
                i.id == invite_id
                    && i.status == "pending"
                    && i.email.eq_ignore_ascii_case(user_email)
            }) else {
                return Ok(None);
            };

            let mut members = self.members.lock().unwrap();
            if let Some(member) = members
                .iter_mut()
                .find(|m| m.company_id == invite.company_id && m.user_id == user_id)
            {
                member.role = invite.role;
            } else {
                members.push(CompanyMember {
                    id: Uuid::new_v4(),
                    company_id: invite.company_id,
                    user_id,
                    username: Some("inviteduser".to_string()),
                    email: Some(user_email.to_string()),
                    avatar_url: None,
                    role: invite.role,
                    created_at: Utc::now(),
                });
            }
            invite.status = "accepted".to_string();
            Ok(Some(invite.clone()))
        }

        async fn decline_pending_invite(
            &self,
            invite_id: Uuid,
            user_email: &str,
        ) -> AppResult<Option<CompanyInvite>> {
            let mut invites = self.invites.lock().unwrap();
            let Some(invite) = invites.iter_mut().find(|i| {
                i.id == invite_id
                    && i.status == "pending"
                    && i.email.eq_ignore_ascii_case(user_email)
            }) else {
                return Ok(None);
            };
            invite.status = "declined".to_string();
            Ok(Some(invite.clone()))
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

        async fn update_member_role(
            &self,
            company_id: Uuid,
            user_id: Uuid,
            role: CompanyAccessRole,
        ) -> AppResult<Option<CompanyMember>> {
            let mut members = self.members.lock().unwrap();
            let Some(member) = members
                .iter_mut()
                .find(|member| member.company_id == company_id && member.user_id == user_id)
            else {
                return Ok(None);
            };
            member.role = role;
            Ok(Some(member.clone()))
        }

        async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
            self.members
                .lock()
                .unwrap()
                .retain(|m| !(m.company_id == company_id && m.user_id == user_id));
            Ok(())
        }
    }

    /// Team-member listing is the one place a non-owner has legitimate access, so it needs its own
    /// coverage: a member gets in, and anyone else is refused in the same words a stranger hears
    /// about a company that does not exist.
    #[tokio::test]
    async fn members_may_list_the_team_and_everyone_else_is_told_nothing() {
        let owner_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let stranger_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });
        let invite_persistence = Arc::new(MockCompanyInvitePersistence {
            invites: Mutex::new(Vec::new()),
            members: Mutex::new(vec![CompanyMember {
                id: Uuid::new_v4(),
                company_id,
                user_id: member_id,
                username: Some("member".into()),
                email: Some("member@example.com".into()),
                avatar_url: None,
                role: CompanyAccessRole::Member,
                created_at: Utc::now(),
            }]),
        });
        let use_cases = CompanyInviteUseCases::new(company_persistence, invite_persistence);

        assert_eq!(
            use_cases
                .list_company_team_members(owner_id, company_id)
                .await
                .unwrap()
                .len(),
            1,
            "the owner still sees the team"
        );
        assert_eq!(
            use_cases
                .list_company_team_members(member_id, company_id)
                .await
                .unwrap()
                .len(),
            1,
            "a member is not the owner but may still read the team"
        );

        let stranger_err = use_cases
            .list_company_team_members(stranger_id, company_id)
            .await
            .unwrap_err();
        let missing_err = use_cases
            .list_company_team_members(stranger_id, Uuid::new_v4())
            .await
            .unwrap_err();

        assert!(
            matches!(stranger_err, AppError::NotFound(_)),
            "{stranger_err:?}"
        );
        assert_eq!(
            stranger_err.to_string(),
            missing_err.to_string(),
            "telling the two apart would confirm the company exists"
        );
    }

    #[tokio::test]
    async fn company_invites_crud_and_accept_flow() {
        let owner_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: owner_id,
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        let invite_persistence = Arc::new(MockCompanyInvitePersistence {
            invites: Mutex::new(Vec::new()),
            members: Mutex::new(Vec::new()),
        });

        let use_cases = CompanyInviteUseCases::new(company_persistence, invite_persistence);

        // Owner creates invite
        let invite = use_cases
            .create_company_invite(
                owner_id,
                company_id,
                "user@example.com",
                CompanyAccessRole::Admin,
            )
            .await
            .unwrap();
        assert_eq!(invite.email, "user@example.com");
        assert_eq!(invite.role, CompanyAccessRole::Admin);

        // Non-owner cannot create invite
        let err = use_cases
            .create_company_invite(
                Uuid::new_v4(),
                company_id,
                "other@example.com",
                CompanyAccessRole::Member,
            )
            .await;
        assert!(err.is_err());

        // Update invite email
        let updated = use_cases
            .update_company_invite(
                owner_id,
                company_id,
                invite.id,
                "newuser@example.com",
                Some(CompanyAccessRole::Admin),
            )
            .await
            .unwrap();
        assert_eq!(updated.email, "newuser@example.com");

        let backwards_compatible_update = use_cases
            .update_company_invite(owner_id, company_id, invite.id, "newuser@example.com", None)
            .await
            .unwrap();
        assert_eq!(backwards_compatible_update.role, CompanyAccessRole::Admin);

        // List invites for user
        let user_invites = use_cases
            .list_user_invites("newuser@example.com")
            .await
            .unwrap();
        assert_eq!(user_invites.len(), 1);
        assert!(
            use_cases
                .has_pending_user_invites("newuser@example.com")
                .await
                .unwrap()
        );

        // User accepts invite
        let user = User {
            id: Uuid::new_v4(),
            username: "newuser".to_string(),
            email: "newuser@example.com".to_string(),
            password_hash: "hash".to_string(),
            avatar_url: None,
            created_at: Utc::now(),
        };

        let accepted = use_cases.accept_invite(&user, invite.id).await.unwrap();
        assert_eq!(accepted.status, "accepted");
        assert!(
            !use_cases
                .has_pending_user_invites("newuser@example.com")
                .await
                .unwrap()
        );
        assert_eq!(
            use_cases
                .accept_invite(&user, invite.id)
                .await
                .unwrap()
                .status,
            "accepted"
        );
        assert!(use_cases.decline_invite(&user, invite.id).await.is_err());
        assert!(
            use_cases
                .update_company_invite(
                    owner_id,
                    company_id,
                    invite.id,
                    "another@example.com",
                    Some(CompanyAccessRole::Member),
                )
                .await
                .is_err(),
            "an accepted invitation cannot drift away from the membership it created"
        );

        // Verify member was added to team
        let members = use_cases
            .list_company_team_members(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, user.id);
        assert_eq!(members[0].role, CompanyAccessRole::Admin);

        let changed_member = use_cases
            .update_company_team_member_role(
                owner_id,
                company_id,
                user.id,
                CompanyAccessRole::Member,
            )
            .await
            .unwrap();
        assert_eq!(changed_member.role, CompanyAccessRole::Member);

        // Verify member (non-owner) can also list company team members
        let member_list = use_cases
            .list_company_team_members(user.id, company_id)
            .await
            .unwrap();
        assert_eq!(member_list.len(), 1);
        assert_eq!(member_list[0].user_id, user.id);

        // Verify random user cannot list company team members
        let random_user_err = use_cases
            .list_company_team_members(Uuid::new_v4(), company_id)
            .await;
        assert!(random_user_err.is_err());

        // Owner removes member from team
        use_cases
            .remove_company_team_member(owner_id, company_id, user.id)
            .await
            .unwrap();
        let members_after = use_cases
            .list_company_team_members(owner_id, company_id)
            .await
            .unwrap();
        assert_eq!(members_after.len(), 0);
    }
}
