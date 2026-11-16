# IMPORTANT LIVE STATE — check parent held, main continues

Root heldONLYcheck.shparent66037 withSIGSTOP afterverifyingcwd/command/currentCargochild.
Tool38573 remainsactive; currentmain test childcontinuesunchangedon3adf. Record
logs/R8-parent-stage-hold.json. No competingruntime/Cargo/fixtures/sourceeditsbeforeprimarysuiteends.
Afteractualmainresults, adopt4exactexternaldraftoverrides(includingnewhelper), snapshot/rootcandidate
blobequality/affectedchecks, thenSIGCONTparent forunchangedDarwinstock-stack/leasepaths. Retainmain
proofonlyforbyte-identicalRust+Darwinprojection, newLinuxproofcomesfromexactdraftCI. Finalmanifest
muststateartifactboundary; donotclaimsingle-snapshotwholelog. DoNOTforgetheldparent. Ifactualfailure
needsRustcorrection, recordpropercorrectionboundary instead. No mainpush/Plan04yet.

# Plan03 p04 — R8 final verification active

## Scope and authority

User authorized finishing Plan03 with implement-astra-reviewed-2-3, normal push to main,
then finishing all Plan04 with the same workflow/main push. Stop before Plan05. No force push
or deployment. Root/A /root is user-confirmed Sol6.1/xhigh. Independent B /root/p04_reviewer
is Astra/high, originally spawned without inherited author context; reuse its handle.
Context controls unavailable: durable records/host defaults. B owns REVIEW/notes/findings;
root owns TASK/HANDOFF. Sole coordination authority is this checkout's
plan/03-principal-addresses/COORDINATION.md, root sole writer, revision43. Applicable
I001–I004(stopbefore05), C001–C007/r1 and revision10 unchanged and acknowledged.

Checkout /Users/mac02/orca/workspaces/openbot/plan03-04; feature/plan03_04; HEADf059f49808777abf0367d3a7f82d11e14bbd4e20.
Full review base3fceb7d3435f3b821c3b37820200e1c5400206d9. Last fetched main2df562d6… remains
ancestor of HEAD; fetch again before acceptance/publication. Preserve all edits/logs. No main
push or Plan04 implementation yet. Plan03p01–p03 and Plan02 prerequisites accepted previously;
p03 accepted3247c1f3…9230d, producer3fceb7d3. p04 and whole Plan03 remain UNACCEPTED.

## Current artifact and reviews

Material3adf4b435a51f0b482d3d372c43398a57cbc20c1823394f14fbfa3ab2a29ec8b is FROZEN
(logs/R8-source.snapshot.txt). Exact seven snapshot exclusions (unchanged):
- plan/03-principal-addresses/phases/p04/docs
- plan/03-principal-addresses/phases/p03/docs
- plan/03-principal-addresses/phases/p02/docs
- plan/03-principal-addresses/phases/p01/docs
- plan/03-principal-addresses/COORDINATION.md
- plan/02-workflow-roles/COORDINATION.md
- plan/02-workflow-roles/phases/p05/docs

R7 source/focused-proof passed independent03PA-P04-B1-R7/N1 on88d4c4a7…04362.
All35modifiedRustblobs still identical (logs/R8-unchanged-rust-source.json);4705Rusttestinventory
unchanged. Preserve native live-source account ordering/NOWAIT, historical immutable identity,
unknown-effect quarantine/no blind retry, and MCP3/5/35second cleanup budgets.

D3/r1 decision61d108373b190c52802eec281abdca7e00466f409dd339270d0e981ab7066b45 passed
independent03PA-P04-D3-R1/N1; coordination43 activated it. R8 changes only Linux isolation
wrappers, new real-socket regression, CI gate and local Linux gate. Linux now allocates a temporary
primary group for preflight+command, retains original UID/groups/env/cache permissions and targets
that GID in firewall rules. Existing egress/loopback/PostgreSQL rules unchanged; Memory wrapper
stays standalone. New regression exercises concurrent root/extractedMemory wrappers, descendants,
supervisor sockets, UID/env/args/status, partial setup/preflight cleanup and selector-only UID
counterexample. Local syntax and both unchanged Darwin preflights/commandsPASS; Linux actual
behavior pending CI. B1-R8/N1 returned F1medium/F2low; both OPEN. B is completed/reusable, no runtime worker.

## Verification and active resources

R7 main fullrun:4668PASS/10FAIL/27IGNORE in4979.29s; full buffers logs/R7-main-failures.txt.
Held parent33669 correctly terminated via TERM+CONT after main finished; tool77624 exited143.
No held parent remains. R7-correction-boundary-stop.json retained. Ten cases allPASS together
with4parallelthreads in130.17s (R8-ten-failures-replay1.txt); quotaalonePASS3.08s. Do NOT infer
unrelatedness or full-green from focused replay. Prior33failures also allPASS after canonical
single-validation correction; previous33/9contracts/20integration/B29proof retained in TASK.

