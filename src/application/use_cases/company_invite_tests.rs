//! The invite, membership and guided-removal use cases, driven against hand-written doubles.
//!
//! Hoisted out of `company_invite.rs` because the module's own test body outgrew the ~500 lines
//! `src/AGENTS.md` allows in an inline `mod tests`; the mocks stay here, shared by every test in
//! it, rather than being re-declared per case.

use super::*;
use crate::entities::company::Company;
use crate::entities::task::{TaskOwner, TaskOwnerCandidate, TaskStatus};
use crate::entities::transport::PrincipalId;
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

    async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
        Ok(vec![])
    }

    async fn list_company_team_accounts(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
        unimplemented!("this double is not exercised on the team-account path")
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
    /// What the pre-check reports, so a test can drive the guard without a database.
    at_stake: Mutex<MemberWorkAtStake>,
}

impl MockCompanyInvitePersistence {
    fn with_members(members: Vec<CompanyMember>) -> Self {
        Self {
            invites: Mutex::new(Vec::new()),
            members: Mutex::new(members),
            at_stake: Mutex::new(MemberWorkAtStake::default()),
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

    async fn member_work_at_stake(
        &self,
        _company_id: Uuid,
        _user_id: Uuid,
    ) -> AppResult<MemberWorkAtStake> {
        Ok(self.at_stake.lock().unwrap().clone())
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
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
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

/// The common case: nobody's live work is at stake, so the removal is the one it always was.
#[tokio::test]
async fn a_member_with_nothing_live_is_removed_exactly_as_before() {
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

/// One live task and one live ask, as a pane would be rendered from.
fn work_at_stake(candidate: TaskOwner) -> MemberWorkAtStake {
    MemberWorkAtStake {
        owned_tasks: vec![
            OwnedTaskAtStake {
                task_id: Uuid::new_v4(),
                channel_id: Uuid::new_v4(),
                correlation_id: Uuid::new_v4(),
                task_type: "agent_run".into(),
                status: TaskStatus::Pending,
                ownership_version: 3,
            },
            OwnedTaskAtStake {
                task_id: Uuid::new_v4(),
                channel_id: Uuid::new_v4(),
                correlation_id: Uuid::new_v4(),
                task_type: "agent_run".into(),
                status: TaskStatus::Processing,
                ownership_version: 7,
            },
        ],
        delegated_asks: vec![DelegatedAskAtStake {
            task_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            outreach_id: Uuid::new_v4(),
            target_id: Uuid::new_v4(),
            outreach_version: 1,
            subject: "Can you confirm the invoice?".into(),
            asked_address: "member@example.com".into(),
            expires_at: Utc::now(),
        }],
        owner_candidates: vec![TaskOwnerCandidate {
            owner: candidate,
            label: "Dana".into(),
        }],
        truncated: false,
    }
}

/// The commands a guided removal issues, standing in for the ownership and delegation ones.
///
/// Each success clears its own item from the pre-check the use case re-reads, because that is
/// what the real commands do — and it is what makes "the removal only proceeds once nothing is
/// left" a property this double can actually falsify.
struct MockMemberWorkCommands {
    at_stake: Arc<MockCompanyInvitePersistence>,
    /// A task whose ownership command refuses, the way a version conflict does when somebody
    /// else moved the task while the pane was open.
    conflicting_task: Option<Uuid>,
    handovers: Mutex<Vec<(Uuid, OwnedWorkHandover)>>,
    redirected: Mutex<Vec<(Uuid, PrincipalId)>>,
}

impl MockMemberWorkCommands {
    fn over(at_stake: &Arc<MockCompanyInvitePersistence>) -> Self {
        Self {
            at_stake: at_stake.clone(),
            conflicting_task: None,
            handovers: Mutex::new(Vec::new()),
            redirected: Mutex::new(Vec::new()),
        }
    }

    fn conflicting_on(mut self, task_id: Uuid) -> Self {
        self.conflicting_task = Some(task_id);
        self
    }
}

#[async_trait]
impl MemberWorkCommands for MockMemberWorkCommands {
    async fn hand_over_owned_task(
        &self,
        task: &OwnedTaskAtStake,
        handover: &OwnedWorkHandover,
    ) -> AppResult<()> {
        if self.conflicting_task == Some(task.task_id) {
            return Err(AppError::Conflict(
                "Task ownership moved since this page was loaded.".into(),
            ));
        }
        self.handovers
            .lock()
            .unwrap()
            .push((task.task_id, handover.clone()));
        self.at_stake
            .at_stake
            .lock()
            .unwrap()
            .owned_tasks
            .retain(|owned| owned.task_id != task.task_id);
        Ok(())
    }

    async fn redirect_delegated_ask(
        &self,
        ask: &DelegatedAskAtStake,
        new_owner: PrincipalId,
    ) -> AppResult<()> {
        self.redirected
            .lock()
            .unwrap()
            .push((ask.target_id, new_owner));
        self.at_stake
            .at_stake
            .lock()
            .unwrap()
            .delegated_asks
            .retain(|open| open.target_id != ask.target_id);
        Ok(())
    }
}

/// One selection, applied to everything: every task goes to the named owner with the one
/// handoff instruction, every ask is re-asked at that same owner, and only then is the member
/// removed.
#[tokio::test]
async fn one_decision_hands_over_every_task_and_redirects_every_ask() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let taker = TaskOwner::Human(PrincipalId::new(Uuid::new_v4()));
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() = work_at_stake(taker);
    let at_stake = invite_persistence.at_stake.lock().unwrap().clone();
    let commands = MockMemberWorkCommands::over(&invite_persistence);
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let handover = OwnedWorkHandover {
        new_owner: taker,
        handoff_instruction: "Chase the invoice by Friday.".into(),
    };
    use_cases
        .remove_company_team_member_with_handover(
            &commands,
            owner_id,
            company_id,
            member_id,
            Some(handover.clone()),
        )
        .await
        .unwrap();

    let handed_over = commands.handovers.lock().unwrap().clone();
    assert_eq!(
        handed_over,
        at_stake
            .owned_tasks
            .iter()
            .map(|task| (task.task_id, handover.clone()))
            .collect::<Vec<_>>(),
        "one command per task, all carrying the single decision"
    );
    assert_eq!(
        *commands.redirected.lock().unwrap(),
        vec![(
            at_stake.delegated_asks[0].target_id,
            taker.principal_id().unwrap()
        )],
        "the asks are re-asked at the same chosen owner, not cancelled"
    );
    assert!(
        invite_persistence.members.lock().unwrap().is_empty(),
        "and then the member is actually removed"
    );
}

/// The partial-failure policy: attempt everything, remove nobody if anything failed.
///
/// A task completed by somebody else between page load and submission fails its ownership
/// command on the version fence. The member stays on the team, the refusal names the item, and
/// the work that *did* move stays moved — so the retry has less to do, not more.
#[tokio::test]
async fn a_failed_item_leaves_the_member_in_place_and_names_what_is_left() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let taker = TaskOwner::Human(PrincipalId::new(Uuid::new_v4()));
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() = work_at_stake(taker);
    let at_stake = invite_persistence.at_stake.lock().unwrap().clone();
    let stuck = at_stake.owned_tasks[1].clone();
    let commands = MockMemberWorkCommands::over(&invite_persistence).conflicting_on(stuck.task_id);
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let refusal = use_cases
        .remove_company_team_member_with_handover(
            &commands,
            owner_id,
            company_id,
            member_id,
            Some(OwnedWorkHandover {
                new_owner: taker,
                handoff_instruction: "Chase the invoice by Friday.".into(),
            }),
        )
        .await;

    let AppError::Conflict(message) = refusal.expect_err("the batch is refused") else {
        panic!("a failed item refuses the removal as a conflict");
    };
    assert!(message.starts_with("Nobody was removed:"), "{message}");
    assert!(message.contains(&stuck.summary()), "{message}");
    assert_eq!(
        invite_persistence.members.lock().unwrap().len(),
        1,
        "nothing is removed while anything is unresolved"
    );
    // Every item was attempted rather than the batch stopping at the first failure.
    assert_eq!(commands.handovers.lock().unwrap().len(), 1);
    assert_eq!(commands.redirected.lock().unwrap().len(), 1);
    // What the admin's refreshed pane now shows: only the item that is still theirs.
    let left = invite_persistence.at_stake.lock().unwrap().clone();
    assert_eq!(left.owned_tasks, vec![stuck]);
    assert!(left.delegated_asks.is_empty());
}

/// An unusable decision — a named owner with no handoff instruction, or no decision at all
/// where tasks need one — is one refusal, before any command goes out.
#[tokio::test]
async fn an_unusable_decision_is_refused_once_and_resolves_nothing() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let taker = TaskOwner::Human(PrincipalId::new(Uuid::new_v4()));
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() = work_at_stake(taker);
    let commands = MockMemberWorkCommands::over(&invite_persistence);
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let refusal = use_cases
        .remove_company_team_member_with_handover(
            &commands,
            owner_id,
            company_id,
            member_id,
            Some(OwnedWorkHandover {
                new_owner: taker,
                handoff_instruction: "   ".into(),
            }),
        )
        .await;
    assert!(
        matches!(refusal, Err(AppError::BadRequest(_))),
        "{refusal:?}"
    );
    assert!(commands.handovers.lock().unwrap().is_empty());
    assert!(
        commands.redirected.lock().unwrap().is_empty(),
        "an unusable decision stops the whole submission, asks included"
    );
    assert_eq!(invite_persistence.members.lock().unwrap().len(), 1);

    // No decision at all is refused too, rather than read as "unassign everything".
    let undecided = use_cases
        .remove_company_team_member_with_handover(&commands, owner_id, company_id, member_id, None)
        .await;
    assert!(
        matches!(undecided, Err(AppError::BadRequest(_))),
        "{undecided:?}"
    );
    assert!(commands.handovers.lock().unwrap().is_empty());
    assert_eq!(invite_persistence.members.lock().unwrap().len(), 1);
}

/// No eligible taker is a clean refusal, not a fallback to leaving the work to nobody.
///
/// The empty candidate list is answered before the submission is even read, so the admin gets the
/// one reason that matters rather than a complaint about a field they could not have filled in.
#[tokio::test]
async fn work_with_no_eligible_taker_refuses_the_removal_and_moves_nothing() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() = MemberWorkAtStake {
        owner_candidates: Vec::new(),
        ..work_at_stake(TaskOwner::Human(PrincipalId::new(Uuid::new_v4())))
    };
    let commands = MockMemberWorkCommands::over(&invite_persistence);
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let refusal = use_cases
        .remove_company_team_member_with_handover(&commands, owner_id, company_id, member_id, None)
        .await;
    let AppError::Conflict(message) = refusal.expect_err("the removal is refused") else {
        panic!("no eligible owner is a conflict about the work, not a bad submission");
    };
    assert_eq!(message, MemberWorkAtStake::NO_ELIGIBLE_OWNER);
    assert!(commands.handovers.lock().unwrap().is_empty());
    assert!(commands.redirected.lock().unwrap().is_empty());
    assert_eq!(invite_persistence.members.lock().unwrap().len(), 1);
}

/// An agent may take the tasks, but not while an ask is at stake — and the refusal says why.
///
/// The candidate list is normally narrowed before the admin ever sees an agent, so this is the
/// backstop for a caller that built its own submission: it refuses before any command goes out
/// rather than handing a person's question to something that cannot answer it.
#[tokio::test]
async fn an_agent_cannot_be_chosen_while_an_ask_is_at_stake() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let agent = TaskOwner::Agent(PrincipalId::new(Uuid::new_v4()));
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() = work_at_stake(agent);
    let commands = MockMemberWorkCommands::over(&invite_persistence);
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let refusal = use_cases
        .remove_company_team_member_with_handover(
            &commands,
            owner_id,
            company_id,
            member_id,
            Some(OwnedWorkHandover {
                new_owner: agent,
                handoff_instruction: "Chase the invoice by Friday.".into(),
            }),
        )
        .await;
    let AppError::BadRequest(message) = refusal.expect_err("an agent cannot answer an ask") else {
        panic!("an unusable selection is a bad submission");
    };
    assert!(message.contains("agent cannot answer"), "{message}");
    assert!(
        commands.handovers.lock().unwrap().is_empty(),
        "the tasks are not moved either — one submission, all or nothing"
    );
    assert!(commands.redirected.lock().unwrap().is_empty());
    assert_eq!(invite_persistence.members.lock().unwrap().len(), 1);

