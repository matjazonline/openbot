# Plan03 p04 — final source frozen; lease/CI/B acceptance pending

## Scope, roles and authority

User authorized finishing Plan03 with implement-astra-reviewed-2-3, normal push to main, then
finishing all Plan04 with the same workflow/main push. Stop before Plan05. No force push or
deployment. Root/A /root is user-confirmed Sol6.1/xhigh. Independent B /root/p04_reviewer is
Astra/high, originally started without inherited author context. Reuse its handle. Context controls
unavailable; durable records/host defaults apply. B owns REVIEW/notes/findings; root owns TASK/HANDOFF.

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04; branchfeature/plan03_04.
HEADf059f49808777abf0367d3a7f82d11e14bbd4e20; full reviewbase3fceb7d3435f3b821c3b37820200e1c5400206d9.
Fresh origin/main still2df562d6 (ancestor,10ahead/0behind); fetch again before publication.
Sole coordination authority is this checkout's plan/03-principal-addresses/COORDINATION.md,
root sole writer, revision45. I001–I004(stopbefore05),C001–C007/r1,revision10 unchanged/acknowledged.
Plan03p01–p03 and Plan02 prerequisites accepted earlier; p03 accepted3247c1f3…9230d/producer3fceb7d3.
p04/wholePlan03 remain UNACCEPTED. No main push or Plan04 implementation yet. Preserve all edits/logs.

## Exact current source and review

Material is STAGED and FROZEN at
1d5990aa52b4c252bd8104b3a3e625fda073f77487e2f834709c474bf6b87022
(logs/R13-source.snapshot.txt). Stage only material before this snapshot; do not restage/change source
while B reviews. Workflow records remain mutable/excluded. All code/test/config blobs match exact
CIcandidatefd5f7f76d6ee9788d2df2bd0544d309d78447723; three extra nonruntime Trail notes are review
material. R13-final-material-equality.json proves that. Current helper/test were adopted ONLY after
stock completed, with parent held and no child; R13-adoption-equality.json records the boundary.

B is NOW reviewing cohesive03PA-P04-B1-R9 source/correction/integration, closing B1-R8/F1-F2 only
with proof. No final phase acceptance until required pending gates finish. Assignment supplied
original Plan03 README/04 document and accepted D1/r5,D2/r4,D3/r1, coordination45 pin, exact exclusions,
full-source identity and verification mapping. Root will provide complete final immutable evidence
without repeating unchanged source review. B must not run runtime/Cargo/shared fixtures.

Seven exact snapshot exclusions:
- plan/03-principal-addresses/phases/p04/docs
- plan/03-principal-addresses/phases/p03/docs
- plan/03-principal-addresses/phases/p02/docs
- plan/03-principal-addresses/phases/p01/docs
- plan/03-principal-addresses/COORDINATION.md
- plan/02-workflow-roles/COORDINATION.md
- plan/02-workflow-roles/phases/p05/docs

## Completed verification and artifact boundaries

PRIMARY all-target gate passed4678library/0FAIL/27IGNORE in2553.31s,5binaryPASS,8sourcePASS.
Immutable logR8-main-complete-before-adoption.txt. STOCK-STACK repeated4678/0FAIL/27IGNORE at
2048KiB in2536.12s; immutable logR13-stock-complete-before-adoption.txt. No Rust/test/bound change
was made for the ten R7 failures; they passed both full reruns. Earlier R7 failed full output and
focused10PASS remain retained without an unrelatedness claim. Earlier33failures also allpassed;
canonical contracts9PASS, reader/Doop20PASS, independentB29PASS retained in R7 records.

Source boundaries in the live full-check log are explicit: primary ran3adf4b43…9ec8b; stock ran
9938338f…f438; lease runs1d5990aa…7022. All Rust/Cargo/migrations and both Darwin common preambles/
branches are byte-identical across them. R13-runtime-projection.json proves the unchanged scope.
Reuse those local main/stock proofs ONLY for that projection. New Linux behavior is proved by
exact current CI, not the old Darwin runs. Do not claim one snapshot covered the whole log.

