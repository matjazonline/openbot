# 02WR-P02-D1-R1 inspection

Architecture and verification-plan review only; no p02 implementation, database mutation, compilation or test execution. Current accepted p01 baseline is 91741e066ae90284ca17ba5163cbc89561eef3ac1fb728daec3645392d1f473d at HEAD103004a8ac2fd643cf14bde7e09022ae79f72053.

Original README, 02-admission-resolution-and-snapshots.md, 03-reviewers-and-ui.md and 04-escalation-and-reassignment.md assessed against D1/r1 and proposed C002/r1. Root/src/application/persistence/http/pages instructions and skill review/acceptance references remain applicable from prior reviews; reviewer-assignment template additionally read this round.

Source spans and contracts independently inspected:
- admission.rs:11-61,207-255: company/association authority, fenced claim, replay before capture, active binding verification and atomic transaction.
- admission_write.rs:7-65: closure capture precedes run, manifest, first job, history and replay command persistence.
- agent_manifest/mod.rs:21-137 and capture.rs:4-82: root dependency closure, resource checks, role edge insertion point, agent revision/current grants, cycle/depth/selection/deadline constraints. Existing capture distinguishes workflow versions and different Agent use modes.
- agent_manifest/storage.rs:50-149: manifest ownership, exact immutable workflow capture, execution step mode/use checks and selected revision lookup. D1's centralized role selection must preserve these checks and projection.
- publication/mod.rs:100-124: p01 runtime guard carried to this phase.
- agent_authority.rs:15-91: live definition checks take agent FOR SHARE, classify post-dispatch revocation carefully, and retain model-only versus tools behavior.
- knowledge_checkpoint.rs:198-256: independent literal-agent selection requiring p02 integration. execution_selection_on caller graph includes history preparation, action authority/reconciliation, agent origins and workflow tools.
- run_views.rs:648-682 and board_reads.rs:42-84: literal-agent metadata and pinned-version agent filter; D1 correctly identifies both needing role-selected equivalents.
- message_admission.rs:444-473 and schedule_admission.rs:188-237: errors otherwise become generic definite refusals or transient retries; typed detail must reach persisted message/schedule/manual surfaces without increasing reason code size.
- agent.rs:437-524: deletion takes company KEY SHARE, then agent UPDATE, then principal demotion; published revision FK has its own explicit Conflict mapping.
- init_schema.sql:12298-12314: published agent revision retains its agent before any run exists.
- init_schema.sql:12367-12410,12574-12631,12659-12682: root manifest ownership, exact workflow/revision membership, immutable-fact guard and deferred membership completeness checks. Existing manifest immutability includes completeness against canonical content, not just update/delete rejection.
- init_schema.sql:15516-15561: role holder kind protection and principal demotion's role/grant removal followed by channel settings UPDATE.

Commands: graft skeleton and exact-range reads; graft callers execution_selection_on/selected_on/lock_agent_delete_on/finish_agent_delete_on --depth all; graft grep agent_reference and BindingOutcome::Refused; unindexed SQL/docs rg after graph discovery. All observations are static inspection. No behavior is labeled tested.

Findings focus on two costly decisions and their proof: lock order versus existing deletion/demotion, complete immutable resolution membership versus permitted later rows, and a competing test that must not attribute existing revision-retention protection to the new role capture. Other proposed resolver, shared closure, version/step identity, runtime consumer, typed-refusal and view decisions are consistent with inspected requirements; this is not approval of unimplemented details.
