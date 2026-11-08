# AH-P02 — Durable exchanges and accepted-output completion

Workflow implement-astra-reviewed-2-2. Root/A /root user-confirmedSol6.1/xhigh, B /root/channel_reply_review_recovery requestedAstra/high independentoriginalcontext, reuse revieweronly; actualtelemetryunavailable. Skill /private/tmp/openbot-chs-p06-3f43t2k7/workflow-skill-implement-astra-reviewed-2-2. Source/records currentisolatedworktree /private/tmp/openbot-agent-history-h_82_5pw/worktree branchcodex/autonomous-agent-history-h_82_5pw; start/reviewbaseHEAD4ab45555. No newauthors/spawns or historycommit/push/deployauthorization. Originalrequirements 02-durable-exchanges-and-completion.md all02A/B/sharedinvariants; all02–07ordered authorized, targetedonly nofullsuite/offer.

Startup checked authoritative /Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md revision4, writer/root; acknowledge AH-I001–I006/r1, AH-C001/r1, AH-C002/r3 acceptedproducer. AcceptedP01sourcee62ebc6c, independentAH-P01-B1-R1PASS, rootcodeacceptance, 165targetedPASS, F1/F2fixed, owned59archiveSHA4ba0fd3c. Producerretainedpins in setup.json; preservedexternal36exacthashes mustremainunchanged. Priorphase59dirtysource is accepteddependency, not newP02work; isolatephase-localrecordwrites. CurrentGITHEADbaseincludes03only; reviewmustincludeuncommittedproducer+newdelta relativep01archive as wellas4abbase, never claimHEADalonecompletefeature. Sameexact9plan04recordexclusions; no sourcehidden.

Cohesivebatch AH-P02-B1: minimal durableexchange/input/order/provenance/level/checkpointlink schema, queuebeforeproviderentry and recoverablebeforecheckpointcreation, immutableexactlogicalexecution replay, atomicacceptedoutput/nativefinalcheckpoint, guardedfailure/cancel preservingacceptedoutput, monotonicknowledge stamps, nativeindexes/FKs/deletion/pins. Preservecurrentactualactor/currentauthority/fences/tokenusage and parent/modelonly/samplepaths; no expandedpromptwriter/secondengine/callbackcompletion. Detailedtransaction/schema/recovery decisions need one cohesivearchitecturegate beforedependentimplementation. P03turnserialization andP04historypreparation remainlatergates; do not falselyclaimalreadyimplemented.

Checks peroriginal02: listnonzero agent_history_storage_, agent_history_completion_, affected agent_history_level_, realcompetingfinalizers/levelobservations, crash/rollback/lostack/stalefence/repair/cancel/workflowfailure. Traceaffected checkpoint/agentdriver/recovery/usage/callers first. Finalaffectedfocusedgate, freshowndb migrations/actualSQLxprepare/check, lockedofflinealltargets/Clippy/fmt/diff/Graft; nofullruntime/stack/leasegate. Recordallfailed/unavailablechecks. No phase02checks yet.

Resources logs/resources.json: rootownedPG18.6 PID85060 port65037, dedicatednewopenbot_ah_p02(_test) (createbeforechecks), serialized warmedtarget old03worktree/target, explicitURLs andstock2MiB. P01DBpairretained, neverresetotherfixtures. RootnoCargo/DBoperationsactive; B idleafterP01PASS. Next: tracecurrentcheckpoint/input/provider/failure/cancel/payloadlifetimeowners throughGraft, proposeD1schema/transaction/recovery decision againstoriginal02, independentgate beforecode.

## AH-P02-D1/r1 proposed cohesive architecture gate

Original02 and06lifetime criteria readinfull; Graft traced actual prepare_agent (14occurrences/9files), update_workflow_checkpoint(13/6), Rig run278–352/drive75–143/final_output225–235, native checkpoint_updates4–95/open373–406, recovery21–100/195–284/334–376, controls170–241, guard_failure50–106, retention4–130/eligibilitySQL and current native schema. Actual StructuredResponse hasbody/originalcontract; schema bound65536, native checkpoint/prepared2MiB. Canonical source input remainsexecution.frozen_inputs.context, retentioncurrentlydeleteswholeterminalroot90days. Planned source restrictiveFK/retention-skip relationship is deliberate lifetime ownership, not source cascade. Actual failedjobcanmeanreconciliation; do not terminalizehistory fromstatusfailedalone.

