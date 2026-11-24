# AWC-P02-D1 r3 — architecture gate: admission checks placed, hint source settled

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22. Answers review
`AWC-P02-D1-R2-N1`, findings F6 and F7.

**r3 is [r2](AWC-P02-D1-r2-decision.md) with the replacements below.** r2 and r1 are kept
unchanged. Everything not replaced here is r2's text and stays in force. r2's D3 step 1 is
amended, and its D3 steps 4–8 and D4 item 1 are replaced.

## Facts added

- Gate AWC-P01-D1 r3 D1.1 orders the remaining admission checks:
  - step 6, intake: errors `intake_not_declared` and `intake_invalid`;
  - step 7, levels: the error `audience_refused`;
  - step 8, destination admission: `run_as` authority, `bindings::verify_selection`, readiness
    and role resolution give `destination_unavailable`; the unit spend cap gives
    `budget.unit_exhausted`; tree limits give the error `workflow.tool_limit`.

  It says steps 5–8 are "re-read at launch". In this design, first dispatch and launch are the
  same transaction, so a single read satisfies that.
- `knowledge::actor_level_on(company, Some(run_as))` (`knowledge.rs:72`) gives the `run_as`
  level. It reads under the account lock the transaction already holds.
- `unit_budgets::check_admission_on(company, run)` (`unit_budgets.rs:159`) charges the run's
  captured write unit. Admission runs it after `run_scopes::capture_on` and rolls the whole
  transaction back on a refusal (`admission_write.rs:66-74`).
- The role refusal from manifest capture is `AppError::WorkflowRole`. Admission records it as a
  refusal (`admission.rs:88-95`).
- `RunKnowledge::new` / `knowledge::stored_levels(ceiling, content)` refuse content above the
  ceiling (`children.rs:341`).

## D3 step 1, amended (F7)

The `Purpose::ChannelCall { desk }` source is one statement:

```sql
SELECT settings.revision, configuration.run_as_user_id
  FROM channel_reply_settings AS settings
  JOIN channels AS desk ON desk.company_id = settings.company_id AND desk.id = settings.channel_id
  JOIN workflow_binding_revisions AS configuration
    ON configuration.company_id = settings.company_id
   AND configuration.binding_id = settings.binding_id
   AND configuration.revision = settings.binding_revision
 WHERE settings.company_id = $caller_company AND settings.channel_id = $desk
```

- It is scoped to the caller's company. A foreign or missing desk yields no hint, so no account
  is locked for it, and step 4 then gives `target_unavailable`.
- A missing `run_as` account is not a refusal of the caller. The ChannelCall source marks its
  `run_as` as optional in the union. If the lock helper returns the union without it (retired or
  missing account), the prelock accepts that, as it accepts a recorded source today. The base
  accounts (caller actor, requester) must still all be locked, unchanged.
- Step 5 then distinguishes the cases. The locked settings name a `run_as` that is not in the
  locked set:
  - if the account is not live, the denied closure `channel_call.destination_unavailable`;
  - if it is live (the settings changed since the hint), `Conflict`.

## D3 steps 4–8, replaced (F6)

Every step below runs inside the dispatch transaction, in this order. Each step runs only if the
previous ones passed.

4. **Desk facts.** As r2: the channel and grant share locks, the archived unit, the ancestry
   walk, and settings under the share lock. `decide_target` yields:
   - the errors `target_unavailable` and `cycle` (D1.1 steps 3–4);
   - the denied closures `unit.archived`, `desk_disabled` and `no_responder` (step 5).
5. **Settings check** (as amended above): `Conflict` or `destination_unavailable`.
6. **Pinned revision and intake.** Restore the revision with `binding_rows::read_binding_at`.
   Then run p01's `check_intake(input_schema, fields)` against it. Its errors
   `intake_not_declared` and `intake_invalid` (with schema and paths) are tool errors (D1.1
   step 6).
7. **Levels.**
   - `run_as` level = `actor_level_on(company, Some(run_as))`.
   - Child ceiling = p01 `child_ceiling(caller ceiling, ALLOWANCE_WITHOUT_REQUEST_RULES, run_as
     level)`, where `ALLOWANCE_WITHOUT_REQUEST_RULES` is `PUBLIC`.
   - The request's level and exactness are the calling run's current `content_level` and
     `content_exact`, read under the fence.
   - p01 `release_request(content, ceiling)`: a request above the ceiling is the error
     `audience_refused` (D1.1 step 7).

   This runs before any child write, so `stored_levels` can never refuse later.
