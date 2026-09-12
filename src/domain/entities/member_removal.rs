//! What a team member still has at stake when someone tries to remove them.
//!
//! Removing a member demotes their principal to `external`, and `background_tasks` names its owner
//! by `(company_id, principal_id, kind)` — so live work they own would either break the demotion or
//! be released behind the admin's back. Neither is the product's answer: an admin is shown the work
//! and takes **one** decision about all of it — who owns it now, always somebody — the way a task
//! transfer already refuses to move work without a handoff instruction.
//!
//! Every item is resolved through a version-fenced, audited command — `TaskOwnershipOperation`
//! for an owned task, `DelegationOperation::ReassignPersonTarget` for an ask waiting on their
//! reply — so this module carries no verbs of its own, only the one decision those commands are
//! issued from and the version each of them has to be fenced against.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::task::{TaskOwner, TaskOwnerCandidate, TaskOwnershipOperation, TaskStatus};
use super::transport::PrincipalId;

/// A live task the departing member owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedTaskAtStake {
    pub task_id: Uuid,
    pub channel_id: Uuid,
    pub correlation_id: Uuid,
    pub task_type: String,
    pub status: TaskStatus,
    /// What a `TaskOwnershipCommand` for this task has to name as its `expected_version`.
    pub ownership_version: u64,
}

impl OwnedTaskAtStake {
    /// How a refusal names this task, when its own command is the one that failed.
    pub fn summary(&self) -> String {
        format!("task {} ({})", self.task_id, self.task_type)
    }
}

/// An open ask that names the departing member as the one expected to answer.
///
/// The identity is matched through `participant_identities`, because an outreach target names an
/// address or an internal channel and never a principal. An internal-channel ask is *not* one of
/// these: whoever is expected to answer it holds the child task that target spawned, which is an
/// [`OwnedTaskAtStake`] instead.
///
/// One of these is **redirected**, not cancelled: the chosen new owner is asked the same question
/// in the same thread through `DelegationOperation::ReassignPersonTarget`, which supersedes this
/// target and correlates a fresh one to it. Nobody is left waiting on an answer that stopped
/// being anybody's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatedAskAtStake {
    pub task_id: Uuid,
    /// The channel the asking task runs on — which is the channel the new owner has to be eligible
    /// on, because the redirected question goes out from it. See
    /// [`MemberWorkAtStake::owner_candidates`].
    pub channel_id: Uuid,
    pub outreach_id: Uuid,
    pub target_id: Uuid,
    /// What a `DelegationCommand` for this outreach has to name as its `expected_version`.
    pub outreach_version: u64,
    pub subject: String,
    pub asked_address: String,
    pub expires_at: DateTime<Utc>,
}

impl DelegatedAskAtStake {
    /// How a refusal names this ask, when its own command is the one that failed.
    pub fn summary(&self) -> String {
        format!("ask \"{}\" to {}", self.subject, self.asked_address)
    }
}

/// Which of these candidates could take over *everything* at stake.
///
/// Taking over a task and taking over an ask are not the same eligibility. A task owner may be a
/// person or an agent; a person-addressed ask is matched to whoever answers it through
/// `participant_identities`, and only a person principal ever has a row there —
/// `create_agent_principal_on` writes none, so an agent has no address the question could be
/// re-asked at and no way to answer it.
///
/// So with an ask at stake the one offer is narrowed to people rather than split into a second
/// picker: the admin takes one decision, and every option in it can carry all of the work. A
/// narrowing that empties the list is the honest answer — there is then nobody who can take this
/// person's work, which [`MemberWorkAtStake::NO_ELIGIBLE_OWNER`] is what a caller says about.
pub fn takers_for(
    candidates: Vec<TaskOwnerCandidate>,
    asks: &[DelegatedAskAtStake],
) -> Vec<TaskOwnerCandidate> {
    if asks.is_empty() {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|candidate| matches!(candidate.owner, TaskOwner::Human(_)))
        .collect()
}

/// The whole pre-check result: nothing here means the removal is an ordinary one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberWorkAtStake {
    pub owned_tasks: Vec<OwnedTaskAtStake>,
    pub delegated_asks: Vec<DelegatedAskAtStake>,
    /// Who may take over every one of [`Self::owned_tasks`] **and** [`Self::delegated_asks`] —
    /// the candidates each affected channel allows, intersected, narrowed by [`takers_for`], with
    /// the departing member already dropped.
    ///
    /// Intersected rather than pooled because the admin picks **one** owner for the whole batch: a
    /// candidate eligible on one item's channel and not another's would be a choice that half the
    /// commands refuse. Empty with work at stake means nobody can take this person's work over,
    /// and the removal is refused with [`Self::NO_ELIGIBLE_OWNER`] rather than falling back to
    /// leaving the work to nobody.
    pub owner_candidates: Vec<TaskOwnerCandidate>,
    /// Whether either list stopped at [`MemberWorkAtStake::MAX_PER_KIND`]. A truncated result is
    /// still non-empty, so the bound cannot become a way past the guard — it only bounds what one
    /// pane renders and what one submission resolves.
    pub truncated: bool,
}

