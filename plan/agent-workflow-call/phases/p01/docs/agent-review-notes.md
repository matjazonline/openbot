# AWC-P01 reviewer notes

## AWC-P01-D1-R1-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage architecture gate; reviewer B, Fable 5.2
(effort as assigned: medium); phase AWC-P01; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`; phase start, review base
and HEAD `fe7791d3405f00335cbc05dd5029730cc9273416`; digest
`fdc78e18fd277e82c774791bce2b7fd368314efad800f2193465ea8471874a97` (exclusion
`plan/agent-workflow-call/phases/p01/docs`), confirmed before review. **Recheck before
returning did not match:** digest `c97f1fc724e6efb458d4cc178d439006ec1408b41b9fe7928ebc1bd0b4592450`,
HEAD unchanged. The only difference in the snapshot file list is a new untracked file,
`.trail/learnings/2026-11-22-a-desk-call-runs-the-called-mac02.md`, which appeared during the
review and was not written by B. The plan and coordination files reviewed are unchanged
(modification times 15:11, before the review began), and the decision file is outside the
snapshot. The findings stand against decision r1; root must record the new digest or the
cause before relying on a frozen identity. Gated decision: AWC-P01-D1 r1,
[logs/AWC-P01-D1-r1-decision.md](logs/AWC-P01-D1-r1-decision.md). Coordination:
`plan/agent-workflow-call/COORDINATION.md` r1, notice AWC-C001 r1;
`plan/08-goals/COORDINATION.md` r4, notice 08G-C001 r1. Both read at review time; no newer
revision present.

**Coverage and evidence.** Requirements read from the originals: agent-workflow-call README
(with the `fe7791d3` diff of the authority bullet), 01 Batches A–C, 02–04, 05 Batch A; Plan08
02B and 03A–03D; plan 11 README proposal 1 and 03B/03C; root `AGENTS.md`. All evidence is
inspection of code at `fe7791d3`; nothing was executed and no database was used (none needed
at this gate). Code read:

- write rule: `thread/ingest/policy.rs:121-171`, `routing.rs:170-230`, `:376-420`, `:805-835`,
  `domain/entities/channel.rs:86-155`, `participant.rs:317-340`, `thread/inbox.rs:41-46`.
  D1.1.3's description of the predicate is accurate (agent sender is `Member`, never owner;
  public desk admits any sender; members desk needs a `Participate` grant).
- tool-call path: `workflow_tool_calls.rs` whole file; `children.rs:159-378`;
  `run_scopes.rs:257-268`. Anchors `workflow_tool_on`, `check_execution_on`,
  `AgentUseMode::Tools`, `configured_child`, `copy_child_on`, `tool_error`,
  `MAX_TOOL_RESULT_BYTES` exist as cited.
- responder selection and message admission: `channel_reply.rs:47-62`, `:292-345`;
  `workflow/admission.rs:11-105`, `:328-350`.
- levels: `knowledge.rs:1-260`, `memory_steps.rs:196-232`, `knowledge_release.rs:1-215`,
  `knowledge_message.sql`, `access_level.rs:53-55`, `:172-185`.
- contract/fields: `triggers.rs` whole file, `channel_reply/contract.rs:1-120`, registry step
  list (`registry/mod.rs:71-76`, `:332-345`; there is no `reply` step, as D3 says),
  `provisioning.rs:20-110`, `agent/authoring_reads.rs:139-170`, `tools.rs:15-101`.
- archive: `department/archive.rs:62-75`, `:152-184`.

Assumptions challenged: the write-rule gate (F4), the ceiling rule (F2), archived versus
unavailable ordering (F1, F6), D3's no-`reply`-step reading (accepted: final output `/fields`
validated by the output schema is implementable and keeps "prose answer is valid" when the
author leaves `fields` optional), and what later phases need (F1, F3, F5, F7).

Verification mapping: the decision's "Verification plan" and TASK "Acceptance mapping" were
checked against the criteria. They are plans only (all checks TBD). Gaps tied to findings: no
row for a destination-admission refusal (F1), for the level-0 thread destination (F2), for
directory visibility (F3) or for a simulated caller (F5).

Reviewed only lightly or not at all: `action_authority::prelock_on` (body not read; F4 notes
it), `agent_origin::workflow_tool_on` body, `bindings::verify_selection`,
`management::authorize_association_on`, the manifest/dependency-scope code behind D1.2's
fourth bullet (phase 02's subject), the response/review pipeline behind D2's closure (phase
04), and `routing.rs` beyond the spans listed. D1.5 was read for coherence only.

**Outcome.** Changes required; no pass for AWC-P01-D1 r1. Findings in
[agent-review-findings.md](agent-review-findings.md): actionable F1, F2, F3, F4 (medium), F5,
F6 (low); optional F7, F8. Unresolved question for root: whether the user confirms the agent
write rule in place of the gate described to them (F4).

**Process and instruction feedback.** The gate was warranted: four of six actionable findings
are decisions later phases would have had to invent. Its size was reviewable. Useful in the
records: code anchors on nearly every bullet, which made checking fast. Missing: the verbatim
user question and answer, and the consuming plans' sentences beside the decided contract.
Candidate instructions are in F1, F2, F3 and F4; none is adopted here.

## AWC-P01-D1-R2-N1

**Identity.** 2026-11-22; same workflow, reviewer, phase and worktree as R1-N1; stage
correction recheck of the architecture gate; decision AWC-P01-D1 r2
(`logs/AWC-P01-D1-r2-decision.md`, sha256 `fd0bc48b…ff33`); HEAD `fe7791d3`; digest
`02789d3daea60d4199edd4c565fbbebbe26785fb7a66b5381c60feac36e29ac8` with exclusions
`plan/agent-workflow-call/phases/p01/docs` and `.trail/learnings`, confirmed at start;
coordination agent-workflow-call r2 (AWC-C001 r2), 08-goals r4.

**Coverage and outcome.** r2 was read in full. The round was stopped without an outcome: during
it the digest changed to `6b2e98c3…eed4`, the plan README and 05 A.4 cited "r3" and a level-0
allowance that contradicted r2's D1.4, and an r3 decision file appeared. Root then confirmed r3
(user decision U4). No pass or failure is recorded for r2. What was concluded about r2's
unchanged sections is carried into R3-N1.

**Process feedback.** A decision revised while under review cost one round's identity checks.
Candidate (scope: root, this workflow): when a user decision arrives mid-review, tell B to stop
before editing the frozen files.

## AWC-P01-D1-R3-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage correction recheck of the architecture
gate; reviewer B, Fable 5.2 (effort as assigned: medium); phase AWC-P01; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`; review base and HEAD
`fe7791d3405f00335cbc05dd5029730cc9273416`; digest
`6b2e98c3160789f86a9b7df44efc8148d789dc935c4f9982e983dc103a4eeed4` (exclusions
`plan/agent-workflow-call/phases/p01/docs`, `.trail/learnings`), computed by B before the
coordinator named it and confirmed again before returning. Decision AWC-P01-D1 r3,
[logs/AWC-P01-D1-r3-decision.md](logs/AWC-P01-D1-r3-decision.md), sha256
`816bc514e85d6d6316c7105459566457ec3fe136e7b194c12b77f4853a87c3e0`. Coordination:
agent-workflow-call r3, notice AWC-C001 r3; 08-goals r4, notice 08G-C001 r1.

