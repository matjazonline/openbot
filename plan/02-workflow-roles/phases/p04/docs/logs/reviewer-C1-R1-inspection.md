# 02WR-P04-C1-R1 independent producer/ownership inspection

2026-11-12; independent B. Clarification only. Frozen partial source d6afad07875439fc5bdf12f3e5ceac1fb7c396e0fe9a214a19e070b231528038. No implementation acceptance or execution.

Read original phase04, current TASK/HANDOFF, accepted D1.3 and retained D2/D3 obligations, complete C1 decision and current/pinned coordination. Independently used graft exhaustive constructor/reference/caller queries and exact source reads, then raw source-wide inventory to cover unindexed partial files (retained reviewer-C1-R1-refusal-inventory.txt).

Actual preparation path:
- service.rs45–85: admissible trigger, current authorization port, admission_binding selection/revision check, identity verify, bounded_snapshots, PreparedAdmission::new, then WorkflowAdmission::admit.
- service.rs168–178 validates company/binding identity;203–250 resolves input/params under bounds and validates input schema, with BadRequest mapping. No live holder lookup or typed role-refusal construction.
- contracts.rs143–183 constructs causality/first step and copies prepared snapshots; errors map BadRequest/Internal, no role resolution.
- admission_binding.rs94–144 reads current or saved binding under read-only repeatable-read selection. It does not resolve live role holders or emit a typed role refusal before PreparedAdmission; saved refusal binding selection is selection/replay context, not a new refusal source.

Constructor/caller challenge:
- Sole non-test application RoleAdmissionRefusal constructor is roles.rs33–52 refuse_unresolved_sample_role, which emits contextless MissingDepartment. Its exhaustive production callers are sample/dispatch.rs15–73,221–264 and sample/history.rs31–71, all scripted sample execution. They use sample frames/activated sample steps and do not own a real admission claim/source. The fourth caller is the preparation/sample unit assertion.
- Message admission classify451–483 and inbound worker classify consume existing AppError::WorkflowRole. Schedule functions carry the typed refusal returned from ports; they do not manufacture a live role selection refusal in preparation.
- Pure roles::resolve_role returns RoleRefusalKind; application callers outside its definition are tests. The live constructor path is persistence agent_manifest/roles::role_error, reached from actual capture; reviewer_resolution.rs20–73 reads an existing run/execution and admission0 manifest, so is post-admission runtime, not this claimed earlier producer.
- agent_manifest::capture_on54–113 takes &PreparedAdmission. Its sole qualified source call is admission_write::create7–65, before insert_run/persist manifest/jobs/admission. admission.rs11–73 owns actual authority/association/source/claim/current-binding checks and catches its role failure inside that persistence admission transaction. Replayed refusal is likewise emitted by that prepared-admission owner. These observations establish location, not correctness/completeness of the new partial escalation implementation.
- role_binding.rs5–49 produces authoring/activation eligibility BadRequest for a missing department. It is not a missing-holder execution with authenticated admission source and must not mint cases.

Inspected exact existing roles_tests.rs152–192 assertions: for agent.run, decision.agent, ai.classify prepare_step_inputs succeeds, retains role=lead and lacks agent; separate sample guard is asserted MissingDepartment with exact step/role. This is useful preparation/sample evidence, not an actual admission or terminal-trigger test. Test was not rerun and author's passing log was not treated as proof of production coverage.

Conclusion: no legitimate current production application-preparation RoleAdmissionRefusal producer before PreparedAdmission was found. The C1 not-applicable disposition is justified only for that absent consumer. It does not remove prepared-admission role refusal, runtime step preparation/failure, terminal/authored-handler/child, message/schedule/manual source, replay, lease or authorization obligations. Scripted samples/authoring cannot acquire a durable case merely to satisfy an absent-path checklist. Re-inventory at p05 and plan03; any introduced earlier real producer requires the original purpose-specific fenced atomic owner before enablement.

Checks: static source/caller inspection, evidence retained-hash verification, source snapshots and both coordination pins. No Cargo/native/SQLx/DB/browser jobs or source/fixture mutations. Shared native binary is known original-HEAD baseline and was not used. C1 evidence status remains static PASS / whole B1 NOT EXERCISED. No actionable/optional findings; current partial feature still requires whole B1 review.