Decision logs/AH-P02-D1-r1.md resolves oneexchange/reusedinput source+digest/output atomiccopy, stricttenant/execution/revision/checkpointscope, native monotoniccounter/pending+levelindexes, explicitboundedpreparation lease/current-agent-before-localfence with durablequeue beforecheckpoint/provider, version4server exchange ref, nativeFinalOutputsamecommit anddeferredagreement, genuine runterminal/finalerror hooks preservingcompleted outputs, restrictive source pins + existingretentioneligibilityandfuture06pruning with no permanentFKcycle. P03turnserialization/P04selection remainlatergates. SourceunchangedacceptedP01e62ebc6c, coord4/C002r3 exactpin. Input AH-P02-AG1-R1-input.json and immutabledecisionSHA notedthere. RootfreezesourceforBarchitectureonly; no new02code/checkclaims. Newp02DBpairnotcreatedyet; no rootCargo/DBchecksactive. No unavailablechecks concealed; phase01actualchecksretainedevidence only.

Additional unchanged-source investigation for implementation after gate: task_harness_runs source fields allNONNULL workflow/company/task/agent/execution, existingUNIQUE(company,task,id), prepared/checkpoint bounded2MiB. Source run_dependency_manifests maps actualrun/root/manifest; agent_manifest_revisions mapscompany/manifest/agent/revision. Nativeguard can proveexactselectedprovenance viaexistingrelations; nativecompiledModelStep kind is resolvedinRustfromimmutablebundle (polling.step_kind/agent_manifest.execution_selection), not guessedfromjobpayload or a preparedlabel. Contractschema65536/MAXcontract65792; AgentExecutionOutput.structured stores originalcontract/body/fingerprint and content agreement nativecheckpointconstraint. Existingcurrent_on verifies selectedmanifest/agent/revision/digest/mode beforecurrentgrants; actualmodeTools is producedonlybyagent.run selection, modelonly usesModelOnly. Anynewhistoryrequirement mustkeyactualfrozeneligibility and checkgenuineexactscope, neverforgedlabel.

Failurefacts: recovery decisionReconcile maysetjobfailed while runwaitingreconciliation andsameactivationrecoverable, so no blanketjobfailedhistoryterminalizer. Genuine runterminal failure/cancel and executionfinal_error cover routedfailure. Existingcancel retiresjobs/waits/message intents and thenupdatesrun; nativeguardfailurehascompanyprelock/run/executions/jobsanddescendantorder. Sourcecurrentrunlock serializesclassificationobservations/finalcheckpointstamp. Existingworkflowretentionroots mustterminal90days, locktree/dependents, checkeligibilitySQL, archiveusage, deleteallrunsthenjobs; newrestrictivehistoryreferencesneed eligibilityskipuntilhistoryprunerremoveseligibleterminalrecords, notcascadeorpermanentcycliccheckpointFK. No02editsmade duringarchreview.

AH-P02-AG1-R1 CHANGES REQUIRED onlymediumF1: existing safeoperatorretry can reopen a failedroot on sameexecution/job, contradicting permanenthistoryFailed. RevisedD1/r2 exactSHA4070ba26945e3e675d84f103a9fe3d3b3cc77c6a90514afe5b5254e2bdef819b, retainedr1/diff; distinct auditednativeFailed→Queued operation preservesidentity/order/input/checkpoint/budgets/stamps, completedoutputneverresets, queuedmayretaincheckpoint andmatchingopencommitshistoryreactivation. Explicitlater03oldpendingpositions/exclusivegate/nohighest-completedwatermark and06resumablefaileddeadlinepins/backpressure consequences. Allactualnativecontrol/SQLpredicate/callers verified; B AH-P02-AG1-R2 PASS architectureonly, F1fixedindecision/nootherfindings, sourcee62/currentcoord4exactverifiedbyroot. Receipt logs/AH-P02-architecture-acceptance.json. Publishedprimarycoord5 AH-C002r4 narrowlifecycle refinement +AH-C003r1 durableexchangeownership beforeimplementation; pinlogs/coordination-r5.md. Producer01stillacceptedcode59archive; no02code/runtimeacceptanceyet. Rootsourcefreeze lifted for whole02 implementation.