8. **Destination admission (D1.1 step 8, before the child write):**
   - `management::authorize_association_on(run_as, inbox(desk))`;
   - `bindings::verify_selection` on the restored configuration;
   - `resources::association_reach_on` + `check_readiness` (stage Admission);
   - `ChannelReplyContract::validate`.

   Each failure is the denied closure `destination_unavailable`.

   Tree limits stay where they are enforced today: p03's dispatch checks the continuation count,
   as `workflow_tool_calls::call_on` does, and the child writer's existing depth/count checks
   are unchanged. p03 maps them to `workflow.tool_limit` (AWC-C003).
9. **Child write under a savepoint.** `SAVEPOINT channel_call_child`, then D4: run row,
   manifest capture (where role resolution happens), unit-scope capture, job, admission and
   wait, and the record. Then `unit_budgets::check_admission_on(company, child)`.
   - On `AppError::WorkflowRole` from capture: `ROLLBACK TO SAVEPOINT` and the denied closure
     `destination_unavailable` (D1.1 step 8 names role resolution there).
   - On the unit-cap refusal: `ROLLBACK TO SAVEPOINT` and the denied closure
     `budget.unit_exhausted`.

   In both cases nothing of the child survives, and the fence and the caller's locks stay held.
   Any other error propagates and the dispatch transaction rolls back as a whole.

## D4 item 1, replaced (F6)

The run row of a `ChannelCall` child is written with:

- actor `run_as`;
- the desk inbox association;
- the destination binding and revision;
- the caller's `delivery_policy`, correlation and parent link;
- `knowledge = stored_levels(child ceiling, request level)` and `content_exact` = the request's
  exactness, both from step 7.

The child starts from the request it consumes, which is all of the caller's context it may see.
It does not start from everything the caller consumed, as an inherited child does.

## Phase ownership of the admission checks (F6)

| Check | Owner | Test |
| --- | --- | --- |
| Intake (step 6) | p02 | V12 `channel_call_snapshot_intake_refused_before_write`: undeclared and invalid `fields` give the error with schema and paths; zero child, record or manifest rows. |
| Levels (step 7) | p02 | V13 `channel_call_snapshot_audience_refused_above_ceiling`: a caller at content level 1 gives `audience_refused` and zero rows. A caller at level 0 commits, with the child's ceiling 0, content 0 and exactness copied. |
| Destination admission (step 8) | p02 | V6 (r2), extended with `verify_selection` failure. |
| Role resolution | p02 | V14 `channel_call_snapshot_role_refusal_is_denied`: a destination workflow whose required role has no holder gives a `destination_unavailable` closure. After the savepoint rollback, zero child, manifest or record rows exist, and the fence still holds. |
| Unit spend cap | p02 | V15 `channel_call_snapshot_unit_cap_is_denied`: an exhausted destination unit gives `budget.unit_exhausted`; zero rows; fence held. A cap on the caller's unit alone does not refuse. |
| Tree limits | p03 | `channel_call_launch_` (AWC-C003). |

V0's positive control asserts the child's knowledge columns as well.

## AWC-C003 r2 (replaces r1 on this gate's pass)

Same as r1, with two changes:
- p02 also takes the intake, level, destination-admission, role and unit-cap checks, under the
  savepoint protocol above;
- p03 additionally owns the tree-limit mapping to `workflow.tool_limit`, and the destination
  audience case in `knowledge::destination_level_on` (D1.4), keyed on the invocation record.

## Invariants added

7. Every admission refusal of D1.1 steps 3–8 happens before a child row exists, or is undone by
   the savepoint. A refused call leaves only its tool result.
8. A call child's stored ceiling and content level satisfy `content ≤ ceiling ≤ PUBLIC` today,
   by construction at step 7.

## Finding dispositions (r2 → r3)

| Finding | Disposition |
| --- | --- |
| F6 | Fixed. Steps 6–9 place intake, levels, destination admission, role resolution and the unit cap; the child's content level and exactness are stated (D4 item 1). The owner table assigns each check, with tests V12–V15. |
| F7 | Fixed. The hint is company-scoped. A missing `run_as` account is optional in the union and becomes `destination_unavailable` at step 5, never a caller refusal. |
