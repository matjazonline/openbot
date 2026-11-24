# AWC-P01 reviewer findings

Common context for every entry below unless stated: date 2026-11-22; workflow
`implement-opus-reviewed-2_4`; task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md); batch
AWC-P01-B1, stage architecture gate; review round
[AWC-P01-D1-R1-N1](agent-review-notes.md#awc-p01-d1-r1-n1); reviewer B, Fable 5.2 (effort as
assigned: medium); phase AWC-P01; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`,
branch `plan08_ph1`, HEAD `fe7791d3`, digest
`fdc78e18fd277e82c774791bce2b7fd368314efad800f2193465ea8471874a97`; reviewed decision
[AWC-P01-D1 r1](logs/AWC-P01-D1-r1-decision.md). All findings originate in the gated decision,
not in an implementation (none exists).

## AWC-P01-D1-F1 — A refusal with a reason has no agreed shape: `unit.archived` and admission refusals

- **Classification:** actionable defect; medium; requirements / cross-plan contract.
- **Evidence.**
  - Plan08 `02-unit-lifecycle.md` 02B.3: "`channel.call` to a desk of an archived unit closes
    `denied` with `unit.archived`." Plan08 `03-goal-units-and-tracking.md` 03C.3: "A refused
    child returns to its caller as a `denied` closure with that reason" (`budget.goal_exhausted`).
    Plan 11 `03-requests-between-units.md` 03B.3: "the model receives its typed `denied` closure".
  - D2 makes `unit.archived` a tool **error** code, and `denied` exists only inside
    `{"status":"replied","child_run_id":…,"content":…}`. A refusal before launch has no child
    (I4), so the three plans' `denied` closure cannot be produced in the D2 envelope. The
    deviation is not stated in D1/D2, in AWC-C001, or in 08G-C001 (which says "returns
    `unit.archived`").
  - Message admission also refuses for reasons D1.2/D2 do not list: the unit spend cap
    (`unit_budgets::check_admission_on`, called from
    `src/adapters/persistence/workflow/admission_write.rs:69`, code `budget.unit_exhausted`), a
    role that cannot be resolved (`AppError::WorkflowRole`, `workflow/admission.rs:88`) and
    resource readiness (`admission.rs:78`). D1.2 says spend is charged to the destination chain
    but gives no outcome when that chain is exhausted; D2 has no code for any of these, and
    `target_unavailable` would hide an actionable reason from a caller who already passed D1.1.3.
- **Impact.** Plan08 p03 (goal rollup counts closures; budget refusal returns `denied`) and plan
  11 03B build on whichever shape is chosen; it is model-visible and costly to change later.
- **Required.** Decide one representation for "admitted caller, destination refuses with a
  reason" (either a `denied` closure that may omit `child_run_id`/`content`, or an error with a
  reason code), list the destination-admission refusals it covers, and record the amendment to
  Plan08 02B.3/03C.3 and plan 11 03B.3 in the coordination notices.
- **Instruction lesson.** Observation: the decision checked the plan being implemented but not
  the wording of the plans that consume the contract. Candidate (scope: architecture gates that
  change a cross-plan contract): quote each consuming plan's sentence beside the decided shape.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D2 makes the closure the result kind; `denied` before launch has no `child_run_id`; D1.1 steps 5 and 8 list `unit.archived`, `desk_disabled`, `no_responder`, `destination_unavailable`, `budget.unit_exhausted`; README "Closure says why" amended. Matches Plan08 02B.3/03C.3 and plan 11 03B.3 wording. Residual consequence recorded as F10.

## AWC-P01-D1-F2 — D1.4 leaves the child's destination audience undefined; the ceiling rule does not take effect as written

- **Classification:** actionable defect; medium; correctness / knowledge levels (security-adjacent).
- **Evidence.**
  - A run's effective read ceiling is `min(admitted ceiling, current destination)`
    (`src/adapters/persistence/workflow/memory_steps.rs:210-232`, `read_ceiling_for`), and a
    destination below the run's content is an error (`KnowledgeRelease`). For a run associated
    with a thread, the destination is `thread_audience_on`
    (`workflow/knowledge.rs:214-236`, `:50-71`): the lowest **human** party; agents and system
    principals contribute nothing (`knowledge.rs:30-35`) and an empty set is `PUBLIC`
    (`domain/entities/access_level.rs:53-55`).
  - The call child runs on a fresh destination thread whose only party is the calling agent
    (README "isolated conversation branch", 03A.2). So, as decided, the child reads at level 0
    whatever `min(caller ceiling, run_as level)` is, and every memory read fails when the request
    was stamped above level 0. D1 does not say who the thread parties/author are or what the
    child's destination level is.
  - D1.4's request check "passes the existing release check for a message written to that
    desk's audience" names a check that does not exist: a `channel` destination has clearance
    `internal`, which equals the content and always passes
    (`workflow/knowledge_message.sql` receivers CTE; `knowledge_release.rs:103-108`). Today the
    bound is applied at message admission instead (`knowledge.rs:142-154`,
    `content > ceiling` → `KnowledgeTrigger`).
  - I1 is contradicted by D1.4: a relayed message from the same agent gives the responder a
    level-0 ceiling today (agent-only thread), while the call gives up to
    `min(caller, run_as)`. Plan 11 03C.1 intends the latter, so the rule is plan-consistent; the
    invariant's wording is not.
- **Impact.** Implemented literally, a lead's call to a member desk yields a child that cannot
  read non-public memory, or fails mid-run instead of refusing with `audience_refused` before
  rows exist. Fixing it in p03 means inventing an unreviewed level rule.
- **Required.** State (a) the fresh thread's author/parties, (b) the child's destination level
  for reads and for its response (for example the child ceiling, since the receiver is the
  calling run), (c) the actual request check in terms of code that exists, and (d) I1 restated
  with the knowledge exception. Add the level-0-thread case to the D1.4 verification row.
- **Instruction lesson.** Candidate (scope: gates touching knowledge levels): trace the level
  through `read_ceiling_for` and `destination_level_on`, not only the admitted ceiling.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D1.4 defines the child's destination level (its ceiling, lowered by a human who joins), removes the nonexistent desk release check, and I1 is restated. r3 sets the allowance to `PUBLIC` per U4, so the ceiling is level 0 today; the destination case is to be unit-tested with a raised allowance. Implementation (the new `destination_level_on` case) is unverified until p03.

## AWC-P01-D1-F3 — The model has no source for `channel_id` or the intake; the named directory holds neither

- **Classification:** actionable defect; medium; contract completeness.
- **Evidence.** D3: "The agent directory entry (`list_company_agents`) carries each desk's
  intake schema beside its address." The entry is
  `AgentDirectoryEntry { id, key, name, active_revision, departments: Vec<DepartmentRef>,
  address: Option<AgentAddress> }` (`src/application/agent/authoring_reads.rs:139-164`);
  `AgentAddress` is the agent's **direct principal inbox** (`principal_id`, `address`) and
  `DepartmentRef` has no desk. The tool description says so: "An agent's address names its
  direct principal inbox; shared desks are created separately"
  (`src/application/workflow/provisioning.rs:61`). D1.1.2 admits only a desk (a channel), and
  D2 takes a channel UUID. Phase 05 A.3/A.4 offers only a copyable ID in a human view and says
  "no schema discovery round trip", which 01 Batch C item 4 contradicts.
- **Impact.** The lead fixture (01-departments 03B.2, Plan08 03A.5) cannot address a member or
  department desk from the directory, and the intake cannot be attached to an entry that has no
  desk. A directory that did list desks must not show a desk the calling agent may not write to
  (I2), which the decision does not address.
- **Required.** Decide where the model reads `{channel_id, address, intake}` (for example a desk
  on `DepartmentRef` and on an agent's shared desk), filtered by the D1.1.3 write rule, and
  reconcile 05 A.4.
- **Instruction lesson.** Candidate: a gate that cites an existing type as carrying a new field
  quotes that type's current fields.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, decision fixed in substance (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): the directory is the discovery, filtered by check 3, and 05 A.4 is amended. The description of today's listing is still wrong; see F9.

## AWC-P01-D1-F4 — The gate decided is not the gate the user was told about

- **Classification:** actionable defect; medium; requirements / authorization traceability.
- **Evidence.** The reviewer assignment quotes what the user was told when answering U1:
  "Without request rules, the only gate would be today's operations authority plus desk
  visibility". The decision's U1 section paraphrases it as "the only gate is the existing rule
  for an agent writing to a desk", and D1.1.3 states "The parent run's actor is **not**
  consulted". These are different predicates: the first tests the parent actor
  (`viewer_access`, `domain/entities/channel.rs:150-155`, with the owner exempt), the second the
  agent principal (`participant_access` with `IngressOrigin::internal_context`,
  `thread/ingest/policy.rs:155-171`: membership `Member`, never owner). On a public desk the
  write rule admits every company agent holding the capability. The README and plan 11 edits
  that carry the write rule are marked "(user, 2026-11-22)" but were written by root.
- **Impact.** An authorization model is being fixed on consent given for a different gate. The
  write rule is defensible (it matches plan 11 03B.2/03B.3 and drops the owner exemption the
  user ruled out), but that is the reviewer's judgement, not the user's decision.
- **Required.** Record the verbatim question and answer in the decision, state the difference,
  and either obtain the user's confirmation of the write rule or keep the disclosed gate until
  plan 11. Also state that the parent run's own live actor authority on the existing tool-call
  path (`action_authority::prelock_on`, `workflow_tool_calls.rs:64-66`) stays in force; its
  exact checks were not read in this round.
- **Instruction lesson.** Candidate (scope: gates resting on a user decision): store the
  question and answer verbatim; a paraphrase is not a review input.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): U1 and U2 question and answer are quoted; U2 selects the agent write right; D1.1.2 states the actor liveness that remains. B cannot verify the quotations against the conversation; they are root's record.

## AWC-P01-D1-F5 — D1.2 does not say which admission facts come from the caller's tree

- **Classification:** actionable defect; low; architecture completeness (one security item).
- **Evidence.** D1.2 replaces `children::configured_child`/`insert_child_on` semantics with
  "as for a message arriving on that desk" and lists lineage as cancellation, deadline, depth,
  count and correlation. Not placed on either side:
  - `delivery_policy`: today copied from the parent so a simulation's child cannot send
    (`workflow/children.rs:359-361`). A message-admitted run gets the normal policy; a simulated
    caller would then cause real effects under the destination's identity. 03A.5 asks to
    preserve it; D1.2 should fix it as a lineage fact.
  - root budget and token-review policy (root-owned, `admission_write.rs:116-143`,
    `admission.rs:256-266`); role snapshot and escalation ladder (`capture_root_on`, root runs
    only) for a destination workflow with `role:` steps; the goal (Plan08 03B.2: the parent's
    goal wins).
- **Required.** One line per fact naming its source (caller tree or destination desk).
- **Instruction lesson.** Cause unknown beyond omission.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D1.2 fact table; `delivery_policy` inherited; I5 and a verification case added.

## AWC-P01-D1-F6 — Closure producers and refusal precedence are under-specified

- **Classification:** actionable defect; low; contract precision.
- **Evidence.**
  - `handed_off` "when the final response came from a nested call" has no decidable rule: the
    final output is the agent's projected output and nothing marks it as coming from a call.
  - `channel.deny` on a run admitted by mail (not a call) has no stated behavior, and the
    `replied` envelope does not say what `content` holds when the closure is `denied`.
  - D1.1 failure text gives `unit.archived` "only when 3 would otherwise pass" but not its
    precedence when 2 also fails (archived and no responder), nor where `audience_refused` and
    `cycle` sit relative to 2/3. If either is evaluated before 3 it tells a caller without the
    write right something about the desk (I2); `audience_refused` reveals the `run_as` level.
  - README: deadline expiry "is the existing typed deadline error"; D2 mints
    `channel_call.deadline_exceeded` without noting the change.
- **Required.** A decidable rule or an explicit deferral to a p04 gate for `handed_off` and
  mail-side `channel.deny`; one ordered list of refusal checks; note the deadline code.
- **Instruction lesson.** None supported.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D1.1 is one ordered list with each check gated on the earlier ones; `channel.deny` and `channel.hand_off` are decidable producers with mail behavior; the deadline code is deferred to 03 with a stated rule.

## AWC-P01-D1-F7 — Archive cannot cancel a call child (optional)

- **Classification:** optional suggestion; low; cross-plan interaction.
- **Evidence.** Archive counts open runs by desk (`department/archive.rs:67-71`), so a call
  child on the destination desk blocks archiving, but "cancel and archive" cancels roots only
  (`archive.rs:155-159`, `parent_run_id IS NULL`). A unit with an in-flight call child is
  refused as not cancellable until the caller finishes. D1.5's "unit archived while waiting"
  case is therefore reachable only after the child finished and before the result is consumed.
- **Suggestion.** Record for p03/p06 so the `revoked` test uses a reachable state and Plan08
  can decide whether archive should cancel call children.
- **Resolution history.** 2026-11-22 open (optional).
  - 2026-11-22, addressed (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D1.5 records the archive interaction and 03 extends cancel-and-archive. See F10 for the Plan08 coordination consequence.

## AWC-P01-D1-F8 — `MessageTriggerInput::schema()` is also the `message.reply` wait payload (optional)

- **Classification:** optional suggestion; low; contract impact.
- **Evidence.** `src/application/workflow/triggers.rs:63` ("a `message.reply` wait must declare
  exactly this payload") with `additionalProperties: false`. Adding optional `fields` changes
  that reserved payload schema and every published wait that declares it.
- **Suggestion.** List the wait payload among D3's consumers in TASK "Contract impact".
- **Resolution history.** 2026-11-22 open (optional).
  - 2026-11-22, addressed (recheck AWC-P01-D1-R3-N1, decision r3 sha256 `816bc514…c3e0`, digest `6b2e98c3…eed4`): D3 lists the reserved payload consumers.

## AWC-P01-D1-F9 — D3 misdescribes what `list_company_agents` returns today

- **Context:** round [AWC-P01-D1-R3-N1](agent-review-notes.md#awc-p01-d1-r3-n1); decision
  [AWC-P01-D1 r3](logs/AWC-P01-D1-r3-decision.md); digest
  `6b2e98c3160789f86a9b7df44efc8148d789dc935c4f9982e983dc103a4eeed4`. Follow-up of F3.
- **Classification:** actionable defect; low; contract precision. Does not block the gate: the
  decided behavior (the directory lists callable desks with their channel id and intake, only
  for desks passing check 3) is determinate.
- **Evidence.** D3 Discovery: "`list_company_agents` today gives an agent's own desk
  (`desk: {id, address}`)". The listing emits
  `"address": {"principal_id": …, "address": …}`
  (`src/adapters/persistence/workflow/provisioning_directory.rs:101`): a direct principal inbox,
  no `desk` key and no channel id. A principal inbox is not a channel, so it cannot pass D1.1
  check 3 and cannot be `callable`. Departments carry `slug`, `name`, `roles` only.
- **Required (in p05 Batch A, or a text correction).** The directory must add channel desks
  (`channel_id`, address, `callable`, `intake`) for department desks and agents' shared desks;
  `callable` must not be attached to the principal `address`. Carry this in TASK "Contract
  impact" for p05.
- **Instruction lesson.** Same candidate as F3: quote the current fields of a type a decision
  says it extends.
- **Resolution history.** 2026-11-22 open; deferred to p05 Batch A by B as not
  acceptance-blocking for this gate.

## AWC-P01-D1-F10 — Consequences other records should carry (optional)

- **Context:** as F9.
- **Classification:** optional suggestion; low; coordination.
- **Evidence and suggestions.**
  - U4 makes every call level 0 until plan 11: a caller that consumed anything above level 0 is
    refused. Plan08 p03's lead dispatch and its tests depend on the call; notice 08G-C001 r1
    does not say so. Suggest adding it when Plan08 coordination is next revised.
  - A `denied` closure before launch leaves no rows (I4), so Plan08 03D's rollup of "calls made
    and their closures" cannot count those. p03 should know.
  - D1.5 has agent-workflow-call 03 change Plan08 p02's accepted cancel-and-archive
    (`department/archive.rs:155-159`). That needs a Plan08 notice when it is done.
  - AWC-C001 r3 says D1 replaces the actor and operations-authority text, but 01 A.3
    (`replied`/`no_reply` statuses) and 01 C.5 (`reply` step) are also replaced by D2/D3 and are
    not amended or named in the notice.
  - `channel.hand_off` naming a nested call that closed `denied` or `no_reply` has no stated
    outcome; p04 should refuse it at runtime.
- **Resolution history.** 2026-11-22 open (optional).

## AWC-P01-B1-F1 — The built-in is added to the enum but not to the init schema's vocabularies

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage batch review; round
  [AWC-P01-B1-R1-N1](agent-review-notes.md#awc-p01-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P01; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`, digest `66d3e78f2c4f551fba5c44894cd8cc2d3c9e9d09d937e861557d98a38413e17e`.