## First implementation segment (unreviewed)

Implemented initial reset-only exchange schema and native guards: reuse source frozencontext/digest, actualsource/conversation/agent/revision/manifest/checkpoint relationships, boundedprovenance/output/domainlevel, queued/active/completed/failure states, protectednativecounter allocation and indexes, nativecheckpointproof/equality/deferredagreement, runterminal/finalerror/safecontrolreopen hooks, restrictive sourcepins. Existingworkflowretentioneligibility skipsretainedexchange sourcetrees. Resetonlyownedp02pair; migrations-main/test-exchange01PASS. This provesDDLsyntax/freshschema, notfullruntimebehavior.

NativeRust preparation refactor nowunderimplementation: explicitLeasePolicy throughAgentPreparation+14callers, actualruntimepassesitslease; PreparedAgentv4 typedserverhistoryexchange field/with_history, ordinaryterminalstatepermitsunchanged plus can_control_retry stateeligibilitymethod (nativeownerretainsauthority). Added sourceFacts frozeninputs and prelocal/currentlockedselection sharedbyidentity andboundedcombinedpreparation. Newagent_history/preparation.rs+storage.rs capturequeue/prepBoundedWrite/readonlylostack, exactinputdigest/provenance replay, atomicnativeorder. Nativepreparestillresolvesprovidersecret aftercapture; no lateagentlocks. Firstprepare-compile01FAILEDwrongResolvedAgentimport only, fixedviaactualpublicationreexport. Fmt01PASS. Newgenuine storagecapturetest checksoriginalarray/null/float input, durablequeuedbeforecheckpoint, exactpreparation/id/orderreplay andnocounteradvance; listcompilingnow. Checkpointopen/link/finalization/provider-state checks notintegratedyet, so do notrunagentprovider/fullaffected cohort orclaimruntimehistorycomplete. No newcompletiontests/wholephasecodeacceptanceyet.


## AH-P02-B1-R1 — complete implementation frozen for whole-phase code review

The source is frozen at `0c6df19310118c599e38cebf51b47cd4cfcb4e3a37b65c1be901da2a14308a8c`,
HEAD/start/review base `4ab45555`, with the same exact nine record exclusions.
`logs/AH-P02-B1-R1-input.json` pins the full source, coordination revision 5,
accepted P01 producer archive, D1/r2 architecture, original external inputs,
source archive, phase delta, and checks. There are 79 owned source paths across
P01 and P02; this phase changes 32 against accepted P01 plus the HEAD baseline.
Archive SHA256 `77a024f060139459a07e59087eba539bb96c773dee76031c90e12add776c167b`.
All 36 external inputs still match their original bytes/absence. No code edits
while review runs; only phase workflow records may change.

The complete implementation covers original 02A/B through the production preparation,
checkpoint open, provider/execution admission, final output and invalid model
settlement writers, native lifecycle/control retry hooks, and source retention
pins. The actual accepted output is exact structured JSON, including JSON null;
invalid candidates have no accepted answer. Global authority precedes local locks.
A matching reopen commits its queued-to-active transition despite AlreadyApplied
checkpoint status. Native fixtures now prepare their actual child/sibling source;
the retention tests first assert the history pin, then explicitly release that
terminal pin before exercising the unchanged workflow-retention boundary. The
revision-overflow fixture flushes deferred constraints before restoring its trigger.
No production guard or accounting assertion was weakened.

