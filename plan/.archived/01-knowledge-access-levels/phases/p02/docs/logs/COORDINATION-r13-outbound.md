# Plan coordination: 01-knowledge-access-levels

> No backward compatibility. Edit init schema and reset owned disposable databases; no backfills, dual paths, legacy decoders or staged rollouts.

- Authority: /Users/mac02/dev/openbot-recovery/plan/01-knowledge-access-levels/COORDINATION.md
- Writer: current Codex /root, primary /Users/mac02/dev/openbot-recovery
- Revision: 13

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

| p03 accepted/integrated | root/A; B /root/knowledge_p03_reviewer, G1-R3/WB1-R2 PASS | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p03 | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p03/plan/01-knowledge-access-levels/phases/p03/docs/ |

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


### KNOW-C002 / r1 — Accepted run, content-consumption and release contracts
- State: active
- Origin: p03 root/A; actual KNOW-P03-WB1-R2/N1 PASS, F1/F2FIXED/CLOSED; producer sourceaac35b772d3dc5ae555e00a0077b2f9a60e4925c locally main-integrated. Accepted installed6c1c22b53202a22db87f5384f4290129c46e8bbcedc1607adfa261deac8578dc/tree5814560962264ec3c568ea3d5c7ab31e449b641856d2d9b9597ce4ceec10fad2, baseef12dc. Full original G1r1 and reopened G1r3 decision4e447c7e1cf163ab4d5f0e5e7382c37b9012d49ed35ecb0fe184334e3682264e and phase03docs retain evidence.
- Targets: p02,p05,p04,p06.
- Contract: Reuse AccessLevel/audience_level/may_release and strict RunKnowledge. Root ceiling is immutable/current admission audience+actor; triggering stamp is initial content; children inherit both under parent ownership. Content is monotone and terminal facts immutable. Human messages PUBLIC; workflow canonical messages stamp the locked producing run in the write transaction. Higher history rows are excluded before counts/newest bounds.
- Contract: Scoped persistence workflow::knowledge::raise_content_level_on(db,CompanyId,RunId,AccessLevel) is caller-owned, not an authorization endpoint. Any new mutating read must independently establish its actual live claim/lease/deadline and atomic output+rise, including server-commit validity. Do not assume a formerly read-only context path already has that fence. History uses exact-claim workflow_history_consumptions proof with composite FKs, identical replay/immutable/cascade guards and deferred live-fence guard. Child terminal consumption/continuation/result replay raises atomically without reverse child-row locks.
- Contract: Preserve ONE common knowledge_release evaluator at NEW intent freeze and actual NEW message/HTTP/MCP/local effect boundaries. Record freeze levels, use current scoped run/destination and minimum across the complete bounded recipient group in one MVCC query. That query is the linearization point. Message publication proves the actual frozen kind/id/address against current verified identity, not only a current user id; retain confirmed-address-rekey/CC races. Outside/absent/retired/unverified readers and new connections are PUBLIC. Internal/in-app-only destinations may retain current content. Preserve current authority→run→execution/job lock order and known receipt/possible marker replay; a prior possible effect remains uncertain/reconciled, never a false safe refusal or new duplicate entry.
- Contract: Typed AppError::KnowledgeRelease has canonical domain workflow KNOWLEDGE_AUDIENCE_BELOW_CONTENT code and StepFailure::fails_run policy. A NEW pre-effect STEP refusal fails the whole run/attention with the exact code, bypassing retries/final-error successors AND token-budget code substitution; unknown prior-effect precedence remains. Agent tool refusals stay centrally continuable tool_error with that same code. Preserve level-only actor/revision HTTP/MCP commands, fresh joint role/level caps and all other connection fields.
- Dependencies/order:02→05→04→06 after accepted03 producer; p04 knowledge observations must compose with the shared raise/release boundaries rather than fork policy.
- Evidence/review: source commit above; p03/docs/REVIEW.md, root-WB1-r2-acceptance.json, source-equivalence bridge; author818nativePASS/all23selectors incl49knowledge, B130PASS, migrations/checksum/SQLx/live/offline/clippy/fmt/source-diffPASS. Full06 still mandatory. Recovered reviewed source retained byte-for-byte in verified archive, not indexed as duplicate Rust.
- Verification: new consumers prove no consumable restricted output without same-transaction raise; no public new effect after high content; current actual fence/commit validity and tenant scope; preserve real competing histories/actions/recipients/connection edits/recovery and continuable tools. Existing p03 tests remain required affected evidence. No backward compatibility.
- Responses: p02 pending; p05 pending; p04 pending; p06 pending. P03 C001/r1SATISFIED at actual reviewed artifact; receiving phase must acknowledge C001/r1+C002/r1 before affected implementation.

