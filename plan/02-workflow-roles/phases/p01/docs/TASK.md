# 02WR-P01 — Role references and publication

Status: ACCEPTED IN PHASE (B1-A1), 2026-11-11. Plan-wide integration/final verification pending p05; role execution continues in p02.

## Identity, ownership and continuity

- Workflow: implement-astra-reviewed-2-2; skill `/Users/mac02/.codex/skills/implement-astra-reviewed-2-2`.
- Root/A: `/root`, Sol 6.1/xhigh confirmed by user in this session. Actual runtime telemetry and effective compaction threshold unavailable; host defaults and durable checkpoints used, target 75% when exposed.
- B: `/root/workflow_roles_reviewer`, requested Astra/high with fork_turns none; no inherited conversation. Architecture pass saved as 02WR-P01-D1-R1/N1; runtime telemetry not independently exposed.
- Worktree: `/Users/mac02/.codex/worktrees/2050/openbot`; branch `codex/departments-addressing-plans`.
- Starting commit/review base/current HEAD: `103004a8ac2fd643cf14bde7e09022ae79f72053`. Clean at startup; fast-forwarded from accepted departments checkpoint b27f1dbe to published main. No pre-existing edits.
- Prerequisite: 01-departments p05 accepted digest `b715976039bbe891c5771d5e7946621afe528baf95b4d57bfd6fd41f59b89e35`, integrated as b27f1dbe then main 103004a8. Refer to its immutable records; never modify them.
- Records: this directory. Root owns TASK/HANDOFF; B owns REVIEW/notes/findings.
- Coordination authority: `/Users/mac02/.codex/worktrees/2050/openbot/plan/02-workflow-roles/COORDINATION.md`, writer root, checked revision 1. I001–I003/r1 and 02WR-C001/r1 acknowledged; deferred channel.call trigger has no p01 implementation impact.
- Snapshot exclusions, fixed: `plan/02-workflow-roles/phases/p01/docs`, `plan/02-workflow-roles/COORDINATION.md`, `plan/03-principal-addresses/COORDINATION.md`. Both coordination files are workflow-only; all deliverable plan/source/docs remain included.
- Owned scope: DepartmentRole vocabulary/schema; workflow registry/compiler/publication and binding eligibility; channel ownership edit guard; role sample and language/help docs. p02 owns resolution/runtime snapshots; p03 human reviewer conversion/UI; p04 escalation/reassignment; p05 final integration. Plan03 queued until plan02 accepted.
- Resources: existing Cargo target reused serially; phase-specific disposable databases and logs to be recorded before checks. No prior phase databases/processes/caches owned here. No external services started.

## Original requirements and batch

Original requirements: `plan/02-workflow-roles/README.md` and `01-role-references-and-publication.md`, all steps and focused criteria. One cohesive implementation batch B1 covers 01A and 01B, followed by independent batch review. Later phases remain authorized but are not yet implemented.

## Architecture gate D1 / revision 1

1. Add Reviewer to the fixed vocabulary and SQL role check; existing lead-only unique index remains. Reviewer has multiple holders and may be an agent or person at assignment; consuming mode enforces kind later. No data migration/backfill. Existing role assignment API uses `is_single_holder`, so Reviewer inserts instead of replacing.
2. For the three model-step kinds, require exactly one of agent or literal role. Emit `registry.agent_or_role` for both/neither, `registry.role` for an unknown/nonliteral role, and existing unknown-input diagnostic on non-model steps. Freeze `RoleReference { step, role, mode }` in compiled representation and publication manifest, derived solely from source. Tools mode only for agent.run; other model steps ModelOnly. Agent capture skips valid role steps; mixed/pinned finite universes keep their exact existing validation/forged-reference refusal. Store/restore recompiles source and recomputes identity, no mutable holder in version.
3. Active binding eligibility requires a department-owned channel when the bundle (including pinned children) has role requirements. Validate atomically in persistence after company/association authorization and under existing channel association lock, for activation and configuring an active binding. Inactive configuration/deactivation stays possible. Error `binding.role_needs_department` names the first step and no principal details. Clearing a desk's department checks its active bindings using their frozen bundles under the same company→channel lock order. Refuse with the same code before changing settings. No alternate write path may detach an active role binding. DB constraints retain tenant relations; application frozen-bundle validation remains the source for semantic role requirements (the stored bundle is bytea, not a trusted JSON field).
4. This phase alone is an intermediate implementation state: do not deploy it before admission p02 and the plan integration gate. Existing pinned behavior remains executable. Admission role ambiguity policy for p02: agent slots require exactly one agent at nearest holding department, never arbitrary selection or fallback through wrong-kind/ambiguous holders. People slots resolve all people, bounded after resolution.

Failure modes: no holder, wrong kind and ambiguity belong to admission p02; invalid role/source fails compilation; missing department fails active binding/ownership write atomically; deactivation always available. Department ownership reassignment to another non-null department remains allowed (future admissions resolve there).

## Acceptance and verification mapping

