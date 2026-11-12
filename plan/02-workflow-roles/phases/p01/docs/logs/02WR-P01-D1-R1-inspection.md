# 02WR-P01-D1-R1 inspection evidence

Architecture review only; no Rust/database behavior executed. HEAD and review base: 103004a8ac2fd643cf14bde7e09022ae79f72053.

Original criteria read from plan/02-workflow-roles/README.md and 01-role-references-and-publication.md. Read 03-reviewers-and-ui.md to confirm the agent picker/preview is explicitly later scope. Applicable root, src, application, persistence, HTTP and pages AGENTS.md read.

Source evidence independently inspected:
- src/domain/entities/department.rs:47-94: fixed role enum, serialization, parsing and single-holder classification.
- migrations/20260817000000_init_schema.sql:15438-15455: tenant composite foreign keys, role check, lead-only partial unique index.
- src/adapters/persistence/department.rs:584-601,606-675: lead-only provisioning and role-aware multi-holder insertion; existing membership/grant behavior must survive adding Reviewer.
- src/application/workflow/registry/mod.rs:318-392: descriptor resolution and source-derived specialized facts.
- src/application/workflow/compiler/compile.rs:678-742: compiled representation and identity assembly.
- src/application/workflow/publication/capabilities.rs:98-153: finite/literal pinned-agent universe and use modes.
- src/application/workflow/publication/mod.rs:113-198: freeze and identity/manifest pipeline; child hashes included.
- src/application/workflow/publication/validation.rs:90-119: exact expected agent universe/use validation.
- src/application/workflow/publication/storage.rs:179-207: source recompilation and compiled/manifest/hash equality on restore.
- src/adapters/persistence/workflow/bindings.rs:41-137: company/association authorization, lifecycle validation, deactivation bypass, authoritative stored bundle equality.
- src/adapters/persistence/workflow/authority.rs:45-81: company FOR NO KEY UPDATE lock before authorization-dependent mutation.
- src/adapters/persistence/workflow/association.rs:6-63: company-scoped channel FOR SHARE, thread association and access checks.
- src/adapters/persistence/department_desks.rs:78-144,350-404: company-before-channel ownership lock and alternate ownership setter.
- src/adapters/persistence/channel_edit.rs:102-126: main settings path takes ownership locks before channel gate and write.
- src/adapters/persistence/channel.rs:482-541: identified second ownership UPDATE via exhaustive indexed search; integration must cover both writers.
- src/application/workflow/compiler/agent_selection.rs:13-60 and src/adapters/http/pages/workflow_authoring/preview.rs:79-117: agent-only authoring consumers, explicitly assigned p03 by original plan/TASK.

Structural commands used: graft map; graft ask; graft skeleton; graft callers save_binding_on --depth all; graft callers set_desk_ownership_on --depth all; graft callers prelock_ownership_on --depth all; graft grep 'department_id ='; graft grep DepartmentRole; graft grep ModelStep. SQL migration is unindexed and located with rg after graft grep department_roles_role_check.

Caller trace confirmed all production binding writers converge on save_binding_on: ordinary save_binding, provisioning_native::provision_binding_on, provisioning_department::follow_department_lead_on and repoint_on. Ownership changes converge through prelock_ownership_on from update_channel_on and set_desk_ownership_on; creation callers also use that lock. The planned transaction boundary can therefore serialize activation versus ownership removal without a new lock order.

D1 verification mapping inspected, not executed: AC1 source selectors/identity; AC2 zero/mixed agent capture, metadata round-trip and forgery rejection; AC3 active binding eligibility and competing activation/detachment; AC4 schema and multiple reviewers with singleton lead; AC5 help/sample. Tests, offline compilation and migration remain NOT EXERCISED for this design round.
