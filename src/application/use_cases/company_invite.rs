use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company_invite::CompanyInvite,
        company_member::{CompanyAccessRole, CompanyMember},
        member_removal::{
            DelegatedAskAtStake, MemberWorkAtStake, OwnedTaskAtStake, OwnedWorkHandover,
            UnresolvedWork,
        },
        transport::PrincipalId,
        user::User,
    },
    use_cases::company::{CompanyPersistence, company_not_found, owned_company},
};

/// An invite belonging to another company is reported exactly like a missing one, so an id probe
/// cannot tell a foreign invite from a nonexistent one. See [`owned_company`].
fn invite_not_found() -> AppError {
    AppError::NotFound("Invite not found in this company.".into())
}

/// Why a bare removal was refused, in the words a caller without the pane can act on.
fn unresolved_work_message(at_stake: &MemberWorkAtStake) -> String {
    let tasks = at_stake.owned_tasks.len();
    let asks = at_stake.delegated_asks.len();
    format!(
        "This person still has live work: {tasks} task(s) they own and {asks} ask(s) waiting on \
         their reply. Remove them from the Team pane, which asks who takes all of it over and \
         re-asks the open asks at them."
    )
}

/// The one decision, checked against everything it is about to be applied to.
///
/// Named rather than returned as a pair because the second field is the *consequence* of the
/// first: who each ask is re-asked at, resolved once for the batch, and absent exactly when there
/// are no asks to re-ask.
struct CheckedHandover {
    handover: OwnedWorkHandover,
    ask_recipient: Option<PrincipalId>,
}

/// Every reason a submission is unusable, answered before a single command goes out.
///
/// All of these are `BadRequest`s about the submission, and none of them has moved anything — as
/// opposed to the `Conflict` a command that actually refused produces.
fn checked_handover(
    at_stake: &MemberWorkAtStake,
    handover: Option<OwnedWorkHandover>,
) -> AppResult<CheckedHandover> {
    // `None` is "the submission carried no decision", which is a refusal for a member who holds
    // anything at all rather than a silent unassignment.
    let handover = handover.ok_or_else(|| {
        AppError::BadRequest("Choose who takes this person's live work over.".into())
    })?;
    handover
        .check()
        .map_err(|message| AppError::BadRequest(message.into()))?;
    let ask_recipient = if at_stake.delegated_asks.is_empty() {
        None
    } else {
        Some(
            handover
                .ask_recipient()
                .map_err(|message| AppError::BadRequest(message.into()))?,
        )
    };
    Ok(CheckedHandover {
        handover,
        ask_recipient,
    })
}

/// The commands a guided removal issues on the work it is clearing.
///
/// Ownership and delegation commands are owned by `ThreadUseCases`; naming them as a port here
/// keeps this use case out of how they are executed, and lets a test drive a single item's failure
/// — which is the only way to exercise the policy that a partial batch is never a partial removal.
///
/// No default methods: an implementation that silently succeeded would turn a refused handover into
/// a removal.
#[async_trait]
pub trait MemberWorkCommands: Send + Sync {
    /// Apply the batch's one decision to one task, fenced on the version the pre-check just read.
    async fn hand_over_owned_task(
        &self,
        task: &OwnedTaskAtStake,
        handover: &OwnedWorkHandover,
    ) -> AppResult<()>;
    /// Re-ask one question addressed to the departing member at the taker the batch chose,
    /// fenced on the outreach version the pre-check just read.
    ///
    /// The same decision as the tasks, applied to an ask: nobody is left waiting on an answer
    /// that stopped being anybody's job, and nobody's question is dropped either.
    async fn redirect_delegated_ask(
        &self,
        ask: &DelegatedAskAtStake,
        new_owner: PrincipalId,
    ) -> AppResult<()>;
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
    /// The live work removing this member would move: tasks they own, and asks still waiting on
    /// their reply. A read; resolving any of it goes through the ownership and delegation commands
    /// that already own those transitions.
    async fn member_work_at_stake(
        &self,
        company_id: Uuid,
        user_id: Uuid,
    ) -> AppResult<MemberWorkAtStake>;
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

