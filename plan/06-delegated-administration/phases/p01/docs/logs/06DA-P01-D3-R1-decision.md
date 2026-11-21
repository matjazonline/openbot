# 06DA-P01-D3/r1 — workflow definitions and agents

Original Plan06 README decisions2–3,01B.4–5. User resolved conflict: bound definitions
are managed by managers of ANY active binding unit; recorded creating department
applies only when there are no active bindings. No Plan05 integration. Implement
only after B2 acceptance and this independent architecture gate.01B.6knowledge
is the next mandatory dependency checkpoint; do not start02or final acceptance.

## Ownership and authority

1. Extend existing ManagementTarget with Definition(WorkflowId). Existing shared
AdministrationPersistence and SQL predicate own the decision. WorkflowOperation's
ManageDefinition and CopyTemplate carry their workflow identity; their lifecycle
and template-copy callers pass that identity. Only company association may use this operation. Authenticated
person/current canonical membership remains required, never an agent. No new
service/container abstraction or API JSON field.

2. workflow_definitions gains non-null managing_department_id with tenant-composite
FK to departments and immutable ownership after creation. An insert trigger supplies
the root only when no explicit managing unit is given, matching existing root-default
ownership patterns for trusted native writes; it is initialization, never a backfill
or a compatibility read. General save/template-copy commands explicitly choose the
creator's sole non-root current membership/role department when exactly one exists;
otherwise root. Persist that choice once, not a live creator-role derivation.
No optional chosen-unit JSON field is added because the original plan explicitly
preserves API shapes. A future explicit internal target can supply the column;
there is no new public selector in this batch. Existing trusted native `_on` callers
retain their purpose-specific authorization and default root ownership. Fresh owned
schema only. Tenant-mismatched/nonexistent ownership and later reassignment fail SQL.

3. Existing definition: find active workflow_bindings through its CURRENT
configuration_revision/version to the workflow; stale revision bindings and inactive
bindings contribute nothing. Resolve each current company/desk/principal association
using the established ManagementTarget semantics, and accept the existing unit
predicate for any one of them. A company binding means root; a desk/thread means
its current department; an agent inbox uses existing any-placement agent authority;
a person inbox means root. With zero active bindings, check the recorded department.
With active bindings none managed, REFUSE even if creator/root-of-an-unrelated-unit
would have authority through the recorded department. Company owners/root admins
still manage the actual units normally. Missing workflow during creation uses the
same chosen creator department as insertion; missing publish/edit fails the existing
draft/revision checks. All public save/copy/publish/archive boundaries recheck while
holding the existing account-before-company writer protocol. Binding writers, moves,
roles and retirement already serialize on company; thus binding deactivation or
role removal wins before a refused edit, or follows a complete authorized edit.
Native broker/owned agent definition visibility and runtime fences are unchanged.

4. Binding configure/activate/pause continues to check its actual current desk/unit
independently of definition edit authority. A manager of one active binding cannot
bind in another unit it does not manage. Tenant/association/run-as/resource/level
and revision/idempotency guards stay. Preserve current NotFound transport behavior
for foreign/sibling target refusals; the intended outside-managed-unit diagnostic
belongs inside the refusal path and must not reveal a foreign target. Publishing
changes no binding automatically, so it does not retroactively require management
of every existing binding (the user's ANY-unit decision governs publication).

5. Agents reuse B1's existing ManagementTarget::Agent and LockedAgentPublication
for draft, tools/skills, publish, revoke and authoring reads. Application agent
preflights use participant AdministrationPersistence for a target agent. Create and
library copy require management of placement's department, root when absent;
role placement still requires the parent's authority. CompanyAgentService performs
the locked unit recheck before insertion/publication in its existing transaction.
Existing inherited model selection/classification/grant/native direct-address
validation remains. Root credentials/configuration/level changes stay root-only.
Agent authoring collection entry may use AnyUnit but every entry is filtered by
Agent authority with bounded page scanning/cursor progression (no unbounded scan);
target entry rechecks its actual agent. Builder tools consume the
same scoped service/actor, never a trusted-agent bypass or actor inferred from data.

6. Narrow existing raw AgentPersistence writes by following its existing cfg(test)
create pattern: raw update/delete become test-only fixture operations; production
library mutations get explicitly library-only methods that prove company_id IS NULL.
Existing public scoped AgentDraftPersistence gains delete_company_agent(scope),
which checks the actual current Agent target before existing retirement locks/effects.
Application delete uses this checked command. No release-build unchecked company
mutator remains. Test-only raw lifecycle fixtures remain as such; new authority
regressions call actual production public commands. Update all mocks explicitly.
AgentDeletion still retires the principal, preserves history and requires deferred
FK commit success atomically. No change to memory/knowledge owner lifecycle.

## Required proof mapping

- Definition create/save/validate/publish/archive actual service and direct prepared
persistence positives for owning unit/ancestor; sibling/member/foreign/agent denies
with no revision/event writes. Record department round-trip and immutable/tenant FK
negatives on a fresh schema; root/multiple-department creation fallback.
- Two active bindings in independent units: EACH unit's manager edits/publishes;
creating-department manager alone cannot; inactive/old revisions confer no authority;
after final deactivation only recorded department applies. Binding outside either
manager's actual subtree refuses while leaving current binding unchanged.
- Actual role-removal and binding-deactivation competing with definition save/publish
in both orders: exact revision/events or refusal, never stale-preflight commit.
- Agent create/copy/edit/tools/skills/publish/delete succeed in managed placement;
existing multi-placement ANY-unit, empty-placement root fallback, sibling/foreign/
ordinary member/agent direct boundary refusals; library-only mutation cannot touch
company rows. Real builder/provisioning actor paths retain identity/level guards.
- Actual agent authority revocation competes with draft write/deletion under company
serialization; zero half state. Preserve inherited publication/native-direct/runtime
checks, especially grant/resource/tenant failures, and owner/admin application tests.
- Locked offline all-target build/clippy, focused DB/route/service checks, fresh
migration, SQLx prepare, source reachability, discovery/renames, format/whitespace,
trail refresh/check; one frozen whole-B3review. Complete final gates remain after
accepted05/ph03and remaining Plan06scope, never claimed at this checkpoint.