| Criterion | Production path/case | Decisive assertion | Planned evidence | Status |
| --- | --- | --- | --- | --- |
| AC1 source and identity | registry model selectors, compiler | both/neither/invalid/non-model rejected; lead→reviewer changes identity; all three kinds accept role | workflow_role_source_ tests | PASS — execution details below |
| AC2 publication | source uses/capture/freeze/store/restore | role version captures no agent, mixed captures only pinned, role metadata round-trips; forged universe refused | workflow_role_publication_ and existing forged-reference tests | PASS — execution details below |
| AC3 binding/ownership | save_binding_on + channel settings under locks | department desk activates; company/bare desk refuses naming step; active-role detach refused; deactivate then detach succeeds; competing activation/detach cannot both commit invalid state | workflow_role_binding_ DB tests | PASS — execution details below |
| AC4 fixed role/schema | domain parse, role assignment, migrated database | vocabulary round-trip; two reviewers coexist; lead still singleton; invalid SQL role refused | domain + department DB tests | PASS — execution details below |
| AC5 authoring docs/sample | registry help and fixture source | help lists roles/rule; shipped role sample compiles and freezes with no agent | sample/help tests, docs review | PASS — execution details below |

## Contract impact

| Changed value | Producers/consumers/equivalents | Obligation/disposition | Checks |
| --- | --- | --- | --- |
| DepartmentRole | domain, SQL, department role API/roster/provisioning vocabulary | preserve lead-only synchronization/grants; multiple Reviewer rows supported through existing collection paths | domain + role persistence + affected department tests |
| Model selector and RoleReference | registry/compiler, publication capture/validation/storage, binding readiness, samples/help; agent picker and runtime later phases | exact pinned universe unchanged; role requirement source-derived; no fabricated agent revision | source/publication + old capabilities/forgery |
| Active binding department eligibility | lifecycle save/provisioning, channel ownership edits, child bundles | atomic checks under established company/association lock order, deactivation unaffected | DB positive/negative/race checks |

Trace evidence: `/tmp/openbot-02-agent-callers.txt`, `/tmp/openbot-02-save-binding-callers.txt`, `/tmp/openbot-02-channel-write-callers.txt`, `/tmp/openbot-02-department-role.txt`. Relevant graph source spans inspected; copy exact relevant evidence to logs for batch review. Current code freezes agent identity at publication and published revision at admission (plan prose predates that refactor); preserve that existing contract and extend its admission path in p02.

## Checks and final gates

During B1: focused source/publication/binding/role tests, formatting, locked offline all-target compilation, fresh migration and SQLx metadata after SQL changes, source reachability and patch whitespace. Additional tests follow actual affected interfaces. Final whole-repository check.sh, CI comparison, database-backed full suite, network isolation, stock stack/lease budgets run once at p05, with immutable results and unchanged thresholds. Plan03 has its own final gate after its integration. No checks executed or claimed PASS yet.

## Next action

Obtain independent D1/r1 architecture PASS before dependent source edits; implement all B1, focused verification, freeze exact artifact and request whole-batch review.

## Owned verification resources

Postgres pg_ctl (PostgreSQL) 18.6 (Homebrew), phase root `/Users/mac02/openbot-workflow-roles-tjdv6uy3`, localhost port 60242, migration DB `openbot_roles_p01`, test DB `openbot_roles_p01_test`. Entire new cluster belongs to this plan; no shared databases used. Test helpers create/drop only their own unique databases here. Started for p01; stop at task pause/completion.

## B1 implementation progress

D1/r1 accepted by root after reading B's saved PASS and unchanged snapshot. Source now includes Reviewer vocabulary/schema, exclusive static model selectors, source-derived role references in compiled/published identity, publication capture omission, compiler semantic revision 10, recursive child requirements, active binding eligibility and both ownership writer guards, role sample/help/language docs. Inactive configuration/deactivation remain available. Detachment inspection is bounded to 256 distinct active version bundles (one restored at a time); excess refuses safely as binding.role_inspection_limit rather than partially checking.

Focused tests added for every model kind, invalid selector cases, mixed universes, restore/forgery, sample/help, department and company binding refusal, both ownership writers, competing activation/removal and multiple reviewer assignments. No tests removed. Initial offline check B1-check-1 failed on a misplaced guard and missing import; both fixed, retained log. B1-check-2 in progress; test-module compiler diagnostics drive routine fixture fixes.

Verification update: B1-check-2.txt PASS (locked offline all targets, 2m48s); B1-migrations.txt PASS (fresh init schema on owned PG18.6). B1-role-tests-1.txt compiling. Both successful commands used only the owned cluster/cache. No full suite run; final gates remain p05.

B1-role-tests-1.txt FAIL: test code formatted LifecycleError as Display; fixed fixture assertions to inspect its Debug error wrapper. B1-role-tests-2.txt executed all 8 new tests: 7 PASS including all 4 database tests and the competing race; 1 FAIL because the pure compiler helper omitted its workflow dependency entry. The failure was fixture setup before the selector assertions, not a changed acceptance criterion. Added that required empty dependency-node entry, retaining all assertions; B1-role-tests-3.txt is rerunning. No check failure is omitted or mislabeled as a pass.