impl MemberWorkAtStake {
    /// How many items of one kind a single pre-check reports, and so how many one submission
    /// resolves.
    pub const MAX_PER_KIND: usize = 50;

    /// Why a removal is refused when there is work at stake and no [`Self::owner_candidates`].
    ///
    /// The handover names a person or an agent and nothing else, so an empty offer is a dead end
    /// rather than a prompt: the admin has to widen somebody's access to the affected channels (or
    /// finish the work) before this person can leave. Refusing is the point — the alternative is
    /// the silent unassignment this whole flow exists to prevent.
    pub const NO_ELIGIBLE_OWNER: &'static str = concat!(
        "Nobody was removed: no teammate or agent is eligible to take over this person's work ",
        "on every channel it runs on. Give somebody access to those channels, or finish the ",
        "work, then remove them.",
    );

    pub fn is_empty(&self) -> bool {
        self.owned_tasks.is_empty() && self.delegated_asks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.owned_tasks.len() + self.delegated_asks.len()
    }
}

/// The one decision a guided removal carries: **who** takes over everything the departing member
/// holds.
///
/// One choice for all of it rather than one per item, and always a named taker — there is no
/// "leave it to nobody" arm, because that is the silent unassignment the flow exists to prevent.
/// A member whose work has no eligible taker is refused with
/// [`MemberWorkAtStake::NO_ELIGIBLE_OWNER`] instead. The mechanism underneath is unchanged and per
/// item — one version-fenced, audited command each — so what this collapses is the admin's
/// decision, not the safety of applying it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedWorkHandover {
    pub new_owner: TaskOwner,
    pub handoff_instruction: String,
}

/// The three fields one handover decides on every ownership command it produces.
///
/// Named rather than returned as a tuple: `(operation, new_owner, handoff)` at a call site is
/// exactly the argument-order mistake `TaskOwnershipCommand::validate` would then reject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipChange {
    pub operation: TaskOwnershipOperation,
    pub new_owner: TaskOwner,
    pub handoff_instruction: String,
}

impl OwnedWorkHandover {
    /// Whether this decision can be applied at all, checked once for the batch.
    ///
    /// `TaskOwnershipCommand::validate` enforces the same two rules per command; stating them here
    /// first means one blank field is one clear refusal rather than N identical failures.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.new_owner == TaskOwner::Unassigned {
            return Err("Choose the teammate or agent who takes this person's work over.");
        }
        if self.handoff_instruction.trim().is_empty() {
            return Err("Say what the new owner needs to know before handing the work over.");
        }
        Ok(())
    }

    /// How one ownership command reads this decision, identically for every task in the batch.
    pub fn ownership_change(&self) -> OwnershipChange {
        OwnershipChange {
            operation: TaskOwnershipOperation::Transfer,
            new_owner: self.new_owner,
            handoff_instruction: self.handoff_instruction.clone(),
        }
    }

    /// Who a person-addressed ask is re-asked at, or why this decision cannot carry one.
    ///
    /// `DelegationOperation::ReassignPersonTarget` replaces one `participant_identities`-matched
    /// target with another, and only a person principal has rows there — see [`takers_for`], which
    /// is why a candidate list offered alongside an ask never contains an agent in the first
    /// place. This is the same rule restated where the command is built, so a caller that skipped
    /// the narrowing gets a refusal rather than a target nobody can answer.
    pub fn ask_recipient(&self) -> Result<PrincipalId, &'static str> {
        match self.new_owner {
            TaskOwner::Human(principal_id) => Ok(principal_id),
            TaskOwner::Agent(_) => Err(
                "An agent cannot answer a question addressed to a person: choose a teammate to \
                 take this person's open asks over.",
            ),
            TaskOwner::Unassigned => {
                Err("Choose the teammate who takes this person's open asks over.")
            }
        }
    }
}

/// One item a submission's own command failed on, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedWorkItem {
    /// What it is, as [`OwnedTaskAtStake::summary`] or [`DelegatedAskAtStake::summary`] names it.
    pub summary: String,
    pub failure: String,
}

/// What one guided removal could not clear.
///
/// Collected rather than returned on the first failure: the admin is told about every item that
/// still needs them, and — because a partial batch is never a partial removal — the member is
/// still on the team when they read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnresolvedWork {
    pub items: Vec<UnresolvedWorkItem>,
}

impl UnresolvedWork {
    /// How many failures a refusal names one by one before it stops.
    pub const MAX_NAMED: usize = 5;

