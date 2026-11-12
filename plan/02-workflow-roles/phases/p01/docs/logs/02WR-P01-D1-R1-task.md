# 02WR-P01 — Role references and publication

Status: architecture decision D1/r1 awaiting independent review; implementation not started.

## Identity, ownership and continuity

- Workflow: implement-astra-reviewed-2-2; skill `/Users/mac02/.codex/skills/implement-astra-reviewed-2-2`.
- Root/A: `/root`, Sol 6.1/xhigh confirmed by user in this session. Actual runtime telemetry and effective compaction threshold unavailable; host defaults and durable checkpoints used, target 75% when exposed.
- B: pending direct-child Astra/high assignment, no inherited conversation; never authors project code/design.
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
| AC1 source and identity | registry model selectors, compiler | both/neither/invalid/non-model rejected; lead→reviewer changes identity; all three kinds accept role | workflow_role_source_ tests | NOT EXERCISED |
| AC2 publication | source uses/capture/freeze/store/restore | role version captures no agent, mixed captures only pinned, role metadata round-trips; forged universe refused | workflow_role_publication_ and existing forged-reference tests | NOT EXERCISED |
| AC3 binding/ownership | save_binding_on + channel settings under locks | department desk activates; company/bare desk refuses naming step; active-role detach refused; deactivate then detach succeeds; competing activation/detach cannot both commit invalid state | workflow_role_binding_ DB tests | NOT EXERCISED |
| AC4 fixed role/schema | domain parse, role assignment, migrated database | vocabulary round-trip; two reviewers coexist; lead still singleton; invalid SQL role refused | domain + department DB tests | NOT EXERCISED |
| AC5 authoring docs/sample | registry help and fixture source | help lists roles/rule; shipped role sample compiles and freezes with no agent | sample/help tests, docs review | NOT EXERCISED |

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
