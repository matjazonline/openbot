# Phase 2 — The pre-check, the guard, and the one-decision batch

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal.** One read that says what a member still has at stake and who could take it over; one
method that applies a single decision to all of it and only then removes them; and a bare removal
that still refuses while anything is live. Removals with nothing at stake behave exactly as today.

**Files touched**

| File | Change |
|---|---|
| `src/domain/entities/member_removal.rs` | `MemberWorkAtStake`, `OwnedTaskAtStake`, `DelegatedAskAtStake`, `OwnedWorkHandover`, `OwnershipChange`, `takers_for`, `UnresolvedWork` |
| `src/domain/entities/mod.rs` | register the module |
| `src/application/use_cases/company_invite.rs` | pre-check port method, `MemberWorkCommands` port, `member_work_at_stake`, `remove_company_team_member_with_handover`, the guard in `remove_company_team_member` |
| `src/application/use_cases/company_invite_tests.rs` | the module's tests, hoisted out of the inline `mod tests` once it passed ~500 lines |
| `src/adapters/persistence/company_invite.rs` | the two queries and the shared candidate list, DB tests |

---

## 2.1 The entity (`src/domain/entities/member_removal.rs`)

Named structs, not tuples, because a UI has to render a link per item:

```rust
pub struct OwnedTaskAtStake {
    pub task_id: Uuid,
    pub channel_id: Uuid,
    pub correlation_id: Uuid,
    pub task_type: String,
    pub status: TaskStatus,
    pub ownership_version: u64,
}

pub struct DelegatedAskAtStake {
    pub task_id: Uuid,
    /// The asking task's channel: the redirected question goes out from it, so whoever answers
    /// has to be eligible there. An affected channel like any owned task's.
    pub channel_id: Uuid,
    pub outreach_id: Uuid,
    pub target_id: Uuid,
    pub outreach_version: u64,
    pub subject: String,
    pub asked_address: String,
    pub expires_at: DateTime<Utc>,
}

pub struct MemberWorkAtStake {
    pub owned_tasks: Vec<OwnedTaskAtStake>,
    pub delegated_asks: Vec<DelegatedAskAtStake>,
    /// One list for the whole batch: the intersection of what each affected channel allows,
    /// minus the departing principal, narrowed by `takers_for`. Empty with work at stake means
    /// the removal is refused (`MemberWorkAtStake::NO_ELIGIBLE_OWNER`).
    pub owner_candidates: Vec<TaskOwnerCandidate>,
    pub truncated: bool,
}
```

`MAX_PER_KIND = 50`, `is_empty()`, `len()`. `truncated` is set when either query came back at the
bound; a truncated result is still non-empty, so it blocks. Candidates are **not** per task: the
admin makes one choice, so there is one list (see the intersection decision in the shared
instructions). `OwnedTaskAtStake::summary()` / `DelegatedAskAtStake::summary()` are how a refusal
names an item.

The one decision — **always a named taker**, never "nobody" — and how a command reads it:

```rust
pub struct OwnedWorkHandover {
    pub new_owner: TaskOwner,
    pub handoff_instruction: String,
}

pub struct OwnershipChange {
    pub operation: TaskOwnershipOperation,   // always Transfer
    pub new_owner: TaskOwner,
    pub handoff_instruction: String,
}
```

- There is no `Unassign` arm and no `UNASSIGN_ALL` wire value. An *empty* submission is still "no
  decision arrived", which is a `BadRequest`; it never reads as "leave the work to nobody".
- `check()` is the pure rule, stated once for the batch instead of failing N identical commands: a
  named owner needs a non-blank handoff instruction, and `TaskOwner::Unassigned` is not a decision.
  `TaskOwnershipCommand::validate` enforces the same two rules per command.
- `ownership_change()` is the only place the decision becomes an operation, so every task in one
  batch is handed over identically.
