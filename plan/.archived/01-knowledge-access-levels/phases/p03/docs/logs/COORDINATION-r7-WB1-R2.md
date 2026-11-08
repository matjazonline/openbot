# Plan coordination: 01-knowledge-access-levels

> No backward compatibility. Edit init schema and reset owned disposable databases; no backfills, dual paths, legacy decoders or staged rollouts.

- Authority: /Users/mac02/dev/openbot-recovery/plan/01-knowledge-access-levels/COORDINATION.md
- Writer: current Codex /root, primary /Users/mac02/dev/openbot-recovery
- Revision: 7

## Plan-wide instructions

### KNOW-I001 / r1 — Scope and ordering
Implement the original README and six group files, all user decisions D1–D6/P1–P9 confirmed. Group order01→03→02→05→04→06. Search04 ships after egress03/library02/readpaths05. Global channel-handles-send03 and agent-history04 plans are out of this requested implementation.

### KNOW-I002 / r1 — Roles, records and resources
Root/A /root is user-confirmed Sol6.1/xhigh; reuse confirmation. Independent B per phase Astra/high, no inherited author history, no project authorship. Selected skill absolute /Users/mac02/.codex/worktrees/b109/openbot/.agents/skills/implement-astra-reviewed-2-2. One cohesive phase batch, one initial architecture gate for costly decisions, no intermediate code reviews. Root owns TASK/HANDOFF; B owns REVIEW/notes/findings. Keep immutable source/decision/authority/check evidence. Context threshold unavailable.
Historical accepted phasep01 worktree /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p01, branchknowledge/p01, base73799dc6ac685008d5c97f154e861d11ae15bfa0. PrivatePG18.6 /Users/mac02/dev/openbot-recovery/.worktrees/pg-knowledge-p01, loopback55501/mac02/open_bots+open_bots_test, ownsocket/target, max_locks256. ExplicitURLexports for cargo; ignored0600.env onlyURLs. CARGO_INCREMENTAL0. Managerownsresource; neverreset/stop siblings. Laterphase resources allocated atstartup.
This authoritative coordination file remains machine-local inprimary and is nevercommitted; phase pins preserve contracts. Worktree copies are not authority.

### KNOW-I003 / r1 — Acceptance and publication
All fix-app01–08 accepted/integrated/publishedmain73799dc6ac685008d5c97f154e861d11ae15bfa0, source/evidence retained. Implement knowledge groups sequentially; acceptedphase commits/localmain integration authorized. Pushmain ONLY after wholeknowledgeplan complete, independentlyaccepted and finalgatesfulfilled. Thenautonomous01–07 withsameworkflow anditscompleted-plancommit/push. Useralreadyauthorizedtheseactions; no reconfirmation. No deployment/productionreset. Preserve freshorigin/main and otherusers'work.

### KNOW-I004 / r1 — Verification
Focusedmeaningfulchecks perphase andaffectedinterfaces, nonzero filter counts, freshownedmigrations andliveSQLxalltargets afterSQL. Finalwhole-planFULLsuite per06 and current .github/workflows/ci.yml authoritativeinventory, includingnetworkisolation/stack/lease/browser/imagegates. Fix-appI004targeted-onlywaiverDOESNOTapplyhere. Recordcommands/outcomes/failedorunavailablechecks; neverclaimfullverificationifrequiredcheckunavailable. Stackframesbefore/after04, boxembedderseam, no raisedlimitwithoutnewearlyfailureCIguard.

### KNOW-I005 / r1 — Preserve established contracts
Retain fix-app C002 company/owner atomicity, retired identities/history, Caller/application authority, independent requiredAUTH_ATTEMPT_SECRET/noJWTfallback, guardedqueue/fence/continuation protocols, global90dayretention and immutable usagehistory. Liveusage5ownershipIDsandharnessworkflowownership remainrequired; archivedusage has no runtimeownerlinks. No restoredconstantqueuecolumns/payloaddecoders/retiredvalues. Resolve historical plan line references through currentgraph/source.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF |
| --- | --- | --- | --- |
| p01 accepted/integrated | root/A; B /root/knowledge_p01_reviewer, G1/WB1-R2 PASS | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p01 | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p01/plan/01-knowledge-access-levels/phases/p01/docs/ |

| p03 implementing | root/A; B /root/knowledge_p03_reviewer, G1-R1 PASS; WB1 pending | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p03 | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p03/plan/01-knowledge-access-levels/phases/p03/docs/ |

## Cross-phase notices