    /// What removing this member would move, so the caller can present it and ask for the one
    /// decision the removal needs — who takes the tasks over — before asking for the removal.
    ///
    /// Same authority as the removal it precedes, and the same refusal for the owner's own row:
    /// asking "what would removing the owner move" is asking about something that never happens.
    #[instrument(skip(self))]
    pub async fn member_work_at_stake(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
    ) -> AppResult<MemberWorkAtStake> {
        self.verify_company_owner(user_id, company_id).await?;

        if user_id == member_user_id {
            return Err(AppError::Internal(
                "Cannot remove company owner from the team.".into(),
            ));
        }

        self.invite_persistence
            .member_work_at_stake(company_id, member_user_id)
            .await
    }

    /// Remove a member, applying one decision to everything they still hold.
    ///
    /// The admin's decision is single; the mechanism is not. Every owned task is still its own
    /// version-fenced, idempotent, audited `TaskOwnershipCommand` and every ask its own
    /// `ReassignPersonTarget`, each built from the versions this method reads *now* rather than
    /// from whatever the pane was rendered with.
    ///
    /// **A partial batch is never a partial removal.** Every item is attempted, failures are
    /// collected, and if anything failed the member stays on the team and the refusal names what
    /// is still theirs. The alternative — removing them with some work unresolved — is the silent
    /// unassignment this whole flow exists to prevent.
    #[instrument(skip(self, commands))]
    pub async fn remove_company_team_member_with_handover(
        &self,
        commands: &dyn MemberWorkCommands,
        user_id: Uuid,
        company_id: Uuid,
        member_user_id: Uuid,
        handover: Option<OwnedWorkHandover>,
    ) -> AppResult<()> {
        // Authorization, the owner-row refusal, and the fresh versions every command fences on,
        // in one read.
        let at_stake = self
            .member_work_at_stake(user_id, company_id, member_user_id)
            .await?;
        if at_stake.is_empty() {
            // Nothing to decide, so nothing to submit: the ordinary removal, which runs the
            // pre-check itself.
            return self
                .remove_company_team_member(user_id, company_id, member_user_id)
                .await;
        }
        // No taker every affected channel accepts. Refused rather than resolved somehow: the one
        // selection names a person or an agent, and there is nothing else it can say.
        if at_stake.owner_candidates.is_empty() {
            info!(
                company_id = %company_id,
                member_user_id = %member_user_id,
                at_stake = at_stake.len(),
                "Guided removal refused: no eligible owner for this member's work"
            );
            return Err(AppError::Conflict(
                MemberWorkAtStake::NO_ELIGIBLE_OWNER.into(),
            ));
        }
        let checked = checked_handover(&at_stake, handover)?;

        let mut unresolved = UnresolvedWork::default();
        for task in &at_stake.owned_tasks {
            if let Err(error) = commands.hand_over_owned_task(task, &checked.handover).await {
                unresolved.push(task.summary(), error.to_string());
            }
        }
        if let Some(recipient) = checked.ask_recipient {
            for ask in &at_stake.delegated_asks {
                if let Err(error) = commands.redirect_delegated_ask(ask, recipient).await {
                    unresolved.push(ask.summary(), error.to_string());
                }
            }
        }
        if !unresolved.is_empty() {
            info!(
                company_id = %company_id,
                member_user_id = %member_user_id,
                unresolved = unresolved.items.len(),
                "Guided removal refused: work is still at stake"
            );
            return Err(AppError::Conflict(unresolved.message()));
        }

        // The guard runs again inside: what this cleared is what it just read, so work that
        // arrived in between refuses the removal rather than being demoted out from under.
        self.remove_company_team_member(user_id, company_id, member_user_id)
            .await
    }

    /// Remove a member, once nothing of theirs is still live.
    ///
    /// The pre-check is run here rather than left to the UI so no caller — a JSON route, a script,
    /// a future flow — can demote somebody out from under work that is still running. What the
    /// caller is expected to do with the refusal is take the one decision
    /// [`Self::remove_company_team_member_with_handover`] asks for and go through that instead.
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

        // Propagated, never defaulted: a failed pre-check must not read as "nothing at stake".
        let at_stake = self
            .invite_persistence
            .member_work_at_stake(company_id, member_user_id)
            .await?;
        if !at_stake.is_empty() {
            return Err(AppError::Conflict(unresolved_work_message(&at_stake)));
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
#[path = "company_invite_tests.rs"]
mod tests;
