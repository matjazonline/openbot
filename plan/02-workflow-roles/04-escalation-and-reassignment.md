# 04 Escalation and reassignment

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Work that stalls on a department desk reaches a person or agent who can act on it without the
procedure's author having written an error route, and a human decision whose reviewer role changes
hands while it is parked moves to the new holder. Continues in
[05](05-integration-and-final-verification.md).

## Batch 04A The escalation ladder

1. Binding configuration (`src/application/workflow/binding/mod.rs:29`) gains
   `escalation: Option<EscalationPolicy { notify: DepartmentRole, hops: 1 | 2 }>`, valid only on a
   binding whose channel has a department. Activation refuses it otherwise with
   `binding.escalation_needs_department`.
2. Triggers, all already observable: admission refused for a role with no holder
   ([02](02-admission-resolution-and-snapshots.md)); a run failed or cancelled by the wait sweeper
   (`sweep_waits`, deadline expiry) or by a guard; a `channel.call` child closed `denied`
   ([agent-workflow-call](../agent-workflow-call/01-contracts-and-authority.md)); a `decision.human`
   deadline passed. An authored error route that handles the failure suppresses the ladder for that
   run; the ladder is the default, not an override.
3. Action: one notification to the holder of the named role in the desk's department, through the
   existing notification path (`notification_events`, `init_schema.sql:4059`), carrying the run,
   the step and the typed reason. If the role has no holder, or the first notice is itself past a
   bounded acknowledgement window (`escalation_ack_hours`, default 24, bounded 1 to 168), the
   second and last hop is the parent department's `lead`. Beyond that the company owner sees it in
   the "waiting on a person" view ([01-departments](../01-departments/04-ui-and-docs.md)).
4. Acknowledgement is a decision row the holder settles (acknowledge, reassign to a principal, or
   cancel the run), so it is audited like every other human decision and shows in the same UI.
5. Records: `workflow_run_escalations (company_id, run_id, hop, role, department_id, principal_id,
   reason, created_at, settled_at)`, append-only, one row per hop. The ladder never re-sends for the
   same run and hop.

## Batch 04B Parked decisions follow the role

1. `decision.human` gains `reviewers_follow_role: bool` (default false), compiled into the version.
   With it set, when a `reviewer` or `lead` role used by a parked decision changes holders, the
   decision's reviewer rows are replaced in the same transaction as the role write
   (`assign_role_on`, [01-departments](../01-departments/01-departments-and-membership.md)), the
   run's role binding rows are appended with the new resolution (never rewritten), and a fresh
   notice goes to the new holder. The proposal hash, artifact and revision are untouched.
2. A decision already settled, or one whose reviewers were pinned principals, is unaffected. A
   reassignment that leaves no reviewer parks the decision as before and triggers the ladder in
   04A.
3. Live reviewer removal (membership loss, principal retirement) uses the same replacement path,
   which is the general fix `plan/todo.md` calls "live reviewer reassignment".

## Focused checks

- `workflow_escalation_`: each trigger produces exactly one hop-1 row and notice; an authored error
  route produces none; no holder skips to hop 2; the acknowledgement window elapsing produces hop 2
  and nothing after; a replayed sweep sends nothing twice; acknowledgement, reassignment and cancel
  settle the row.
- `workflow_escalation_competing_`: two sweepers race on the same expired run and leave one hop row.
- `workflow_reviewer_follow_`: a parked decision moves to the new holder in one commit; a settled
  one does not; the hash is unchanged; a step without the flag keeps its reviewers; removing the
  last reviewer triggers the ladder.
