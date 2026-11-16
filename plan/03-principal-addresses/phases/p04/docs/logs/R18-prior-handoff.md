# Plan03 p04 — committed checkpoint pushed to main; unfinished edits remain local

Latest user request: **"push to main"**, following merge-only/no-test instruction.
Published committed HEADd8fdd990 to main by normal SSH fast-forward push; remote tip verified.
The existing uncommitted Plan03 implementation/evidence remains local and was not included.
No tests rerun by the agent. Plan03 remains unfinished; Plan04 has not started.

## Worktree, authority and source

- Worktree: /Users/mac02/orca/workspaces/openbot/plan03-04
- Branch: feature/plan03_04
- HEAD/merge: d8fdd9909e9ebfaac45541044ed4317023d81ac3, parents f059f498 and origin/main6e55258c.
- Full original review base:3fceb7d3435f3b821c3b37820200e1c5400206d9.
- Current staged material artifact:0e3209cada08f6ad800801cf8b9303415985afb6c8e0c6712e636a9d93f4be95; logs/R17-post-main-merge.snapshot.txt.
- Sole coordination authority: this checkout's plan/03-principal-addresses/COORDINATION.md,
  root sole writer, revision51. I001-I004/r1 and C001-C007/r1 unchanged.
- Root/A /root: user-confirmed Sol6.1/xhigh. B /root/p04_reviewer: retained independent
  Astra/high, no inherited author context, idle. Root owns TASK/HANDOFF/coordination;
  B owns REVIEW/notes/findings. Context controls unavailable; durable checkpoints apply.

Prior authorization remains finishing Plan03 and then Plan04 under implement-astra-reviewed-2-3;
stop before05. User explicitly authorized publishing the committed checkpoint now, independently
of unfinished Plan03 acceptance. This push is not final acceptance of uncommitted work.
Do not infer permission to rerun tests merely because main was merged or pushed.

## What the merge retained

Upstream expanded local verification and sharedscripts/check-doop.py. Kept the original grouped
gate failure handling and upstream coverage. Existing Plan03 Linux scope, pinnedUbuntu runners,
CI debug-metadata settings and OCI setup remain. Existing Doop failure annotations and image
log display moved into the sharedhelper ratherthan restoring duplicatedinline workflow logic.
No MCP catalog correction has been authored.

All593nonoverlapping edited/evidence files retain exact bytes/modes; three overlapping files
were reconciled. Original50stagedpaths restored, plus sharedhelper carrying existingdiagnostics;
workflow records remain unstaged. Backup: /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-main-merge-akrlfdl2.
Preservation proof logs/R17-main-merge-preservation.json; no other worktree/stash touched.
Temporary ownedstash was removed only after verification; future work must honor the personal
no-stash preference discovered during this merge.

## Accepted checkpoints and remaining gap

Plan03p01-p03/priorPlan02 prerequisites accepted earlier. p04/wholePlan03 remainsUNACCEPTED.
B1-R10/N1 source/focusedPASS applied to historical6207f5af…7f88f on HEADf059f498/coord48; all
source findings, including R9/F1 uncertainLinuxsetup ownership, were resolved there. This is
not final plan acceptance or a pass for the new merge identity.

Historical local primary4678PASS/0FAIL/27IGNORE plus5binary/8source cases; stock2048KiB
4678PASS/0FAIL/27IGNORE; lease139PASS. Missing browser prerequisite repaired and exactJS8PASS.
Current premerge four fixture corrections + two oldDoopfailures passed6cases locally in77.75s
with TEST_DATABASE_URLunset. Focused stackenvironment was not retained; no2MiBclaim for thatrun.
Strict alltargetClippyPASS; inventory4705unchanged;108manifests valid at that checkpoint.
Reuse only under proven source/environment/fixture projections; newrunner was not executed.

ActualLinuxfocus43867269846 on exactpremerge production/testsource FAILED17PASS/6FAIL in
210.98s at4threads/4CPUs, originaldeadlines. Five Doopdocumentcases timedout (onewaiting for
reconciliation aftercreate_frame); BeforeWake crashchild reachedCLAIM+PREPARED then returned
withoutREADY. No missing-env failure. Logs R16-linux-focused-failure.txt/platform/downloadidentity.
This gap remains genuine; do not call prior localPASS or privateDoopPASS its resolution.

B1-R10/N2 independently clarifies originalplan/repo rules allow focused correction proof and
unchanged-gate reuse; a wholeCIrerun after every edit is not intrinsically required. Appropriate
actualLinuxproof/evidence-backed disposition is still required. No criterion waived.

Premerge exactd68c7353 Memoryverify/image, memoryintegration and privateDoopintegration PASS;
generalCI43863539586 was active at lastpoll. Historicalagent-history workflow failsbeforejobs
onbaseline andcandidates; unrelatedbaselineidentity retained inR5. No all-workflows-greenclaim.

## Current investigation, resources and next action

R17 currentexactbinary7f486896…36d8 profiling completed19PASS/4FAIL97.38s; sample canperturb
timeouts, so this is diagnostic. CPUprofile/demangled counterpart showMCPcatalog validation,
JSONschema compilation andworkflowYAMLrestore. Source proves strictMCPpublication read calls
read_catalog thenrow.try_into revalidates the samecatalog. R17-catalog-callers.txt traces23files.
Next proposed implementation is one-row reuse of the alreadyvalidatedprivateReadCatalog while
preserving strictrejection, ordinaryquarantine, allfreshauthority reads and originaldeadlines.
No correction authored or reviewed. Follow the latestuser scope before continuing implementation.

No active localCargo/runtimeworker or heldparent. PG18 onlocalhost51167, phase-owned databases
and uniqueCARGOTARGET remain for resumption. Source externalenvfile
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh forNode24.21/GNUtools/Python3.13/PG18.
Snapshots require GNU_PATH/LC_ALL=C/LANG=C. Only verified phase-owned resources may be cleaned.
No localDocker/gh; publicGitHubAPI jobstates available. Fulljoblogs via authenticatedbrowser.
Orca computer-use skill loaded; dedicatedChrome29615 downloadedR16 archive successfully;
Orcawindow357 restoration performed. No GUI interaction currently needed.

On future continuation: reconcile current source/coordination and user instructions; fix actual
Linuxprogress gap with evidence, then independentfinalreview/acceptance. Retain historical failures.
Before publishing remaining implementation, validate retained evidence inclusion, refresh/checkTrail,
freshfetch and normalmainpush when complete. Then start Plan04, stopbefore05. All old source/check identities remain
historical; do not assume newsource acceptance.

Exact seven source-snapshot exclusions:
- plan/03-principal-addresses/phases/p04/docs
- plan/03-principal-addresses/phases/p03/docs
- plan/03-principal-addresses/phases/p02/docs
- plan/03-principal-addresses/phases/p01/docs
- plan/03-principal-addresses/COORDINATION.md
- plan/02-workflow-roles/COORDINATION.md
- plan/02-workflow-roles/phases/p05/docs

Preserve all edits, logs and five stagedlearnings. Prior fullhandoff is retained at
logs/R17-handoff-before-main-merge-update.md; TASK/REVIEW carry complete history.
