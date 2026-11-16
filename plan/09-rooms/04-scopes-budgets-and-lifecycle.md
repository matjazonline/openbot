# 04 Scopes, budgets and goal lifecycle

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Room collaboration respects unit ownership, resource selection, content boundaries, shared
discussion limits and goal archive. Continues in [05](05-ui-transport-and-docs.md).

## Batch 04A Scope and audience

1. Pass the room's owning unit into 05's single `run_scopes` resolver at every turn admission.
   Capture the result in `workflow_run_scopes`, using the selected agent's principal and roles.
   The default memory write unit is the room's unit. Reuse the existing principal namespace and
   any authorized lead reads; no room namespace, per-member scope union or new memory service.
2. Apply 07's captured resource ownership and model checks to the room scopes. Adding an agent
   does not make that agent's departmental connections available here. Validate configuration
   and recheck at admission/use as the existing protocols require. Missing dependencies are a
   visible refusal, not a fallback to a more privileged model/credential or the agent's direct run.
3. Current authorized room readership constrains context, retrieval, files and publication through
   the existing access-level/consumption rules. Membership addition during a run cannot widen the
   audience of its frozen input or cause over-level output to appear. Revocation is live even
   though run scopes are frozen. Test both addition of a lower-level person and removal after
   preparation. UI readers and notifications enforce per-message visibility independently.
4. Agent-to-agent addressing is a new turn in the room context. An authorized `channel.call`
   remains a separate delegation in the destination unit with its normal authority and reviewed
   return. Only the allowed return may be quoted into the room; unrelated destination thread
   history and private tool transcripts are never copied.

Focused checks: `room_scopes_` runs one agent in two units and checks distinct writes/searches;
`room_resources_` refuses sibling resources and an ownership move; `room_audience_competing_`
races membership/level changes with prepared output and file/notification delivery. Use scripted
models to inspect exact context without provider-dependent behavior.

## Batch 04B Shared limits and cost attribution

1. Charge `model_usage_calls.unit_id` to the room's write unit and apply 07's ancestor caps.
   Preserve 08's implemented goal attribution/caps separately. Agent membership in another unit
   does not move billing there. Reuse the usage ledger; do not add a second monetary ledger.
2. All turns of a discussion share a finite token allowance in addition to 03's turn/deadline
   bounds. Extend the existing token reservation/settlement owner to support a discussion across
   its runs, retaining ordinary per-run/root ceilings. Proposed default: 32,000 total input/output
   tokens per discussion, with each call reserving its bounded input and maximum output before
   invocation and settling actual usage afterwards. Refuse if no reservation fits. Ambiguous
   provider failures retain the runtime's conservative charge; a retry receives no free allowance.
3. This allowance is an additional runtime bound, not a change to 07/08's admission-only spend
   caps. Say precisely which limit stopped work. A later human request gets a new allowance but
   remains subject to company capacity, rate limits and unit/goal budgets. Include native/local
   execution and nested model work: a backend that cannot enforce the allowance must refuse room
   execution, not bypass it. Do not claim a strict monetary cap from a token count alone.
4. Unit usage pages and goal rollups include room turns through their ordinary attributed runs.
   Discussion inspection shows used/reserved tokens, turns left and the stop reason. No metric
   contains prompt content or credentials.

Focused checks: `room_budget_` checks unit/goal attribution and all model execution paths;
`room_budget_competing_` races reservations for the last allowance and verifies reserve/settle,
crash recovery and retry accounting. Assert every call is refused before invoking a fake provider
when its reservation cannot fit. Keep existing stack and lease limits unchanged.

## Batch 04C Goals, archive and deletion

1. Use 08's `goal_of` and immutable thread/run goal fields for room threads and turns. A follow-on
   agent turn carries the discussion's existing goal lineage; the model and mention payload cannot
   select another goal. A new human request follows 08's existing-thread/archived-goal rule.
2. Extend the unit/goal archive open-work query to queued/admitting room turns and active room
   discussions, including their already admitted runs. Share the same serialization gate with
   enqueue/admission. Archive either refuses for open work or wins before new work can be queued.
   A deadline/cancellation leaves a terminal discussion, so a completed discussion cannot block
   archive forever. Cancel-and-archive awaits terminal runtime cancellation.
3. An archived owning unit refuses room messages, membership/settings edits and all new agent
   work, just as 08 requires of desks. Existing authorized history stays readable; restoring
   does not replay old turns or alter the thread's pause state. Principal retirement still revokes
   access and work. Delete retains the existing history/run foreign-key guards.

Focused checks: `room_goal_` covers attribution and rollups without leaking other desks' content;
`room_archive_competing_` races enqueue and admission with archive; `room_lifecycle_` covers
cancel-and-archive, restore, membership retirement and full company teardown. Run focused affected
05–08 regression families with the README's nonzero-count protocol.