**Coverage and evidence.** Inspection only; nothing executed. r2 read in full; r3 checked as
the `diff` against r2 (title, header, U4, D1.4 first bullet, D1.4 verification row). Plan text
checked with `git diff fe7791d3`: README "Run the destination…" and "Closure says why", 05
A.4, plan 11 README proposal 1; all cite r3 and level 0 and agree with the decision. Each of
F1–F8 rechecked against the decision text; dispositions are in the findings file. New in
r2/r3 and examined: the ordered checks D1.1 against I2 (every answer more specific than
`target_unavailable` comes after check 3), the D1.2 fact table against `children.rs:330-361`
and `admission.rs:11-105`, the destination audience in D1.4 against `memory_steps.rs:210-232`
and `knowledge.rs:214-236`, the closure-first envelope against Plan08 02B.3/03C.3 and plan 11
03B.3, `channel.deny`/`channel.hand_off`, and directory discovery against
`workflow/provisioning_directory.rs:70-106`. The placeholder test D1.4 cites exists
(`knowledge_e2e_tests.rs:58`); its body was not read.

Not verified: that the U1–U4 quotations match the user's words (root's record); the bodies of
`prelock_on`, `verify_selection`, `authorize_association_on`; feasibility details of role
resolution for a non-root run and of the launch-time unit budget check (phase 03); the
response pipeline behind the closure producers (phase 04). The gate pass accepts the
verification plan, not its execution; every row is still to be run.