## Plan-owned container tooling
Root provisioned Colima profileopenbot-knowledge (2CPUs/6GiB/60GiB) for eventual model-image/final gate. DOCKER_HOST=unix:///Users/mac02/.colima/openbot-knowledge/docker.sock, DOCKER_CONFIG=/Users/mac02/dev/openbot-recovery/.worktrees/docker-knowledge; no default context activation/bootservice/user-config overwrite. Docker server29.5.2/linux/aarch64, CLI29.9.3, Buildx0.38.2 ready. Evidence p03docs/logs/final-gate-container-ready.json. This is owned whole-plan test tooling, no deployment. Preserve unrelated services. Later phase can adopt it explicitly; whole-image checks not run yet.

- Revision8: p03 actual G1r3/WB1R2PASS, F1/F2closed, root exact acceptance and sourceaac35b7 locally mainff integrated; C001/r1SATISFIED and C002/r1ACTIVE for02/05/04/06. Current03 resources remain55503/target until explicit handoff. Plan-owned Docker tooling provisioned independently. Final06/push remain pending. I001–I005/r1 unchanged.

- Revision9: final source-preserving p03 metadata bc516908a075b9c7ea1d39bac94b93b9d916e1a8 also locally mainff integrated, sourceaac35b772d3dc5ae555e00a0077b2f9a60e4925c/accepted tree58145609 unchanged. C002/r1 remainsACTIVE, no technical changes. Next02startup; no knowledgepush yet.


| p02 investigating | root/A Sol6.1/xhigh; fresh Astra/high B pending first completeG1 | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p02 | /Users/mac02/dev/openbot-recovery/.worktrees/knowledge-p02/plan/01-knowledge-access-levels/phases/p02/docs/ |
- Revision10: p02 startup at basebc516908a075b9c7ea1d39bac94b93b9d916e1a8 (accepted/integrated03), branchknowledge/p02, isolated futurePG55502/fresh target; root/A user-confirmedSol61xhigh. C001/r1+C002/r1 and I001–I005/r1 ACKNOWLEDGED for all02A–C contracts, satisfaction pending codechecks/review. Registered responsep02:ACKNOWLEDGED (original C002 publicationpending responses retainedhistorically). P03acceptedresources preserved; plan-ownedcontainerprofile may be adopted byp02. Whole p02 baseline/designinvestigation starts; no02code/decisionPASS yet.

- Revision11: p02 actual KNOW-P02-G1-R1PASS/N1 and rootexactdecision/source/authorityproof accepted. Whole02A–C implementation authorizedunder decision11862cda79fec74a0391d8bd378d3831d1552e418fe35ffb4d6d7702b3027e59. Private55502/freshtargetactive; B /root/knowledge_p02_reviewer idle/reusable forONEwholeWB1, noauthorworkers. I001–I005r1+C001r1+C002r1unchanged/acknowledged; satisfaction pendingcodechecks/review. No02source/codeacceptance/pushyet.