    // The same agent is a perfectly good taker once the asks are gone.
    invite_persistence
        .at_stake
        .lock()
        .unwrap()
        .delegated_asks
        .clear();
    use_cases
        .remove_company_team_member_with_handover(
            &commands,
            owner_id,
            company_id,
            member_id,
            Some(OwnedWorkHandover {
                new_owner: agent,
                handoff_instruction: "Chase the invoice by Friday.".into(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(commands.handovers.lock().unwrap().len(), 2);
    assert!(invite_persistence.members.lock().unwrap().is_empty());
}

/// The bare removal still refuses live work, so a caller without the pane — a JSON route, a
/// script — cannot walk past the decision.
#[tokio::test]
async fn live_work_blocks_a_removal_that_carries_no_decision() {
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let company_id = Uuid::new_v4();
    let invite_persistence = Arc::new(MockCompanyInvitePersistence::with_members(vec![
        member_row(company_id, member_id),
    ]));
    *invite_persistence.at_stake.lock().unwrap() =
        work_at_stake(TaskOwner::Human(PrincipalId::new(Uuid::new_v4())));
    let use_cases = CompanyInviteUseCases::new(
        acme(owner_id, company_id),
        invite_persistence.clone() as Arc<dyn CompanyInvitePersistence>,
    );

    let refusal = use_cases
        .remove_company_team_member(owner_id, company_id, member_id)
        .await;
    assert!(matches!(refusal, Err(AppError::Conflict(_))), "{refusal:?}");
    assert_eq!(invite_persistence.members.lock().unwrap().len(), 1);

    // The same pre-check is what the caller reads to render the decision.
    let at_stake = use_cases
        .member_work_at_stake(owner_id, company_id, member_id)
        .await
        .unwrap();
    assert_eq!(at_stake.delegated_asks.len(), 1);
    assert_eq!(at_stake.owner_candidates.len(), 1);

    // Once it is resolved, the removal goes through unchanged.
    *invite_persistence.at_stake.lock().unwrap() = MemberWorkAtStake::default();
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
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
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
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
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