Focused checks: runtime10 passed 227/0 ignored; latest history runtime11 passed
49/0 ignored. Together with 19 unaffected additional retention cases passed in
runtime09, this is 248 distinct passing targeted tests, including 49 history.
`logs/AH-P02-targeted-runtime-evidence.json` contains the exact union and receipts.
Runtime09 retained seven related fixture failures, all corrected and passed in
runtime10; its one ignored browser fixture exporter is unchanged and explicitly
excluded from runtime10 because no browser-authoring behavior changed. Earlier
compile and runtime failures remain recorded. Exhaustion fixture now provides
initial response plus two permitted repair calls. The unsupported cyclic fixture
was replaced by a genuine fresh admission in the same private session.

New-DDL migration pair, actual online SQLx prepare and prepare --check with locked
offline all-target compilation, separate offline all-target check, Clippy all-targets
-D warnings, formatting check, diff check and deterministic Graft refresh all passed.
SQLx metadata has no source delta; dynamic queries were exercised on the real DB.
Commands, environment, timing and outputs are in the pinned per-check receipts.
No full runtime/stack/lease suite and no live-provider claim. Root owns PG65037;
no root Cargo or DB operation is active. B remains independent and is assigned
whole-code review next; architecture PASS is not code acceptance. All remaining
phases 03–07 depend on actual P02 code acceptance.


## AH-P02-B1-R2 — both findings corrected, source frozen for one recheck

B1-R1 CHANGES REQUIRED contained exactly medium F1/F2; root read all new findings
and complete notes. Both are implemented against accepted D1/r2, with no changed
architecture contract. See logs/AH-P02-B1-corrections.md. Actual fresh-schema deletion
protection uses existing run/control ownership and NOWAIT; ambiguous checkpoint open
now reads proof of its entire reactivation effect. Genuine deferred-COMMIT applied/
aborted enqueue/reopen and final unknown-ack provider replay tests passed, along
with retry/delete contention, expiry, nonresumable cleanup and company teardown.
Original competing-finalizer replay test was renamed to describe its actual scope.

Source c24664b28aed5506158d3a2e709ae3c5ec1228567e03fdff06bd15c146a08147; exact same nine exclusions, coordination5 unchanged.
Owned82, full phase delta35, correction
paths10. Full originalR1 archive/inputs remain immutable;
R2 input pins full source, explicit correction diff and originalR1 source archive.
New archiveSHA bfea3a87c7f347549e52ace9857fd1b4084a8780765430c0adab8de42e931c53.

Current affected list/runtime04 PASS238/0ignored, including58 history; history03
separately PASS58/0ignored. Aggregate257 distinct targeted passes with19 explicitly
reused unchanged generic retention cases, recorded in targeted-runtime-evidence-r2.
Fresh owned-pair reset/migrations02, actual SQLxprepare/check-findings02, offline
alltargets, Clippy -Dwarnings, fmt/diff/Graft all PASS. New SQL runtime and actual
commit uncertainty were executed on PostgreSQL18.6, explicit own URLs and stock2MiB.
No full suite or live provider claim. All external36 inputs preserved and the
coordination authority matches its immutable pin. No operation remains active.
Source is FROZEN for B correction recheck; root accepts only after actual B PASS
and exact source/coord/evidence verification. P03 implementation remains pending.


## Phase02 accepted; producer published

AH-P02-B1-R2 PASS, F1/F2 FIXED, root read complete new review/notes/findings and
independently verified exact sourcec24664b2, source/prior/producer archives, all82
owned and36 external hashes, currentcoord5 and every pinned check receipt/log.
Receipt logs/AH-P02-code-acceptance.json.257 targetedPASS including58history; no
full suite/live provider. Source freeze lifted AFTER acceptance. Root published
primary/local coordination revision6, AH-C002/r5 and AH-C003/r2 accepted producer;
immutable pin logs/coordination-r6.md. Originalr5 review pins stay unchanged.
Next phase03 may start from this exact producer, with its own records/resources
and same9exclusions. Do not write phase02 records from later phases. B idle after
PASS; reuse for03 architecture/code gates. No history commit/push authorization.
