//! The invite, membership and guided-removal use cases, driven against hand-written doubles.
//!
//! Hoisted out of `company_invite.rs` because the module's own test body outgrew the ~500 lines
//! `src/AGENTS.md` allows in an inline `mod tests`; the mocks stay here, shared by every test in
//! it, rather than being re-declared per case.

use super::*;
use crate::entities::company::Company;
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

impl MockCompanyInvitePersistence {
    fn with_members(members: Vec<CompanyMember>) -> Self {
        Self {
            invites: Mutex::new(Vec::new()),
            members: Mutex::new(members),
        }
    }
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
            i.id == invite_id && i.status == "pending" && i.email.eq_ignore_ascii_case(user_email)
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
            i.id == invite_id && i.status == "pending" && i.email.eq_ignore_ascii_case(user_email)
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

fn acme(owner_id: Uuid, company_id: Uuid) -> Arc<MockCompanyPersistence> {
    Arc::new(MockCompanyPersistence {
        companies: Mutex::new(vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: owner_id,
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }]),
    })
}

fn member_row(company_id: Uuid, user_id: Uuid) -> CompanyMember {
    CompanyMember {
        id: Uuid::new_v4(),
        company_id,
        user_id,
        username: Some("member".into()),
        email: Some("member@example.com".into()),
        avatar_url: None,
        role: CompanyAccessRole::Member,
        created_at: Utc::now(),
    }
}

/// Removal is the owner's call; there is no held work to hand over first.
#[tokio::test]
async fn the_owner_removes_a_member() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    use_cases
        .remove_company_team_member(owner_id, company_id, member_id)
        .await
        .unwrap();
    assert!(invite_persistence.members.lock().unwrap().is_empty());
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
            avatar_url: None,
            created_at: Utc::now(),
        }]),
    });
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
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
            avatar_url: None,
            created_at: Utc::now(),
        }]),
    });

    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(Vec::new()));

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
        .update_company_team_member_role(owner_id, company_id, user.id, CompanyAccessRole::Member)
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