### KNOW-Q001 / r1 — Existing workflow reverse dependency guard (integration debt)
- State: active verification debt; nonblocking for p02's verified affected scope, MUST be closed before whole-plan final06/push.
- Origin: p02 author focused regression audit; root/A sole writer. No implementation contract changed and no source acceptance published.
- Targets: p05 (acknowledge when planning shared ports), p06 (required correction/review/full-check closure).
- Evidence: p02/docs/logs/baseline-reverse-import-proof.json and baseline-reverse-import-guard.log. On immutable accepted basebc516908a075b9c7ea1d39bac94b93b9d916e1a8, unchanged src/application/workflow/http.rs SHAa378db798c1581529cbf521eeb793147e72868c9d2c762ac6cd9790903e11d7f names crate::use_cases::connection_level::ConnectionLevelCommand. Original accepted p03 nativebinary open_bots-7b3508cbfb62bd10 independently fails workflow_and_transport_depend_on_shared_ports_without_reverse_usecase_imports (exit101), same as current expanded p02 regression run174PASS/1FAIL. New p02 SDK-runtime guard passes.
- Required disposition: preserve strict actor/company/expected-revision/level-only connection semantics, place the shared command in the inner consuming port/domain location, and adapt all callers under affected review; do not weaken/skip the boundary assertion in final verification. Final06 whole currentCI gate must include this passing test. P02 leaves the unrelated pre-existing producer source/assertion unchanged and records its failed baseline and explicit focused-scope exclusion; it does not claim the whole boundary suite passes.
- Responses: p02 ACKNOWLEDGED/disposition above; p05/p06 pending startup acknowledgement. C001/r1+C002/r1 and I001–I005/r1 remain unchanged.

- Revision12: p02 author baseline-proof published Q001/r1 verification debt for05/06; no changed library/run/release/ownership contract or code acceptance. Whole p02 source implementation underway, one WB1 still pending; completed real ARM Linux image/model gates and native/browserevidence in p02docs. Final06/fullplanpush remains mandatory.