- `ask_recipient()` is the same decision read as an ask's new target: `Ok(PrincipalId)` for a
  human, an error for an agent or for nobody. It restates `takers_for`'s rule where the command is
  built, so a caller that skipped the narrowing is refused rather than given a target nobody can
  answer.

```rust
pub fn takers_for(candidates: Vec<TaskOwnerCandidate>, asks: &[DelegatedAskAtStake])
    -> Vec<TaskOwnerCandidate>;
```

With no ask at stake the offer is untouched (an agent owns tasks perfectly well). With **any** ask
at stake it is narrowed to `TaskOwner::Human`, because only a person principal has
`participant_identities` rows — see the investigation in the shared instructions. A narrowing that
empties the list is the correct outcome, and `MemberWorkAtStake::NO_ELIGIBLE_OWNER` is what the
caller then says.

The failure collection, also pure:

```rust
pub struct UnresolvedWorkItem { pub summary: String, pub failure: String }
pub struct UnresolvedWork { pub items: Vec<UnresolvedWorkItem> }   // push / is_empty / message
```

`message()` starts with "Nobody was removed:", names the first `MAX_NAMED` (5) items with their
errors, counts the rest, and tells the reader to refresh the pane.

## 2.2 The ports

On `CompanyInvitePersistence` (a read, so it sits with the other member reads):

```rust
async fn member_work_at_stake(&self, company_id: Uuid, user_id: Uuid)
    -> AppResult<MemberWorkAtStake>;
```

And, in the same use-case module, the commands the batch issues — declared as a port because
`ThreadUseCases` owns both of them and because a test has to be able to make one item fail:

```rust
#[async_trait]
pub trait MemberWorkCommands: Send + Sync {
    async fn hand_over_owned_task(&self, task: &OwnedTaskAtStake, handover: &OwnedWorkHandover)
        -> AppResult<()>;
    async fn redirect_delegated_ask(&self, ask: &DelegatedAskAtStake, new_owner: PrincipalId)
        -> AppResult<()>;
}
```

No default methods: an implementation that silently succeeded would turn a refused handover into a
removal. It is implemented in the route layer (Phase 3) over the existing commands.

## 2.3 The queries (`src/adapters/persistence/company_invite.rs`)

The member's principal first (`kind = 'person'`, `user_id = $2`); no principal means nothing at
stake, and an empty result is returned without two more round-trips.

**Owned tasks** — `background_tasks` where `owner_principal_id` is that principal,
`owner_principal_kind = 'person'` and the status is one of the five live ones, ordered by
`created_at, id`, `LIMIT MAX_PER_KIND + 1`.

**Delegated asks** — `task_outreach_targets` joined to `task_outreaches`, `background_tasks` and
`participant_identities` on `(transport, namespace, subject)`:

```sql
JOIN participant_identities AS identity
  ON identity.company_id = target.company_id
 AND identity.principal_id = $3
 AND identity.transport = target.external_transport
 AND identity.namespace = target.external_namespace
 AND identity.subject   = target.external_subject
```

with `target.status = 'active'`, `outreach.status IN ('waiting','timeout_pending_approval')` and
`task.status NOT IN ('completed','stopped')`. `DISTINCT` on the target, because a person can hold
more than one identity row for the same address shape.

The ask row also carries `task.channel_id`, because an ask now needs a taker of its own.

**Candidates** — `shared_owner_candidates` calls the existing
`TaskPersistence::list_task_owner_candidates(self, company_id, channel_id)` once per *distinct*
channel among the owned tasks **and the asks** (bounded by the 50 of each above), drops the
departing principal, **intersects** the lists by `candidate.owner`, and finally applies
`takers_for` so an agent drops out whenever an ask is at stake. One read per channel, one list out;
no second copy of that eligibility SQL and no second picker.

## 2.4 The use case (`src/application/use_cases/company_invite.rs`)

```rust
pub async fn member_work_at_stake(&self, user_id, company_id, member_user_id)
    -> AppResult<MemberWorkAtStake>
```

