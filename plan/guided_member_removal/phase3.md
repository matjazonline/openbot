# Phase 3 — The Team pane asks one question and removes in one click

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal.** The member pane shows the live work, asks the one thing there is to decide — who takes
all of it over — and "Remove from Team" applies it to everything and completes the removal. Every
option in that one control names a person or an agent; there is no "unassign".

**Files touched**

| File | Change |
|---|---|
| `src/adapters/http/pages/team_settings.rs` | `MemberPane.work_at_stake`, the one-decision section, the removal form |
| `src/adapters/http/routes/ui_team.rs` | the pane loads the pre-check; one removal route; `TeamWorkCommands` |
| `src/adapters/http/pages/tests.rs` | pane tests |

---

## 3.1 The pane (`team_settings.rs`)

`MemberPane` carries `work_at_stake: Option<&'a MemberWorkAtStake>` — `None` for a pane with no
authority to act (a member looking at a colleague), so a non-owner's pane is unchanged.
`MemberPane::at_stake()` is the one place that asks "is there live work *and* may this viewer
remove anybody": it returns `None` for an empty pre-check too, so "nothing at stake" renders
exactly like "no pre-check at all".

`removal_block` renders **one `<form hx-post=…/removal>`** containing, in order, the work section
and the footer with the Remove submit button. One form means one decision point and one action;
there is no state where some items are resolved and the button is waiting to unlock. A viewer who
cannot remove anybody gets the footer alone, with no button and no form.

When `at_stake()` is `Some`:

- A warning section headed `Live work to hand over (N)`, N being every item.
- **The one decision**, rendered whenever anything is at stake — asks included, because an ask now
  needs a taker too:
  - `<select name="owner" required>` — a disabled placeholder, then one option per
    `at_stake.owner_candidates` (`human:<uuid>` / `agent:<uuid>`). No "unassigned" option exists.
    The list is already narrowed to people when an ask is at stake (`takers_for`), so the admin
    cannot pick an agent that could not answer.
  - With **no** candidates there is no picker at all: the section says this person cannot be
    removed yet and what to do about it. Clicking Remove gets the same answer from the use case
    (`NO_ELIGIBLE_OWNER`) rather than falling back to anything.
  - one `<textarea name="handoff_instruction">`, shared by every item. Not marked `required` in
    HTML — `OwnedWorkHandover::check()` is the one place that rule lives, and a blank one comes
    back as the pane's error banner.
- **The tasks**, listed for reading: status, type and a link into the board
  (`/ui/tasks?company_id=…&view=board&correlation_id=…`). No control per row.
- **The asks**: `N pending ask(s) to this person will be redirected to the chosen owner.` plus
  subject, address and deadline per row, each linking to its task. No control of their own — the
  one selection above already said who answers them, and `ReassignPersonTarget` carries it.
- The truncation note, when the pre-check was bounded, says this removal resolves the first 50 of
  each kind and that removing again picks up the rest.

`hx-confirm` states what the one click does: *"Remove sam from the Acme team? 3 active task(s) and
2 pending ask(s) to them move to your chosen owner."* — and the plain "they lose access" sentence
when nothing is at stake.

## 3.2 The routes (`ui_team.rs`)

`Workspace` carries `thread_use_cases: Arc<ThreadUseCases>` — the use case that already owns both
commands, so the Team tab submits the identical command the Tasks workspace does.

```
POST /ui/companies/{company_id}/team/members/{user_id}/removal
```

is the **only** removal route on this tab. It replaced `DELETE …/members/{user_id}` (a removal now
carries a body: the decision) and the three per-item resolution routes, which are gone — there is no
endpoint left that resolves one item, and none that removes somebody without saying where their work
goes.

The handler:

1. `parse_handover(&form)` — `MemberRemovalForm { owner, handoff_instruction }`, both optional.
   Blank/absent `owner` → `None` ("no decision arrived"); `human:<uuid>` / `agent:<uuid>` → an
   `OwnedWorkHandover` with the submitted instruction. There is no "nobody" value to parse.
2. `Workspace::acting_principal` → the admin's own principal, which the audit trail records.
3. `remove_company_team_member_with_handover(&TeamWorkCommands { … }, …)`.
4. Success → `cleared_response()` (empty pane, list refreshed, URL cleared). Failure → the member's
   pane with the error in its banner; because the pane re-reads the pre-check, the reader sees
   exactly what is still outstanding. A refusal is never a 500.

`TeamWorkCommands { thread_use_cases, company, actor }` implements `MemberWorkCommands`:

- task: `handover.ownership_change()` fills `operation` / `new_owner` / `handoff_instruction`, and
  the command goes out with `authority: Manager`, `reason: OwnerUnavailable`, a fresh `command_id`
  and `expected_version: task.ownership_version` — the version the batch read moments earlier.
- ask: load the parent task (scoped to this company), then `DelegationCommand` with
  `authority: CompanyManager`,
  `operation: ReassignPersonTarget { outreach_id, target_id, new_principal_id }`,
  `reason: TargetUnavailable`, a fresh `command_id` and `expected_version: ask.outreach_version`.
  `new_principal_id` is the batch's one `ask_recipient()`, so every ask goes to the same person the
  tasks did.

## 3.3 Tests (`pages/tests.rs`)

- a pane with a task and an ask at stake: one `hx-post=…/removal`, exactly one `name="owner"` and
  one `name="handoff_instruction"`, **no** `value="unassigned"` anywhere, the items listed without
  controls, no `/work/task/*` or `/work/delegation` endpoint anywhere, and the Remove button still
  present;
- asks only → still exactly one picker, and the "redirected to the chosen owner" line;
- no shared candidates → no picker at all, and the "cannot be removed yet" explanation;
- an empty `MemberWorkAtStake` renders the plain removal exactly as before (the existing
  `the_member_pane_offers_remove_to_the_owner_and_never_for_the_owner` keeps passing);
- a non-owner's pane renders neither the section nor the button, even when handed a pre-check.
