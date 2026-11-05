//! The invite, membership and guided-removal use cases, driven against hand-written doubles.
//!
//! Hoisted out of `company_invite.rs` because the module's own test body outgrew the ~500 lines
//! `src/AGENTS.md` allows in an inline `mod tests`; the mocks stay here, shared by every test in
//! it, rather than being re-declared per case.

use super::*;
use crate::entities::company::Company;
use crate::entities::company_member::CompanyMembership;
use crate::use_cases::company_test_support::MockCompanyPersistence;
use chrono::Utc;
use std::sync::Mutex;

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
    async fn create_invite(&self, command: CreateCompanyInvite) -> AppResult<CompanyInvite> {
        let invite = CompanyInvite {
            id: Uuid::new_v4(),
            company_id: command.company_id,
            company_name: Some("Acme".into()),
            email: command.email.to_string(),
            role: command.role,
            access_level: command.access_level,
            status: "pending".into(),
            created_at: Utc::now(),
        };
        self.invites.lock().unwrap().push(invite.clone());
        Ok(invite)
    }
    async fn get_invite_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<Option<CompanyInvite>> {
        Ok(self
            .invites
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.id == id && i.company_id == company_id)
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

    /// No test here invites an owner; the refusal is covered against Postgres in
    /// `persistence/company_owner_tests.rs`.
    async fn is_company_owner_address(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn update_invite(
        &self,
        command: UpdateCompanyInvite,
    ) -> AppResult<Option<CompanyInvite>> {
        let mut list = self.invites.lock().unwrap();
        let Some(invite) = list.iter_mut().find(|i| {
            i.id == command.invite_id && i.company_id == command.company_id && i.status == "pending"
        }) else {
            return Ok(None);
        };
        invite.email = command.email.to_string();
        if let Some(role) = command.role {
            invite.role = role;
        }
        if let Some(level) = command.access_level {
            invite.access_level = level;
        }
        Ok(Some(invite.clone()))
    }
    async fn get_member_by_user_id(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<Option<CompanyMember>> {
        Ok(self
            .members
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.company_id == company_id && m.user_id == user_id)
            .cloned())
    }
    async fn set_member_access_level(
        &self,
        _change: MemberLevelChange,
    ) -> AppResult<Option<CompanyMember>> {
        Err(AppError::Internal(
            "Level writes require the native authority fixture".into(),
        ))
    }
    async fn set_invite_access_level(
        &self,
        _change: InviteLevelChange,
    ) -> AppResult<Option<CompanyInvite>> {
        Err(AppError::Internal(
            "Level writes require the native authority fixture".into(),
        ))
    }

    async fn delete_invite(&self, id: Uuid) -> AppResult<()> {
        self.invites.lock().unwrap().retain(|i| i.id != id);
        Ok(())
    }

    async fn list_invites_for_user(
        &self,
        _user_id: Uuid,
        expected_email: &str,
    ) -> AppResult<Vec<CompanyInvite>> {
        Ok(self
            .invites
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.email.eq_ignore_ascii_case(expected_email))
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
        let Some(invite) = invites
            .iter_mut()
            .find(|i| i.id == invite_id && i.email.eq_ignore_ascii_case(user_email))
        else {
            return Ok(None);
        };

        if invite.status == "accepted" {
            return Ok(Some(invite.clone()));
        }
        if invite.status != "pending" {
            return Ok(None);
        }

        let mut members = self.members.lock().unwrap();
        if let Some(member) = members
            .iter_mut()
            .find(|m| m.company_id == invite.company_id && m.user_id == user_id)
        {
            member.role = invite.role;
        } else {
            members.push(CompanyMember {
                access_level: invite.access_level,
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
        _user_id: Uuid,
        user_email: &str,
    ) -> AppResult<Option<CompanyInvite>> {
        let mut invites = self.invites.lock().unwrap();
        let Some(invite) = invites
            .iter_mut()
            .find(|i| i.id == invite_id && i.email.eq_ignore_ascii_case(user_email))
        else {
            return Ok(None);
        };
        if invite.status == "declined" {
            return Ok(Some(invite.clone()));
        }
        if invite.status != "pending" {
            return Ok(None);
        }
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
    Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: owner_id,
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]))
}

fn member_row(company_id: Uuid, user_id: Uuid) -> CompanyMember {
    CompanyMember {
        access_level: crate::entities::access_level::AccessLevel::PUBLIC,
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
    let admin_id = Uuid::new_v4();
    let stranger_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();

    let company_persistence = Arc::new(
        MockCompanyPersistence::new(vec![Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: owner_id,
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }])
        .with_memberships(vec![
            (member_id, company_id, CompanyMembership::Member),
            (admin_id, company_id, CompanyMembership::Admin),
        ]),
    );
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

    assert_eq!(
        use_cases
            .list_company_team_members(admin_id, company_id)
            .await
            .unwrap()
            .len(),
        1,
        "an admin may read the team too"
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

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        channel_defaults: Default::default(),
        id: company_id,
        user_id: owner_id,
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }]));

    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(Vec::new()));

    let use_cases = CompanyInviteUseCases::new(company_persistence.clone(), invite_persistence);

    // Owner creates invite
    let invite = use_cases
        .create_company_invite(crate::use_cases::company_invite::CreateCompanyInvite {
            actor_id: owner_id,
            company_id,
            email: ("user@example.com").to_string().into(),
            role: CompanyAccessRole::Admin,
            access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        })
        .await
        .unwrap();
    assert_eq!(invite.email, "user@example.com");
    assert_eq!(invite.role, CompanyAccessRole::Admin);

    // Non-owner cannot create invite
    let err = use_cases
        .create_company_invite(crate::use_cases::company_invite::CreateCompanyInvite {
            actor_id: Uuid::new_v4(),
            company_id,
            email: ("other@example.com").to_string().into(),
            role: CompanyAccessRole::Member,
            access_level: crate::entities::access_level::AccessLevel::PUBLIC,
        })
        .await;
    assert!(err.is_err());

    // Update invite email
    let updated = use_cases
        .update_company_invite(crate::use_cases::company_invite::UpdateCompanyInvite {
            actor_id: owner_id,
            company_id,
            invite_id: invite.id,
            email: ("newuser@example.com").to_string().into(),
            role: Some(CompanyAccessRole::Admin),
            access_level: None,
        })
        .await
        .unwrap();
    assert_eq!(updated.email, "newuser@example.com");

    let backwards_compatible_update = use_cases
        .update_company_invite(crate::use_cases::company_invite::UpdateCompanyInvite {
            actor_id: owner_id,
            company_id,
            invite_id: invite.id,
            email: ("newuser@example.com").to_string().into(),
            role: None,
            access_level: None,
        })
        .await
        .unwrap();
    assert_eq!(backwards_compatible_update.role, CompanyAccessRole::Admin);

    let user = User {
        auth_revision: 0,
        id: Uuid::new_v4(),
        username: "newuser".to_string(),
        email: "newuser@example.com".to_string(),
        email_verified_at: Some(Utc::now()),
        password_hash: Some("hash".to_string()),
        avatar_url: None,
        created_at: Utc::now(),
    };

    let unverified_user = User {
        email_verified_at: None,
        ..user.clone()
    };
    assert!(matches!(
        use_cases.list_user_invites(&unverified_user).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(
        !use_cases
            .has_pending_user_invites(&unverified_user)
            .await
            .unwrap()
    );
    assert!(matches!(
        use_cases.accept_invite(&unverified_user, invite.id).await,
        Err(AppError::BadRequest(_))
    ));

    // List invites for the verified current address.
    let user_invites = use_cases.list_user_invites(&user).await.unwrap();
    assert_eq!(user_invites.len(), 1);
    assert!(use_cases.has_pending_user_invites(&user).await.unwrap());

    let accepted = use_cases.accept_invite(&user, invite.id).await.unwrap();
    assert_eq!(accepted.status, "accepted");
    assert!(!use_cases.has_pending_user_invites(&user).await.unwrap());
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
            .update_company_invite(crate::use_cases::company_invite::UpdateCompanyInvite {
                actor_id: owner_id,
                company_id,
                invite_id: invite.id,
                email: ("another@example.com").to_string().into(),
                role: Some(CompanyAccessRole::Member),
                access_level: None
            })
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

    // The two doubles stand for one `company_members` table: acceptance wrote the row the
    // company double answers `company_access` from.
    company_persistence.memberships.lock().unwrap().push((
        user.id,
        company_id,
        CompanyMembership::Member,
    ));

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