CURRENT sole root Cargo/runtime worker: tool38573, bashcheck parent66037,
complete scripts/check.sh, log logs/R8-final-check5.txt, RUST_TEST_THREADS=4.
It owns cache and newly-created pa03_p04_dev_check_* fixtures on PostgreSQL51167. No B runtime
worker. No competing Cargo/shared fixtures/source edits while full gate owns these resources.
Stock-stack/lease and all required gates still mandatory. Do not stop another project's workers.

Source /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh for pinnedNode24.21,
GNUcoreutils/Python3.13/PostgreSQL18, UTF8locale, DBs pa03_p04_dev/test on localhost51167 asmac02,
CARGO_TARGET_DIR=/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target.
Snapshots need GNU PATH plus LC_ALL=C LANG=C. Diagnostic DBpa03_p04_r8_diag is phase-owned.
Only exact phase-owned disposableDBs on51167 may be cleaned. Old external diagnostic checkout
pa03-p04-precleanup-diag has timing prints and is NOT authoritative source proof.

## CI and next steps

Current exact-material candidatee888c26bf0738ed553abd42efb6ae763ae474347 on
codex/plan03-p04-r8-e888c26b was pushed normally via SSH using an external alternateGitindex;
actualHEAD/index unchanged. R8-ci-source.json retains blobs/identity. CurrentCI43833996212 and
Memory43833996248 active. PriorR7candidate957de863 privateDoop+memoryPASS; separateMemorybothPASS,
but generalverify lost runner communication during isolatedDBsuite. Exactannotation retained.
Both oldLinuxwrappers selected invokingUID and therefore also affected same-user control sockets;
D3 corrects that substantiated scope defect. Do not claim every prior loss was caused by it or OOM.
Current Linux regression must establish actual behavior. No localDocker/container/gh executable;
use publicGitHubAPI for job/step results, SSH for normal pushes. Fulljoblogs may need auth that
is unavailable; do not expose credentials. Unchanged agent-history workflow again fails before
jobs; prior baseline identity evidence retained, do not silently fix unrelated workflow.

1. Poll current localfullgate and CI; retain actual failures. B will return R8sourcefindings/PASS.
   No finalacceptance until realLinuxproof and requiredfull/local/private/browser/stock-stack/lease
   gates pass. If source correction needed, record explicit boundary and run affectedchecks/review.
2. Create final immutable evidence manifest, validate it, recheck coordination/source and obtain
   B final acceptance. Use actualevidence, notpassingstepnames as behavioral claims.
3. Recheckmain, trailbuild/check, accept exactartifact, commit allretainedrecords/evidence and
   uncommittedTrail learningsl-070f6689+l-057c03ea, normalpushmain. Then startPlan04; stopbefore05.
   Candidate performance/network learnings can be saved after sourcefreeze releases, with identity
   and review accounting; do not introduce hidden material exclusions.

TASK/REVIEW are detailed records. Earlier handoff with all old paths/gates retained in
logs/R8-handoff-before-consolidation.md and logs/R6-handoff-before-consolidation.md.


## New correction draft while R8 proof remains frozen

B1-R8/F1: INT/TERM after successful resourcecreation butbeforeflagpublication leaksownedresources.
B1-R8/F2: cleanupstillunbounded. Author correctedonlyexternaldraft
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-r9-net, NOTroot3adfsource. Draftconsolidates
Linuxmechanism atservices/memory/scripts/network-isolation-linux.sh (bothwrapperentrypointsreuse;
Memoryextractionstillselfcontained), deferssetupsignalexituntilownershippublished, tracksinstalled
hooks, addsnativecleanupdeadlines/noninteractive sudo, visiblefailure+safeGIDretention. Drafttests
signalactualcreators andholdactualcleanupafterdeletion. Localdraftsyntax/generatedfixtures/Darwin
PASS; Linuxpending. NewexternaldraftCIcandidate46fc08d4…4023, metadataR9-draft-ci-source.json,
branchcodex/plan03-p04-r9-draft-46fc08d4 pushednormally. ItprojectsR8treeplus4pinnedexternalfiles;
actualrootHEAD/index/material remainsfrozen3adf foractivefullcheck38573. Afterfullrunfinishes,
applytestedbytesandproveexactcandidateblobequality/newrootsnapshotbeforeaffectedBacceptance.
CurrentR8CIrealLinuxscopeproofPASS; privateMemoryintegrationPASS. No mainpush/Plan04yet.


Latestverification: externaldraft46fc08d4 Linuxscope/signal/deadlineteststepPASS
(R9-draft-ci-43836394223-jobs3.json); memoryintegrationPASS, remainingCIactive. R8localmain~2750PASS/
0FAIL, alltenR7failedcasesPASSagaininfullrun. No rootimplementationchanges. Continuefullgate38573;
thenapplythe4exactdraftoverrides(includingthenewhelper), recomputerootsnapshot/equality andrequestBcorrectionreview.
