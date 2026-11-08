# Plan coordination: 02-autonomous-agent

- Authority: /Users/mac02/dev/openbot-recovery/plan/02-autonomous-agent/COORDINATION.md
- Writer: /root, Sol 6.1/xhigh (user confirmed), primary /Users/mac02/dev/openbot-recovery
- Revision: 14

## Plan-wide instructions

- AUTO-I001/r1: Execute original numbered phases 01 through 07 sequentially. Reuse durable workflow/action execution; no additional scheduler or agent engine. Reset incompatible disposable data; no backward compatibility.
- AUTO-I002/r1: Root is sole author. Each phase has an independent Astra/high reviewer with no inherited conversation and no project authorship. Use implement-astra-reviewed-2-2 and phase-local records; gates for consequential decisions, one cohesive whole-phase batch review.
- AUTO-I003/r1: User authorized phase commits and local main fast-forwards; publish main after the whole autonomous plan completes. Knowledge prerequisite main ce93ff340a54db18770d8a5eec1c1d2008c5e5b1 is accepted and pushed. No deployment or production reset authorized.
- AUTO-I004/r1: Latest user instruction: “we should run only targeted tests”. Apply to authors, reviewers, corrections and final integration. Supersedes manual broad/full test runs in original phase 07 and repository guidance for this task. Preserve CI configuration and existing limits/guards. Record exactly selected/executed checks; do not claim a full suite pass or request that decision again.
- AUTO-I005/r1: Preserve accepted fix-app and knowledge contracts. Knowledge authority r24 (adcb7e538bb0f63433e72244c8e447cf051699325435626d52d7e5eaadca9fcd), immutable pin in p01 logs. Preserve independent AUTH_ATTEMPT_SECRET, Caller/owner/retention/usage semantics, knowledge authorization, run ceiling/content monotonicity, atomic search observation, live release checks and shared native model ownership.

- AUTO-I006/r1: User instructs checking weekly allowance every15minutes and writing a handoff/stopping at approximately10% remaining. Root owns the read-only local watcher at /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-tooling/autonomous-weekly-quota-watch.py, records sibling autonomous-weekly-quota/. It samples the actual current session codex rate_limits window10080minutes every900seconds, and writes STOP-WEEKLY-QUOTA.json at remaining<=10. Root must inspect latest/STOP at work boundaries; on threshold checkpoint all current work, stop owned workers/checks safely, retain work/evidence/resources for resume, and stop implementation. No full tests authorized. Current first sample30% remaining at2026-11-05T20:48:44UTC; next21:03:44UTC. Watcher unifiedexec session61159; do not start duplicate monitors. Active reviewers must honor a root budget-stop instruction and checkpoint rather than claim acceptance.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | /root author; B /root/autonomous_p01_reviewer; accepted/integrated e31ae98 | /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p01 (autonomous/p01) | plan/02-autonomous-agent/phases/p01/docs/{TASK,HANDOFF}.md within worktree |

| p02 | /root author; B /root/autonomous_p02_reviewer; accepted/integrated dc35ee4 | /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p02 (autonomous/p02) | plan/02-autonomous-agent/phases/p02/docs/{TASK,HANDOFF}.md within worktree |

| p03 | /root author; joint03–04 B pending | /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p03 (autonomous/p03) | plan/02-autonomous-agent/phases/p03/docs/{TASK,HANDOFF}.md within worktree |

| p04 | /root author; same joint03–04 B pending, no independent scheduling | /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p03 (autonomous/p03), sharedserializedmilestone | plan/02-autonomous-agent/phases/p04/docs/{TASK,HANDOFF}.md; joint evidencep03docs |

## Cross-phase notices

### AUTO-C001 / r1
- State: active
- Origin: p01 /root TASK.md AUTO-P01-G1-D1 decision revision2; G1-R2-N1 independent PASS.
- Targets: all current and future phases p01–p07.
- Instruction/change: Agent-owned typed binding/source and skill-assignment incarnations; immutable complete canonical revision/manifest content with native exact-set sealing; separate draft/live grants; actual tenant-qualified targets; existing registry revocation retains native references and tombstones live incarnations. Reuse existing execution/action systems and narrow secret interfaces. Exact full decision pin is p01 logs/AUTO-P01-G1-D1-r2-reviewed.md (see REVIEW for actual retained effective pin).
- Dependencies/order: Consequential design accepted before p01 implementation; later phases consume accepted p01 implementation only after WB1 PASS. No production admission change yet.
- Evidence/review: /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p01/plan/02-autonomous-agent/phases/p01/docs/REVIEW.md AUTO-P01-G1-R2-N1; original requirements plus effective r2 override text in TASK.md remain authoritative.
- Verification: targeted contract/storage tests, exact-set append refusal, real retention collector/competing claimants, same-root descendant attachment, retained v1 workflow pin across same-workflow repin. Preserve stricter MCP reviewed-version/schema checks. Phase-specific runtime implementation evidence remains required.
- Responses: p01 acknowledged r1 in its TASK/HANDOFF; future phases pending registration.