B1-role-tests-3.txt PASS: 8/8, no skips (four pure source/publication/sample tests and four real DB tests). Existing affected regression checks PASS: publication 43, registry 63, compiler 33, fixtures 18, DepartmentRole enum 3, forged-agent publication 1, binding SQL 7, lead replacement 1, competing lead assignments 1. Counts overlap new pure role tests where publication filter includes them; do not sum these into a distinct-test total. Commands/results retained in B1-regression-results.json and corresponding B1-*.txt. Formatting and whitespace PASS; reachability, SQLx regeneration and final offline check remain in progress.

## B1-R1 frozen implementation and executed mapping

- Base/start/HEAD remain 103004a8ac2fd643cf14bde7e09022ae79f72053; no commits or preserved pre-existing edits.
- Artifact: 91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d. Exact unchanged exclusions listed above. `B1-R1-source-bytes.json` pins all 24 changed source/deliverable paths; `B1-R1-implementation.tar.gz` retains their complete bytes for continuation without requiring a commit. Snapshot file and archive are workflow evidence, not substitutes for reviewing actual source.
- Coordination revision 1 checked again; I001–I003/r1 and C001/r1 still applicable/acknowledged; pin coordination-r1.md unchanged. No pending cross-phase request.
- AC1: workflow_role_source_selectors_are_exclusive_static_and_fixed_for_every_model_kind exercises all three kinds, both/neither, unknown/numeric/dynamic roles, non-model refusal and changed identity. B1-role-tests-3.txt PASS.
- AC2: workflow_role_publication_has_requirements_without_agent_capture_and_restores_exactly and workflow_role_publication_mixed_steps_keep_only_the_pinned_agent exercise canonical freeze/restore and extra-agent refusal. Existing workflow_agent_publication_refuses_forged_snapshot_and_keeps_frozen_version exercises the real publication boundary; B1-forged-agent.txt PASS. No mutable holder is captured by p01.
- AC3: all three workflow_role_binding_* eligibility/detachment tests exercise real persistence and both ownership writers. Competing activation/detachment waits for both production transactions, releases the company lock, and asserts exactly one succeeds and the final invariant. B1-role-tests-3.txt PASS. The shared Fixture uses a permissive application preflight port, while the production persistence authorization/locks/write methods are exercised; it does not prove HTTP authorization, which is unchanged. Child requirement traversal is additionally inspected, not claimed as a new exercised child-admission test; runtime child resolution belongs to p02.
- AC4: workflow_role_binding_reviewer_vocabulary_round_trips_multiple_holders assigns two Reviewer principals through the real API, reads both back and demonstrates unknown SQL role rejection. Enum round-trips and existing singleton lead write/race tests PASS.
- AC5: workflow_role_source_sample_and_help_explain_the_same_requirement freezes the shipped YAML and checks help for all three kinds. Existing 18 fixture tests, 63 registry tests, 43 publication tests and 33 compiler tests PASS, with overlapping test names explicitly not added into a distinct total.
- Structural checks PASS: formatting; tracked patch whitespace; source-module reachability; fresh migration; SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets; final locked offline all-target compilation. SQLx regenerated successfully with no metadata diff, expected because changed queries are runtime strings. Runtime DB checks supply behavioral proof. No tests removed/moved; semantic revision assertion updated 9→10 for the new compiler contract.
- Raw machine output is retained unchanged. docs/.gitattributes marks only logs/*.txt binary to preserve terminal/patch padding; source, deliverable Markdown/JSON and normal whitespace rules are unaffected.
- Evidence manifest: logs/02WR-P01-B1-R1.evidence.json. Retained initial FAIL attempts are explicitly historical pre-correction checks, not current successes. All phase-required affected checks now pass. Plan-wide final gates remain required at p05 and have not been exercised here.

Next: independent B reviews entire B1 from the original requirements and actual diff/callers; root accepts only after a matching digest PASS and complete saved findings/notes.

## Root acceptance B1-A1

Independent B PASS saved as 02WR-P01-B1-R1/N1 for 91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d; all 24 paths reviewed, 8/8 independent focused rerun, no actionable findings. Root recomputed matching digest, validated R1 evidence, checked persisted notes/findings and unchanged applicable coordination r1, and accepted p01 scope. A1 evidence manifest links root checks and retains all prior failures. Current source archive is a stable accepted prerequisite for p02 without a commit. No project source changed after freeze.

Mandatory p02 carry-forward: PublishedBundle::prepare_step_inputs still applies the pinned-agent universe guard to every model step; role-only execution is not supported by this phase and must use frozen role requirements at that runtime boundary in p02. Other role execution, captured holders, run views and live authority checks remain p02. B source-inspected recursive child requirements, active reconfiguration and the conservative 256-version inspection refusal; these are not claimed as newly executed behavioral cases. Whole plan completion remains p05 and all plan03 phases remain queued.