owner-gated exactly like its siblings, and `self` never removes themselves (the same guard
`remove_company_team_member` has), so the pane cannot be asked about the owner.

```rust
pub async fn remove_company_team_member_with_handover(
    &self,
    commands: &dyn MemberWorkCommands,
    user_id: Uuid,
    company_id: Uuid,
    member_user_id: Uuid,
    handover: Option<OwnedWorkHandover>,
) -> AppResult<()>
```

in order:

1. `member_work_at_stake(..)` — authorization, the owner-row refusal, and the versions every
   command fences on, in one read at submission time.
2. Nothing at stake → straight to `remove_company_team_member(..)`; the plain removal is untouched.
3. Work at stake but `owner_candidates` empty → `AppError::Conflict(NO_ELIGIBLE_OWNER)`, logged.
   No command goes out and there is no unassign fallback: the admin has to widen somebody's channel
   access or finish the work.
4. `checked_handover(..)` answers every unusable submission at once: `handover` must be `Some`
   (otherwise `BadRequest` — `None` must never read as "leave it to nobody"), must pass `check()`,
   and — when any ask is at stake — must yield an `ask_recipient()`. All before a single command.
5. One `hand_over_owned_task` per task, then one `redirect_delegated_ask(ask, recipient)` per ask.
   Every item is attempted; each failure is pushed onto `UnresolvedWork` with the item's
   `summary()`.
6. If anything failed: `AppError::Conflict(unresolved.message())`, logged with the count. **The
   member stays.**
7. Otherwise `remove_company_team_member(..)`, which re-runs the pre-check — so work that arrived
   between step 1 and here refuses the removal instead of being demoted out from under.

`remove_company_team_member` keeps the guard it gained in this phase, with the message now pointing
at the pane that asks the question:

```rust
let at_stake = self.invite_persistence.member_work_at_stake(company_id, member_user_id).await?;
if !at_stake.is_empty() {
    return Err(AppError::Conflict(...));
}
```

propagated with `?` — this is an authorization-shaped decision and must not collapse to a default
(`src/AGENTS.md`, "Don't collapse errors into defaults").

## 2.5 Tests

Use-case level, against the existing `MockCompanyInvitePersistence` plus a `MockMemberWorkCommands`
whose successes clear their own item from the pre-check (which is what the real commands do):

- nothing at stake → `remove_member` is called (unchanged behaviour);
- one decision, two tasks and one ask → one command per task carrying the *same* handover, the ask
  **redirected at that same taker**, then the member removed;
- work at stake with an empty candidate list → `Conflict(NO_ELIGIBLE_OWNER)`, nothing attempted;
- an agent chosen while an ask is at stake → `BadRequest` before any command, and the same agent
  accepted once the asks are gone;
- **partial failure** → one task's command refuses (a version conflict); every other item is still
  attempted, the error is a `Conflict` naming the stuck item, the member is **still on the team**,
  and the items that succeeded are gone from the pre-check the refreshed pane reads;
- an unusable decision (blank handoff, or no decision where tasks need one) → `BadRequest`, and
  nothing was resolved — asks included;
- the bare `remove_company_team_member` still refuses live work.

Persistence level (shared DB, ids scoped):

- a member owning one live task and one `completed` task → only the live one comes back, with the
  shared candidate list and without the departing member in it;
- a member owning tasks on two channels, one of them an allowlist channel → `owner_candidates` is
  the intersection: the company owner, and not the teammate who is only eligible on the open one;
- a member with an active person-addressed outreach target → one `delegated_asks` row carrying the
  asking task's channel and the outreach version the redirect command needs, plus a candidate list
  drawn from that channel;
- an agent assigned to the affected channel → offered while only tasks are at stake, absent once an
  ask is, with the agent principal proved to hold **zero** `participant_identities` rows;
- a member with neither → empty, and `remove_member` still succeeds.