**Outcome.** PASS for decision revision AWC-P01-D1 r3. F1, F2, F4, F5, F6 fixed in the
decision; F7, F8 addressed; F3 fixed in substance with a residual low defect F9, deferred to
p05 Batch A and not blocking this gate; F10 optional. See
[agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** The finding-to-change table at the end of the decision
made the recheck fast. The verbatim user decisions resolved F4 and F2 without argument. No new
instruction candidate beyond R2-N1's.

## AWC-P01-B1-R1-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P01-B1 (01 Batches A, B, C), stage batch review;
reviewer B, Fable 5.2 (effort as assigned: medium); phase AWC-P01; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`; phase start, review
base and HEAD `fe7791d3405f00335cbc05dd5029730cc9273416` (all work uncommitted); digest
`66d3e78f2c4f551fba5c44894cd8cc2d3c9e9d09d937e861557d98a38413e17e`, exclusions
`plan/agent-workflow-call/phases/p01/docs` and `.trail/learnings`, confirmed at start, after
B's test run and before returning. Gated decision AWC-P01-D1 r3. Coordination:
agent-workflow-call r4 (AWC-C001 r4), 08-goals r5 (08G-C001 r2), both read. Evidence manifest
`logs/AWC-P01-B1-R1.evidence.json`, validated by `check-evidence.py --expect-artifact`
(5 PASS, 2 FAIL, 1 NOT EXERCISED).

**Coverage and evidence.**

- Read in full: the `src/` diff against `fe7791d3` (25 changed files), the new
  `channel_call.rs` and `channel_call_tests.rs`, the test diffs in `binding/tests.rs` and
  `exactness_tests.rs`, TASK "Acceptance mapping" and "Contract impact".
- Executed by B: `cargo test --locked --offline --lib channel_call_` → 17 passed
  (`logs/AWC-P01-B1-R1-reviewer-focused.txt`); one read-only query of the scratch test
  database's constraint definition.
- Inspected, not rerun: the author's affected run (796 passed), attempt 1 failure, fmt, SQLx
  check, source-modules and clippy logs. The clippy failure names nine files, none in this
  diff.
- Criterion to assertion, followed by B: 01 A.1 snapshot exactness and reserved name; 01 A.2
  parse matrix (unknown and spoofed keys, byte bounds at char boundaries, deadline equal, later,
  now, past); 01 A.3 every closure and nine error codes against the trusted output schema; 01
  A.5 the exact child input; D1.1 order (all sixteen unwritable combinations give
  `target_unavailable`, then cycle, archived, disabled, no responder, and a positive control);
  D1.4 ceiling and release; 01 C.2 bound paths and the three activation codes; 01 C.3 intake
  check. Each assertion distinguishes the failure it names. These are pure-function proofs; no
  production caller of `decide_target`, `child_ceiling`, `release_request` or `check_intake`
  exists yet, and the manifest records the DB controls as NOT EXERCISED.
- Consumers checked against code: every `AgentToolTarget`, `ToolKind` and `AgentBuiltinTool`
  match site found by grep is handled; the four callers of `check_message_trigger`; both
  callers of `agent_action_target`; `is_reserved_tool_name` callers. The SQL vocabulary was
  missing from the author's inventory (F1).
- Placements challenged: activation bound, steps to p04 and unchanged `schema()` accepted
  (F2 asks for the record); catalogue absence and the stub dispatch accepted as unreachable
  intermediate state, subject to F1; DB controls deferred, accepted as NOT EXERCISED.
- Reviewed lightly: provider-side adaptation of the dotted tool name and of `oneOf`/`format`
  in the trusted schemas (phase 05 A.1); `preview.rs` and `sample/tools.rs` one-line arms.

**Outcome.** Changes required; no pass for digest `66d3e78f…e17e`. Actionable: AWC-P01-B1-F1
(medium), F2, F3 (low). Optional: F4. See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** Batch size was right. The author's list of placements
and the retained failed attempt were useful and accurate. Missing: SQL surfaces in the contract
inventory (F1) and a gate addendum for the D3 changes (F2). Candidates are in F1 and F2.

## AWC-P01-B1-R2-N1

**Identity.** 2026-11-22; workflow `implement-opus-reviewed-2_4`; task [TASK.md](TASK.md),
review [REVIEW.md](REVIEW.md); batch AWC-P01-B1, stage correction recheck; reviewer B, Fable
5.2 (effort as assigned: medium); phase AWC-P01; worktree
`/Users/mac02/orca/workspaces/openbot/plan08_ph1`; phase start, review base and HEAD
`fe7791d3405f00335cbc05dd5029730cc9273416` (all work uncommitted); digest
`2ebbc0cd1c41a6241a85eb15f6a1bd508e826d40fd54e32f2079fd569ea1feaa`, exclusions
`plan/agent-workflow-call/phases/p01/docs` and `.trail/learnings`, confirmed at start and
before returning. Gate AWC-P01-D1 r3 with addendum A1. Coordination: agent-workflow-call r5
(AWC-C001 r4, AWC-C002 r1), 08-goals r5 (08G-C001 r2). Manifest
`logs/AWC-P01-B1-R2.evidence.json` validated against the digest (5 PASS, 1 NOT EXERCISED).

**Coverage and evidence.** Recheck by finding ID over the correction delta only: the init
schema diff, the new publication test, `parse_content`, `check_call_fields`,
`Schema::failing_paths`, the added test bodies, the addendum, AWC-C002 and TASK. Executed by
B: `cargo test --locked --offline --lib channel_call_` against the fresh scratch databases, 18
passed including the DB test (`logs/AWC-P01-B1-R2-reviewer-focused.txt`); a read-only query
showing both constraints name the new value. Inspected, not rerun: the author's affected run
(797 passed), SQLx check, source-modules and fmt logs. R1 coverage of the unchanged code is
reused. Not verified: clippy, which root is rerunning; in R1 it failed only on nine
pre-existing lints outside the diff. The DB-backed authority controls remain NOT EXERCISED
and belong to p02/p03.

**Outcome.** PASS for digest `2ebbc0cd…feaa`. F1 and F3 fixed; F2 fixed except two stale
TASK lines (records only); F4 closed. See [agent-review-findings.md](agent-review-findings.md).

**Process and instruction feedback.** Corrections arrived in one round with a test for each
code fix. One claim in the hand-off (TASK "Batch plan" item 3 updated) did not match the
file; no instruction change supported by a single instance.