### AUTO-C002 / r1
- State: active
- Origin: p01 TASK AUTO-P01-G1-D1 decision revision3; native storage evidence WB1-storage-r1 and MCP grant FK/caller tracing.
- Targets: p01–p07.
- Instruction/change: Retain existing company MCP grant key rows with revoked state, project only active permissions, cascade revocation into retained reviewed operations and one-way agent binding grant incarnations; regrant never reactivates approvals. Keep native tenant-qualified keys and active-grant validation. See exact authored D1r3 for complete contract.
- Dependencies/order: Blocking affected grant-lifetime source edits until independent G1 PASS; initial C001/r1 remains effective for independent work. No implementation acceptance.
- Evidence/review: /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p01/plan/02-autonomous-agent/phases/p01/docs/TASK.md, G1r3 proposal and WB1-storage-r1.log. Independent PASS AUTO-P01-G1-R4-N1 for effective D1r4; three real native probes retained. Native activation holds FOR SHARE through commit; same-key reactivation requires a fresh review version.
- Verification: Actual grant removal/regrant with retained bindings, current projection, fresh review version, native active-grant invariant and whole-company cascade; targeted only.
- Responses: p01 acknowledged active r1 and accepted G1 effectiveD1r4 before affected edits; future phases pending.

### AUTO-C003 / r1
- State: active
- Origin: p01 TASK effectiveD1r5; WB1-skills-r1 existing company-skill deletion regression.
- Targets: p01–p07, especially phase02 publication and phase04 current authority.
- Instruction/change: Separate immutable assignment history/original skill identity from a nullable native current-skill reference retired only by source deletion. Skill deletion revokes assignment grants without losing frozen revision text; current authority intersects live pointer and one-way grant. Exact authored decisionr5 contains FK/scope/locking rules.
- Dependencies/order: Block dependent skill-lifetime edits until independent G1 PASS. C001/r1 and C002/r1 remain active for unaffected work. No implementation acceptance.
- Evidence/review: p01 TASK D1r5 and logs/WB1-skills-r1.log; independent PASS AUTO-P01-G1-R5-N1 for effective D1r5.
- Verification: Existing deletion behavior and library refusal, retained frozen text/IDs, native scope/retirement/ABA checks, both real competing deletion/read orders; targeted only.
- Responses: p01 acknowledged activer1 after G1r5 PASS; implementation resumes; future phases pending.

## Accepted producer artifacts

- p01 whole01A+B: accepted independent AUTO-P01-WB1-R2-N1 PASS digest3e0968ad96fb9e4aa6ce62163d077077d8865f8f0d37cd56e99613bb1ed82874 (reviewbasece93ff340a54db18770d8a5eec1c1d2008c5e5b1; solep01docs exclusion), code/evidence commit e31ae98defcc6e6dea4c2778f5561ef15e4e716d. Localmain identical fast-forward integrated; no autonomous push yet. All G1/WB1 findings closed at recorded scope. C001–C003 now have accepted implementation producer identity for downstream phases. Native16storage/8contract, affected existing57+13skills/final5caller tests and correction16+13+exact1 checks PASS; independent8+16+correction4 PASS. Targeted-only scope; no full suite claim. Runtime consumers02–07 remain outstanding. p01 resources retained, no source changes after acceptance.

Phase02 startup acknowledgement: /root read revision7; I001–I006/r1 and C001–C003/r1 acknowledged for independent publication/current-source capture. Accepted producer e31ae98defcc6e6dea4c2778f5561ef15e4e716d is phase02 starting commit. p02 TASK/HANDOFF are authoritative for new phase; p01 records remain unchanged.