### KNOW-C001 / r1 — Accepted p01 shared access contracts (all dependent phases)
- State: active.
- Origin: p01, root/A; producer TASK/HANDOFF and logs/root-WB1-acceptance.json in .worktrees/knowledge-p01/plan/01-knowledge-access-levels/phases/p01/docs/.
- Targets: p02,p03,p04,p05,p06 and future dependent knowledge phases.
- Dependencies/order: effective after accepted p01; reviewed code4bc57b3 plus source-preserving evidence/integration commits through ef12dc51e24609cad75ab1a354b81c8588578fca.
- Evidence/review: actual KNOW-P01-G1-R1 and KNOW-P01-WB1-R2 PASS/N1, F1 closed; exact artifact identities below and179 focused tests.
- Verification: dependent contract/caller audits plus phase-focused tests; preserve p01 behavior in final06 full current-CI gate.
- Responses: p03 acknowledged C001/r1 at startup; authoritative .worktrees/knowledge-p03/plan/01-knowledge-access-levels/phases/p03/docs/TASK.md and HANDOFF.md. Other future phases pending startup acknowledgement.
- Instruction/change:
P01 accepted by actual KNOW-P01-WB1-R2 PASS (note N1), F1 closed, root exact artifact proof. Source code commit4bc57b3, accepted installed2474cb464ac7b4e31e09102c4b37f551f5cc7ab8514ba13a899031e6c58515ac, diff a99d4f5447f43e04e2346f283bdf587f86043ae9b70d1040ee6c8d3a9762deea, tree1fadfc702fd6508d7c10e6a1c355e3a25123c37487de42784fbea8284682a62b. Full phase evidence under p01/docs,179 focused tests PASS, all14 selectors nonzero, Clippy/offline/SQLx/fmt PASS. Preserve:
- One AccessLevel L0–L9 domain type and public.access_level SQL domain. Owner rows explicitly9/fixed; outside/observed/former identities PUBLIC. No restored Disabled identity state.
- PrincipalAccessContext and CompanyAccess carry level from the same read as membership; invalid facts/lookup errors propagate. Email proof withholding remains independent.
- may_assign_level(LevelActor,LevelTarget,requested) is the shared rule. Admin target/current/request ceilings use fresh current authority; ordinary members cannot assign. Owner-only role/address/create/delete/removal powers do not broaden to admins.
- Actor-bearing named invite/level commands; new short writer lock order actor account SHARE → existing invite when relevant → company NO KEY UPDATE → deterministic member rows;1slock/5sstatement timeout and controlled conflict. Future dependent protocols must account for these locks.
- Invitation level only copies on NEW membership insertion. Existing member accept/replay preserves current level, owner9 preserved.
- UI/JSON/browser level-only entrypoints use this application policy; strict payloads reject role injection; self-demotion refreshes current caps. No compatibility defaults on stored Invite payloads.
P03 must acknowledge before defining run/history/audience/release contracts; later groups preserve these facts. Whole knowledge/final06 release gate remains outstanding.


- Revision2: p01fresh55501resources/bothmigrations/exactchecksums andofflinealltargetbaselinePASS88s. RootrecordedwholephaseG1r1proposal; codepristinebase73799dc. FreshindependentBknowledge_p01_reviewerG1assignmentfollows. TechnicalI001–I005r1unchanged. No dependentimplementation/phaseacceptance/KBpush.

- Revision3: p01 whole WB1 implementation authored; 175 focused native tests PASS, zero failures/ignored, all14 selectors nonzero including20 access_level_ tests and real competing PostgreSQL writers. Live all-target compilation PASS; author clippy/cache/offline checks and final diff audit precede whole-batch review. G1 decision r1 remains accepted without architectural changes. No p01 code acceptance, commit, integration or knowledge publication. I001–I005/r1 technical requirements unchanged; no cross-phase notices published before acceptance.

- Revision4: p01 accepted, committed and locally fast-forward integrated at 16ea8cefc1093a39c8caa47374ce754c9a59f719. No remote knowledge publication. C001/r1 publishes reviewed shared contracts for dependent phases. Next p03 allocation/G1; I001–I005/r1 unchanged.

- Revision5: p01 final source-preserving metadata ef12dc51e24609cad75ab1a354b81c8588578fca fast-forward integrated. Registered p03 at /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p03, baseef12dc51e24609cad75ab1a354b81c8588578fca, private55503/fresh target, root/A with fresh B pending. P03 acknowledges I001–I005/r1+C001/r1 in its own records. Startup setup/baseline underway; no p03 design PASS or implementation. Current resource ownership is phasep03; preserve accepted p01 resources/evidence.

- Revision6: metadata clarity labels p01 resources historical; current p03 resources remain55503/fresh target per directory and phase records. P03 pristine baseline bothmigrations/schema/offlinecompile/525focusedPASS, root recorded complete G1r1 decisions for whole03A–D. Fresh Astra/high B follows; no dependent implementation before actualPASS. I001–I005/r1+C001/r1 unchanged.

- Revision7: p03 whole G1r1 accepted by actual independent B PASS/N1 and root exact proof. Whole03A–D implementation underway: admission, atomic history/child levels, message stamps, common release boundaries, typed refusal and connection controls authored. New native focused r5 32/0/0 PASS; expanded affected regression run underway, findings under correction before any whole-code review. No p03 code acceptance/integration/publication. I001–I005/r1+C001/r1 unchanged.
