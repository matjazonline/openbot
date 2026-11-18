## Authorized continuation and B1 architecture gate — 06DA-P01-D1/r1

Latest user instruction: "proceed and stop when 05/ph03 is necessary". Implement
and independently accept stable non-knowledge batches; STOP before knowledge
integration and any dependent whole-phase/final-plan acceptance. Coordination r2
pins this scope in logs/coordination-r2.md. No producer work or new publication
permission inferred. No source edits have begun.

### Batch B1: shared predicate and department administration

This is a stable authorization contract and one complete management owner, including
application, persistence and competing callers. Later owner batches consume its
accepted contract. Preserve existing root-only operations until each consumer is
explicitly ported; the old company-management helper is removed only after its
consumer inventory has an explicit disposition. B1 is not acceptance of all p01.

**D1/r1 decisions, invariants and failure behavior**

1. One SQL STABLE function `unit_management_role(user, company, unit)` owns the
   predicate. A null unit selects that company's root. It first proves the target
   department belongs to the requested tenant and the actor is a current person
   with canonical root membership. It returns `owner`, `admin` or
   `lead:<department UUID>`: owner precedence, then root admin, then nearest lead
   on the target's bounded ancestor chain. Root is depth1, maximum8 including root,
   matching MAX_DEPARTMENT_DEPTH and department_shape_problem. Unknown/foreign
   target, missing/retired identity, ordinary member, sibling/descendant lead or
   agent UUID returns no authority. There is no widening of grant/read/level policy.
2. Application `use_cases/administration.rs` owns a small required persistence port
   for this lookup, a typed acting-role result and the `manages_unit`/require helper.
   PostgresPersistence implements it and PostgresDepartments forwards to its existing
   persistence handle. DepartmentPersistence includes this authority port so current
   department use cases can preflight without a new service/container dependency.
   Future company/principal consumer ports can inherit the same authority port when
   ported, keeping one SQL decision instead of local recursive implementations.
   The SQL return is decoded fallibly; infrastructure errors propagate.
3. Department persistence keeps the existing bounded transaction protocol. Lock the
   actor account first, then company FOR NO KEY UPDATE, then current principal/root
   membership, then target department/principals in their established order. Reuse
   company_level_authority::{start,lock_team} to establish current person membership
   without imposing root-management permission. Query the same predicate under the
   company lock before each write; direct persistence entry cannot bypass permission.
   Existing serialized role/membership/reparent writes guarantee revocation either
   follows an authorized old-role write or precedes a refused write. No optimistic
   stale preflight can authorize a commit after revocation.
4. Create a child: manage its parent (None means root). Rename/edit/delete an existing
   department: manage its parent; root itself requires root management. Reparent:
   manage both the old parent and the destination parent, checked while locked.
   Existing cycle/depth, owned-desk/history deletion and root shape guards stay.
   Membership add/remove: manage the department itself. Membership and roles are
   distinct witnesses; removing a membership does not silently clear an independent
   held role. Role assign/clear: manage the parent; all root role changes require
   company ownership. The mirrored owner role remains uneditable and admin remains
   root-only. An ordinary unit lead cannot alter its own lead role or gain authority
   over its parent/sibling through a move or role operation.
5. Preserve the existing special owner-only removal of a person's root membership
   and its complete sorted-account teardown protocol in this batch; widening this
   company-member removal path is outside this initial department contract. Root
   agent membership still uses existing root-manager path. Role refusals and foreign
   targets use NotFound before effects; existing invalid role/root-shape requests
   retain their structural validation. Any later change to root membership removal
   requires explicit original-criteria reconciliation in the owning batch.
6. This batch changes no knowledge/memory scope contract. Its migration adds only
   the shared function to the init schema; fresh schemas only, no backfill. Plan05
   integration must reconcile the init migration from accepted producer source.
   No production/shared database reset and no shared server configuration changes.

**Affected contracts and proof mapping**

- Predicate DB matrix: exact owner/admin/unit/ancestor lead role; sibling/child,
  member, stranger, cross-company target, external/system/agent principal refusal;
  deepest legal eight-level chain and unknown target. The production decision is
  SQL only, avoiding a second Rust policy; Rust decodes role data, never derives it.
- Actual DepartmentUseCases and PostgresDepartments matrix for all mutation kinds,
  including direct persistence calls. Assert descendant work succeeds, sibling and
  own-role escalation are NotFound with zero mutation; reparent out refuses;
  parent-manager changes are allowed; root role changes are owner-only.
- Real competing role-removal versus create/update/member management call, observed
  waiting on the company lock; assert either authorized old-role atomic result or
  refusal, never stale-role effect after demotion. Retain existing department races.
- Run `department_`/`manages_unit_`/`delegated_admin_` focused DB families and HTTP
  authorization families after B1, fresh migrations, SQLx prepare, locked offline
  all-target compilation and source-module reachability. New tests are appended;
  no deleted/moved tests planned. Later route/UI batch adds lead rendering/HTTP cases.
- One full repository gate remains required after integrated complete Plan06. At
  the user-requested dependency stop, run appropriate checkpoint verification and
  explicitly retain whole-plan unverified status; do not claim complete Plan06.

B1 architecture status: pending independent review. Decision revision is immutable
in this section; corrections get a new section/revision. Future definitions require
user clarification: active-binding units (README decision3) versus always recorded
managing department (01B.4). An async question is pending; do not build that dependent
contract without the answer.