- **Classification:** actionable defect; medium; requirements / contract impact. Originates in
  the implementation, not the gate.
- **Evidence.**
  - 01 A.1: "Add `ChannelCall` / `channel.call` to the existing built-in capability owner …
    Reserve the name across tool registries." The owner includes two CHECK constraints on
    `agent_capability_bindings` (`migrations/20260817000000_init_schema.sql:12510-12523`):
    `agent_binding_target_shape` lists the storable `builtin` values and
    `agent_binding_reserved_alias` lists the reserved aliases. Neither names the new tool, and
    the diff touches no migration. The scratch test database confirms it
    (`pg_get_constraintdef(agent_binding_target_shape) LIKE '%channel_call%'` is false).
  - `AgentBuiltinTool::parse("channel.call")` now succeeds, so the name-resolution path
    (`agent_binding_selection.rs:43`, reached from `workflow/provisioning.rs:631`) builds a
    built-in binding whose insert (`agent_capability.rs:76-90`, `builtin = 'channel_call'`)
    violates the CHECK and returns `BadRequest("Select a valid tool target …")`
    (`relationship_error`, `:33-43`). By inspection, not executed.
  - TASK "Contract impact" lists the Rust consumers but no SQL surface, and attributes the
    tool being ungrantable to the authoring catalogue. The actual barrier is the CHECK, and it
    also blocks the fixtures p02/p03 need (a published agent holding the capability) long
    before p05. This placement is not among the ones the author disclosed.