### AUTO-C004 / r1
- State: active
- Origin: p02 /root TASK AUTO-P02-G1-D1 decisionr1.
- Targets: p02–p07.
- Instruction/change: Independent agent resolution and atomic expected-draft/active publication with immutable scoped idempotency result; one-way identity grants, explicit revoke and active-pointer-only deactivation; historical content never reloads mutable skills, new admissions require active selection, retained historical revisions/current authority remain separate. Exact accepted decision is p02logs/AUTO-P02-G1-D1-r2-reviewed.md, including source-skill-before-draft-skill-link/assignment capture order.
- Dependencies/order: Architecture AUTO-P02-G1-R2-N1 PASS for effectiveD1r2 permits p02 dependent implementation; later phases consume accepted p02 implementation commit, not design alone. Existing C001–C003 remain active.
- Evidence/review: /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p02/plan/02-autonomous-agent/phases/p02/docs/TASK.md; independent PASS AUTO-P02-G1-R2-N1; F1 design-closed, actual competing publication/deletion proofs remain mandatory.
- Verification: p02 targeted resolver/publication/concurrent writers, immutable requests, historical replay, grant ABA and deactivation/deletion tests; runtime integration remains03–07.
- Responses: p02 acknowledges activer1 and accepted D1r2 before dependent edits; future phases pending accepted producer.

Phase02 WB1 boundary: root implementation and all focused author checks complete, independent whole-code review pending; no p02 implementation acceptance or commit. Actual B /root/autonomous_p02_reviewer (requested Astra/high, independent context) reused after accepted D1r2 gate. Active I001–I006/C001–C004 r1 unchanged; C004 still requires accepted implementation producer for later phases. Source/evidence pins in p02 TASK/HANDOFF. Targeted-only scope unchanged.

- p02 whole02A+B accepted independent AUTO-P02-WB1-R1-N1 PASS digestc234ff74b383cd488c78d0911e2e878e1e3ffa66f54365409519da0b68a82113 (basee31ae98defcc6e6dea4c2778f5561ef15e4e716d; solep02docsexclusion), code/evidence commit dc35ee426d8f88abdaf40562027ae69ad66863eb. Localmain identical fast-forward integrated, noautonomouspushyet. C004/r1 now has accepted implementationproducer dc35ee4 for downstream03–07. G1F1 verifiedclosedincode; no newWB1findings. Authorfocused40/31/7/4 andindependent35+2PASS; actualmigrationchecksumsbothownedDBsverified. CurrentClippy/offlinealltarget/SQLx/fmt/diff/stricttransportPASS; nofullsuite/later-runtimeclaim.

Phase03 startup: /root readrevision12 andaccepted02producerdc35ee426d8f88abdaf40562027ae69ad66863eb. I001–I006/C001–C004r1 acknowledgedforworkflowreferences/admissionmanifest; p03startsfromexactacceptedlocalmain. Original03A+Bwholephaseauthorized; designinvestigation/gatepending, no dependenteditsyet. Existing runtime/provisioning consumers must be traced and coherently adapted to changed shared interfaces; no backwardcompatibility or final04/05acceptance inferred.

## Scheduling amendment — AUTO-I001 / r2

- State: active; supersedes only r1's separate03/04 implementation checkpoints, retains original01–07 requirements and ordering. User authorized entireautonomousplan; no newscope/publicationpermission inferred.
- Execute original03 then04 steps within ONE cohesive03–04 milestone before codeacceptance, because existing prepare/dispatch/currentauthority callers consume embeddedagent definitions andMCPslots that03 removes. A partialinterface transition would require a compatibilitypath or a brokenruntime. Selected2.2workflow explicitlysupports cohesive milestones; do not create either. This is dependent shared-interface work, not independentphaseexecution.
- Sourceworktree /Users/mac02/dev/openbot-recovery/.worktrees/autonomous-p03 branchautonomous/p03; rootsoleauthor. Phase03records primary, phase04records link joint evidence and keep own originalcriteria/notice acknowledgements. One independent Astra/high B reviews all original03A+B and04A+B plus necessary affected producers coherently; no mid-layerreviews, acceptwholemilestonebefore05. Bothrecordroots are exactworkflow-only exclusions whenfrozen; no sibling source excluded.
- WholemilestoneG1 must cover reference/bundleformat, immutableadmissionclosure andparentchildmanifestownership, frozenpreparation/directtooltargets, exactliveauthority/leases/callscope. Original05/06/07 acceptance remainsseparate; necessary produceradaptations are traced, reviewed andpublished tofutureconsumers ratherthan treated as completed05. No sourceeditsuntilG1PASS.
- No separatephase04worker/worktree/DB/concurrentwriter; resourcesownedjointlyunder/root PG55603. Commit/localmainFF afteracceptedjointartifact; pushauthorizationunchangedwholeplanonly. All targeted-only/quotastop/security/acceptedproducercontracts unchanged.