    pub fn push(&mut self, summary: String, failure: String) {
        self.items.push(UnresolvedWorkItem { summary, failure });
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Why the removal was refused, naming the items rather than counting them.
    ///
    /// The first sentence is the policy: nothing was removed. A version conflict here means
    /// somebody moved one of these while the pane was open, so the answer is always "look again",
    /// never "we removed them anyway".
    pub fn message(&self) -> String {
        let named: Vec<String> = self
            .items
            .iter()
            .take(Self::MAX_NAMED)
            .map(|item| format!("{} — {}", item.summary, item.failure))
            .collect();
        let rest = self.items.len().saturating_sub(named.len());
        let tail = if rest > 0 {
            format!(" (and {rest} more)")
        } else {
            String::new()
        };

        format!(
            "Nobody was removed: {count} of this person's item(s) could not be handed over. \
             {named}{tail}. Refresh this pane to see what is still theirs, then try again.",
            count = self.items.len(),
            named = named.join("; "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::transport::PrincipalId;

    fn ask() -> DelegatedAskAtStake {
        DelegatedAskAtStake {
            task_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            outreach_id: Uuid::new_v4(),
            target_id: Uuid::new_v4(),
            outreach_version: 1,
            subject: "Can you confirm the invoice?".into(),
            asked_address: "leaver@example.com".into(),
            expires_at: Utc::now(),
        }
    }

    #[test]
    fn a_truncated_result_still_blocks_a_removal() {
        let full = MemberWorkAtStake {
            owned_tasks: Vec::new(),
            delegated_asks: vec![ask()],
            owner_candidates: Vec::new(),
            truncated: true,
        };
        assert!(!full.is_empty());
        assert_eq!(full.len(), 1);
        assert!(MemberWorkAtStake::default().is_empty());
    }

    /// The one decision is checked once, and reads as exactly the command each task needs.
    ///
    /// There is no release arm to check: the handover always names a taker, and "nobody" is a
    /// refusal rather than an option.
    #[test]
    fn a_handover_always_names_a_taker_and_reads_as_one_transfer() {
        let principal = PrincipalId::new(Uuid::new_v4());
        let transfer = OwnedWorkHandover {
            new_owner: TaskOwner::Human(principal),
            handoff_instruction: "Chase the invoice by Friday.".into(),
        };
        assert_eq!(transfer.check(), Ok(()));
        assert_eq!(
            transfer.ownership_change(),
            OwnershipChange {
                operation: TaskOwnershipOperation::Transfer,
                new_owner: TaskOwner::Human(principal),
                handoff_instruction: "Chase the invoice by Friday.".into(),
            }
        );
        assert_eq!(transfer.ask_recipient(), Ok(principal));

        // A named owner without a handoff, and "transfer to nobody", are both refused before any
        // command goes out — one message, not one per task.
        assert!(
            OwnedWorkHandover {
                new_owner: TaskOwner::Human(principal),
                handoff_instruction: "   ".into(),
            }
            .check()
            .is_err()
        );
        assert!(
            OwnedWorkHandover {
                new_owner: TaskOwner::Unassigned,
                handoff_instruction: "anything".into(),
            }
            .check()
            .is_err()
        );
    }

    /// An agent may own a task but can never be asked a person's question.
    ///
    /// Both halves of the rule: the offer is narrowed before the admin sees it, and the command
    /// builder refuses an agent even if a caller skipped the narrowing.
    #[test]
    fn an_agent_is_not_offered_or_accepted_while_an_ask_is_at_stake() {
        let agent = TaskOwnerCandidate {
            owner: TaskOwner::Agent(PrincipalId::new(Uuid::new_v4())),
            label: "Invoice bot".into(),
        };
        let human = TaskOwnerCandidate {
            owner: TaskOwner::Human(PrincipalId::new(Uuid::new_v4())),
            label: "Dana".into(),
        };
        let both = vec![agent.clone(), human.clone()];

        // Tasks only: an agent is a perfectly good task owner, so the offer is untouched.
        assert_eq!(takers_for(both.clone(), &[]), both);
        // With an ask at stake the agent is not an option at all.
        assert_eq!(takers_for(both, &[ask()]), vec![human]);
        // And narrowing to nothing is allowed to happen — the caller refuses rather than falls back.
        assert!(takers_for(vec![agent.clone()], &[ask()]).is_empty());

        assert!(
            OwnedWorkHandover {
                new_owner: agent.owner,
                handoff_instruction: "Chase the invoice by Friday.".into(),
            }
            .ask_recipient()
            .is_err(),
            "an agent has no participant_identities row to re-ask through"
        );
    }

    /// A refusal names the items, and says nothing was removed.
    #[test]
    fn unresolved_work_names_what_is_still_outstanding() {
        let mut unresolved = UnresolvedWork::default();
        assert!(unresolved.is_empty());

        for index in 0..UnresolvedWork::MAX_NAMED + 2 {
            unresolved.push(format!("task {index}"), "version conflict".into());
        }
        let message = unresolved.message();
        assert!(message.starts_with("Nobody was removed:"), "{message}");
        assert!(message.contains("task 0 — version conflict"), "{message}");
        assert!(
            message.contains("(and 2 more)"),
            "the list is bounded: {message}"
        );
        assert!(
            !message.contains("task 6"),
            "only the first {} are named: {message}",
            UnresolvedWork::MAX_NAMED
        );
    }
}