- **Impact.** The capability cannot be stored by any path; `agent_publication/tools.rs::builtin`
  and the dispatch arm are unreachable; the DB does not reserve the alias.
- **Required.** Either extend both constraints in the init schema now (reset the scratch
  databases, rerun the SQLx check and the affected agent-capability tests), or record the
  deferral with its owning phase (p02 Batch A already edits the init schema) and correct the
  TASK inventory and its "no author can grant it" explanation.
- **Instruction lesson.** `verification-contracts.md` already says "Include SQL readers";
  candidate (scope: openbot, adding an enum value that is persisted): grep the init schema for
  a sibling value before declaring the inventory complete.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed (recheck AWC-P01-B1-R2-N1, digest `2ebbc0cd…feaa`): both constraints in the init schema name `channel_call` / `channel.call`, with the CASE alias mapping; confirmed in the recreated scratch test database. New DB test `channel_call_contract_built_in_binding_is_stored_and_published_exactly` stores and publishes the binding and asserts the stored `builtin`, alias and exact trusted schemas; run by B, passed. The tool is now storable by name while absent from the catalogue; a granted call returns `target_unavailable` until p03, as disclosed.

## AWC-P01-B1-F2 — The implementation departs from gate r3 without the decision of record saying so

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage batch review; round
  [AWC-P01-B1-R1-N1](agent-review-notes.md#awc-p01-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P01; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`, digest `66d3e78f2c4f551fba5c44894cd8cc2d3c9e9d09d937e861557d98a38413e17e`.
- **Classification:** actionable defect; low; records. No code change needed.
- **Evidence.** Gate r3 D3 says "`schema()` declares it optional" and "One new compiler rule …
  a publication error"; its verification row puts `channel.deny` routing/validation in "p01
  (compile)". The batch keeps `MessageTriggerInput::schema()` unchanged, bounds the fields in
  `ConfiguredBinding::check_message_trigger` (`binding/mod.rs:111`, `:126-164`) and adds neither
  step. B reviewed the three placements and accepts them:
  - `schema()`: it is the literal input schema of many responders and the exact
    `message.reply` payload, so an optional generic `fields` there declares an unbounded intake
    on every such desk (`logs/AWC-P01-B1-affected-attempt1-failed.txt`). Leaving it out also
    removes F8's impact.
  - activation bound: only a message-triggered binding gives `fields` this meaning, and all
    four callers of `check_message_trigger` (`lifecycle/bindings.rs:56`, `:101`,
    `lifecycle/provisioned.rs:65`, `:119`) pass through it.
  - steps to p04: a terminal step finalization cannot handle would break mail runs.

  But the decision file and AWC-C001 r4 still state the old text, TASK "Batch plan" item 3 still
  lists `channel.deny` and result-field projection, and TASK "Status" ends with "Implementation:
  not started. Review: none." Nothing outside TASK tells p04 it now owns registering both steps
  and validating result fields.
- **Required.** A dated addendum to the gate (or r4) for the D3 changes, the p04 ownership in
  AWC-C001, and the stale TASK lines corrected.
- **Instruction lesson.** The workflow says to reopen the gate when a gated decision changes;
  here the change was made and disclosed only in TASK. Candidate: small gate amendments get an
  addendum in the decision file before the batch review.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed except two stale TASK lines (recheck AWC-P01-B1-R2-N1, digest `2ebbc0cd…feaa`): `logs/AWC-P01-D1-r3-addendum-A1.md` records the three placements and AWC-C002 r1 (coordination r5) gives p03/p04/p05 their ownership. TASK "Batch plan" item 3 still lists the `channel.deny` step, a compiler bound and result-field projection, and "Status" still names the R1 digest. Records only, outside the digest; root to correct before acceptance.

## AWC-P01-B1-F3 — A body of only invisible characters is accepted as a request

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage batch review; round
  [AWC-P01-B1-R1-N1](agent-review-notes.md#awc-p01-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P01; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`, digest `66d3e78f2c4f551fba5c44894cd8cc2d3c9e9d09d937e861557d98a38413e17e`.
- **Classification:** actionable defect; low; correctness.
- **Evidence.** `parse_content` (`channel_call.rs:167-192`) tests `body.is_empty()` before
  `CanonicalContent::parse`, which strips invisible Unicode (`ingress.rs:109-119`,
  `unicode_sanitization.rs:151`). A body made only of such characters passes and yields an
  empty canonical body. 01 A focused checks ask for "canonical bounds including Unicode"; no
  test covers this case. By inspection, not executed.
- **Required.** Test emptiness on the parsed content and add the case to
  `channel_call_contract_refuses_…`.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed (recheck AWC-P01-B1-R2-N1, digest `2ebbc0cd…feaa`): emptiness is judged on the canonical text after parsing (`trim()`), and the test adds whitespace-only and zero-width-only bodies; run by B, passed.

## AWC-P01-B1-F4 — Notes for later phases and style (optional)

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage batch review; round
  [AWC-P01-B1-R1-N1](agent-review-notes.md#awc-p01-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P01; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`, digest `66d3e78f2c4f551fba5c44894cd8cc2d3c9e9d09d937e861557d98a38413e17e`.
- **Classification:** optional suggestion; low.
- **Evidence and suggestions.**
  - `ChannelCallRequest::parse` takes `now`. A replayed saved call whose deadline has since
    passed would parse as `deadline_invalid`; p03 must resolve the replay before the shape
    check or use the stored dispatch time.
  - `check_call_fields` iterates a 3-tuple with two `&str` (`binding/mod.rs:140-151`), against
    `src/AGENTS.md` "Name your tuples".
  - `Schema::failing_paths` with `limit == 0` never stops early (`compiler/schema.rs:94-105`).
  - `declared_fields` reads `properties.fields` only; a schema declaring it through `allOf`
    or `$ref` is treated as declaring none. Consistent as long as p02 and p04 use the same
    function.
- **Resolution history.** 2026-11-22 open (optional).
  - 2026-11-22, partly taken (recheck AWC-P01-B1-R2-N1, digest `2ebbc0cd…feaa`): named struct `Declared`, `failing_paths` returns at once for a zero limit; the replay and `allOf`/`$ref` notes are in the addendum and AWC-C002. Closed.