### KNOW-C003 / r1 — Accepted library, indexing and offline model contracts
- State: active; effective after actual accepted/integrated p02.
- Origin: p02 root/A; independent KNOW-P02-G1-R1/G1-R2 and whole KNOW-P02-WB1-R1/N1 PASS, no actionable findings. Sourcecommit6298ce36844569114bacffa10024c9055627cce3, metadata9156228ccb46c00066a03f75448e896e078a02de locally mainFF integrated. Accepted installed0e336135a0355baa89e20ef75a53a2afe379519e45062f3c65bbb3b205a4c0f2/tree5b976a69098aeeb1c80c81113d36257dd49a258f1b04aa36c48e4493b56b841e. Original records p02/docs, root-WB1-acceptance.json and phase-source-commit.json.
- Targets: p05,p04,p06 and autonomous dependent consumers after wholeknowledge.
- Contract: KnowledgeFileName is the single strict ASCII0–9[_-] parser, basename/path/trim/control checks; KnowledgeDocumentName equality/hash/order and KnowledgeDocumentKey use pinned ICUUnicode16fullcasefold matchingPG18 generatedcasefold/C-collatedkey. Displaycase preserved. No Borrow<str> on insensitivehashname. Layers identify(company,canonicalname,level), uploads sequential/laterduplicatewins/stableid+revisionincrement; allfiles/chunks one atomiccommit after freshauthority+quota recheck. Deleteexpectedrevision prevents stale-removal. Shared authority lock order actoraccountSHARE→companyNOKEYUPDATE→currentmember andOwner9/jointfacts retained. Ordinaryteammembers maywrite up to ownlevel, no contact/operator elevation. Reuse existing helper/currentrole policy, no owner/admin-only broadening.
- Contract: application::knowledge owns required KnowledgeDocuments and synchronous TextEmbedder ports, no safe defaults. Librarylists SQLmetadataonly andfilter<=freshviewer BEFORErows/counts; no hiddennames/counts/quotausage. No fullcontentdownloadendpoint. KnowledgeDocumentLayer names/levels/revisions are visible metadata, not a source for bypassing run-ceiling/search authorization. Human overwriteunknownCOMMIT is conservativeuncertain503/refresh, never proofno-write/retryidempotence. Do not reuse that humanuploadprotocol for retryable workfloweffects.
- Contract: knowledge_documents/chunks tripletenant+doc+levelFK, strict positive revision/hash/textbounds, 384finite/1-D/1-based/L2 vectors, generatedsimple FTS. Pinned model identity intfloat/multilingual-e5-small@614241f622f53c4eeff9890bdc4f31cfecc418b3/fp32/mean/384. Modelchanges requireDBreset, no reindex/dualmodel compatibility.
- Contract: ONE process model already loaded in Foundation and shared through AppState.knowledge/KnowledgeLibrary.embedder(). Future search must reuse that Arc; never load perrun/company/provider or downloadatrequesttime. TextEmbedder handles passage/queryprefixes andcounts actualconfiguredtokenizer withoutpadding/truncation, max512 includingprefix/specials/headingcontext, nativebatch<=4, normalized384outputs. Chunker is pure/token-aware/nooverlap/bodyloss/Markdownfence-aware, boundedheadingpath andchunks. Existingoneindexingpermit/trackedblockingwork/cancellation/registration-vs-shutdownfence holds actualpermits untilnativeworkstops. Actual fastembed nativecall is not interruptible; future querywork needs similarly bounded/trackedblocking ownership, cancellation and no late consumption; C002 actual liveclaim+sameTxoutput/rise+commitvalidity remains authoritative.
- Contract: caps10files/request,1MiBrawUTF8text/file,10MiBstoredcompany,4096layers/company,4096chunks/file,8192chunks/request,heading2048bytes/128actualtokens. Preserve generic privatequotaerrors and boundedreader/rates. No stacklimitraise. Newprotocols retain competing-claimant coverage and earlyCI failure signals.
- Contract: actual KNOWLEDGE_MODEL_DIR default/app/models/multilingual-e5-small. ExactSHA/size model+tokenizer+runtime+licenses checked BEFOREDB/workers, startup60sbound, no fallback/lazy/network. Build/devonly scripts/prepare-knowledge-model.py/staticmanifest; exact upstreamlicenseCRLF is preserved by narrow.gitattributes rule andmustcontinue matchingmanifest. CPUORT1.24.4 Linuxaarch64/x64/macARMbundles, nonroot rootownedreadablemodel. Dedicated knowledge model-check usesproductionloader withoutDB/secrets; operationalcredentials/help behavior preserved. Serve explicitly bounds runtime shutdown1s; supervisedknowledge drain5s.
- Contract: Fly2GiB selected from actualARM Linuxcold/maxbatch peak~1452MiB. REQUIRED CI scripts/check-knowledge-image.py fails above1536MiB underhard2048MiB andverifies profilematch, offline/nonroot/read-only/integrity/Serve-beforeDB/shape/quality/4x512/chunk+querybounds. Keep earlyguard when changingresourcebounds; reduceworkingmemory before anyfurtherincrease. No x86_64 inference/memory claim yet; runtimebyteschecked, currentCImeasuresitsown image. Final06fullcurrentCI stillmandatory.
- Verification: author174lib+5binPASS/10nonzero selectors; independent128lib+5binPASS incl3schema invariants, realbrowser/progress/dedup/stylesboth themes, artifactpreparation andactualimagegate. LiveSQLx/cache/clippy/offline/fmt/sourcechecksPASS. ConfirmedQ001 baseline reverse-importguard remainsDEBT targeted05/06, no fullsuiteclaim. Original .logfailureevidence retained.
- Responses: p02 C001/r1+C002/r1SATISFIED at reviewedartifact; p05/p04/p06pendingstartupacknowledgement ofC003/r1+Q001/r1 as applicable. Do not weaken p03 release or add a mutating knowledge read without its real claim/commit fence.

- Revision13: acceptedp02source6298ce3+metadata9156228 locally mainintegrated, C003/r1publishes acceptedlibrary/modelcontracts; no sourcechangesafterreview. Nextp05 startup. Q001/r1stillactive andmandatory06closure. Wholeknowledgepush deferred until05/04/06/fullcurrentCI; no deploy/resetproduction.