D3/r1 accepted61d10837…66b45. Root wrappers use one standaloneMemory-owned Linux helper. It selects
a temporary primary GID for guarded preflight/command, retains original UID/groups/environment and
all egress/loopback/PostgreSQL restrictions, and leaves supervisor sockets outside the firewall.
It defers setup signal exit until successful creation publishes ownership, tracks hooks separately,
reports cleanup failures and retains GID if firewall cleanup failed. Native operation timer runs
INSIDE sudo3s+1s; outer5s+1s unchanged. Default childINT/TERM restored during bounded cleanup.
No resource limit raised, root coordinator or auth policy change. Actual Linux scope/signal/real
privileged-child deadline/no-late-effect/cleanup testPASS on R13CI43846081398 step16, saved in
R13-ci-43846081398-jobs1.json. Both entrypoints/extractedMemory, concurrent scopes, UID-only
counterexample, real creator interruption, env/args/status and partial-failure cleanup are covered.
Actual local shell/generated-fixture syntax and both unchangedDarwin wrappersPASS(R13-local-focused.txt).

Historical R9b/R10/R11/R12 stronger deadline attempts failed and are retained. R10/R11/R12
annotations show15s communicate timeout. Fixture initially inserted a subprocess before sudo;
corrected case execs real sudo. Signal restoration alone and topology correction alone were
insufficient; actual timer inside privilege boundary passed. No root-coordinator redesign/gate
was started. No unsupported sole-cause claim for past runner losses/OOM. Preserve native account
ordering/NOWAIT/historical identity/unknown-no-retry and MCP3/5/35second budgets, unchanged sinceR7.

## Active resources — NO HELD PARENT

Sole root Cargo/runtime worker is tool38573, scripts/check.sh parent66037, now LEASE gate.
Parent was correctly SIGCONT-resumed; logs/R13-parent-stage-resume.json records it. NO HELD PARENT.
No competing Cargo/shared fixture tests or source edits while this worker/review owns resources.
Full logR8-final-check5.txt remains mutable until the full script ends. B has no runtime worker.

Source /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh for Node24.21/GNUcoreutils/
Python3.13/PostgreSQL18, UTF8, mac02 DBs on localhost51167 and uniqueCARGO_TARGET_DIR
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target. Snapshot needs GNU PATH andLC_ALL=C LANG=C.
Full gate owns pa03_p04_dev_check_66037/its ownDBs. Diagnostic pa03_p04_r8_diag also ours.
Only verified phase-owned disposable resources on51167 may be cleaned; never other projects.
External author draft pa03-p04-r9-net now matches R13 helper/test; root has adopted exact bytes.
Old precleanup diagnostic checkout has timing prints and is not authoritative proof.

## Current CI, evidence and next actions

Current exact-code candidatefd5f7f76, branchcodex/plan03-p04-r13-fd5f7f76. MetadataR13-ci-source.json.
CI43846081398 and separateMemory43846081478 still active at last poll. Linux scope step and
memory-integrationPASS; required general/private/full jobs must actually complete. OlderR9
CI43836394223 privateDoop/memoryPASS, generalverify stillDBsuite at last poll; separateMemory
43836394480 bothPASS. Required current Linux helper behavior/full CI remains mandatory.
Unchanged agent-history workflow fails before jobs on every candidate; baseline identity retained
inR5 evidence. Leave confirmed unrelated workflow unchanged; do not claim all workflows green.

No localDocker/container/gh. Public GitHub API exposes job/step results; full joblogs403(adminrights).
Failure annotations now include source traceback. SSH normal pushes work. Actual Git HEAD/index
were not mutated by external CIcandidate creation; final source was deliberately staged later.
Preflight manifest logs/B1-R9-preflight.evidence.json valid(PASS5,FAIL2historical,NOTEXERCISED1remaining).
The validator verifies retention, not behavioral truth. TASK's latest R13 table maps criteria/assertions.

1. Poll worker38573/lease and actualcurrent CI. Retain failures, do not claim complete prematurely.
2. Receive B1-R9 findings/sourcePASS. Fix all actionable findings as one correction if any; otherwise
   keep source frozen and supply actual complete immutable final manifest once gates finish.
3. Validate final evidence, recheck source/coordination/main, obtain B exact final acceptance, then
   root accepts phase/wholePlan03. Stage final records/evidence without changing staged material,
   verify inclusion using --require-tracked, trailcheck, commit all retained work, normalpushmain.
4. Then start/finish Plan04 with same workflow; stopbefore05. Only four originalPlan04 docs were
   read while waiting; no Plan04 phase started. Plan04 has root membership/schema, access/grants
   including1000members/200desks measurement, then integration/full gate; auth suites after eachbatch.

Retain/commit five new learnings: l-070f6689,l-057c03ea,l-65d46a3b,l-0602646b,l-2fdea430.
This resume's Trail query savings152710tokens,15queries (inherited old tally not this resume).
Prior verbose handoff saved logs/R13-handoff-before-consolidation.md. TASK/REVIEW retain full history.
