# 06DA-P01-D3/r2 — workflow definitions and agents

Original Plan06 README decisions2–3,01B.4–5. User resolved conflict: bound definitions
are managed by managers of ANY active binding unit; recorded creating department
applies only when there are no active bindings. No Plan05 integration. Implement
only after B2 acceptance and this independent architecture gate.01B.6knowledge
is the next mandatory dependency checkpoint; do not start02or final acceptance.

Supersedes unreviewed r1. The following construction-order and requester obligations
are part of this revision, not a later implementation exception.

## Ownership and authority

1. Extend existing ManagementTarget with Definition(WorkflowId). Existing shared
AdministrationPersistence and SQL predicate own the decision. WorkflowOperation's
ManageDefinition and CopyTemplate carry their workflow identity; their lifecycle
and template-copy callers pass that identity. Only company association may use this operation. Authenticated
person/current canonical membership remains required, never an agent. No new
service/container abstraction or API JSON field.

2. workflow_definitions gains non-null managing_department_id with tenant-composite
FK to departments and immutable ownership after creation. The FK is deferred NO
ACTION to permit whole-company cascade teardown while preventing deletion of a
still-owned unit; verify both actual production lifecycle paths. An insert trigger supplies
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

## Creation ordering and real Builder consumers (r2)

Current company_agent::create_company_agent_on publishes before placement, and
publish_initial_on checks current ManagementTarget::Agent. Simply changing an
outer root preflight cannot work for a lead creating a new agent. Preserve that
existing target guard rather than introducing an authority bypass/proof mode.

- Before initial publication, insert the new agent's ACTUAL intended membership
row under the account/company lock, after proving management of the chosen unit
and role-parent eligibility. No role is assigned and no grant/reviewer refresh or
desk gate is taken in this preparation. The agent is genuinely a member of its
final unit inside this same transaction, so existing publication and native direct
workflow binding target guards can see its proper owner without a root exemption.
Role assignment/grants/reviewers/department lead response are finalized once all
agent/direct-workflow and other creation is complete. Any error rolls back the
whole transaction/savepoint, including that early membership.
- Reuse/extract department member validation and insert at the responsible layer;
do not duplicate role or grant derivation. Existing full place_in_department_on
remains the finalizer with its one ordered refresh. The creation request explicitly
distinguishes unplaced (root), placement completed in creation, and placement that
the calling transaction completes later, using an enum with placement payloads,
not bool flags or a new configuration option. The three actual callers are
CompanyAgentService::create_on, onboarding::Session::insert/place and
provisioning_builder::builder_on. Account for every caller: Builder supplies its
resolved placement during creation and still finalizes last; public create/copy
finalizes as part of creation; onboarding retains its owner/root semantics and
later placement, unless an existing explicit placement is available at insertion.
No release path may commit a prepared-only placement.
- Existing desk gates stay the LAST locks in each creation path. New early member
rows are not a grant sync and cannot take those gates. Include an actual competing
construction/ingress or equivalent ordered-lock regression and rollback test that
proves membership, agent, native workflow and grants commit together or all vanish.

Builder's real tools are provisioning kinds CreateAgent/ListAgents/CreateDeskForAgent;
the older create_agent_channel literal is absent from current source. Current
provisioning.rs::authorize_requester imposes root authority before kind dispatch.
Retain requester_on's root-run/manual/schedule/authenticated application-message
provenance and reject agent/external/system requesters. Entry may ask AnyUnit but
per-kind effects authorize their ACTUAL targets: CreateAgent resolved placement
(root when none); ListAgents scoped each returned agent; CreateDeskForAgent actual
new desk's unit (currently root-default with no department selector, so root-only
unless an existing request already names another unit). AssistantChannelService
keeps the same target rule. Do not infer a unit from an unrelated owned agent to
create a root desk with a lead. Preserve provisioning action/source/lease and
resource/classification fences; no email-to-person requester shortcut.

list_on retains existing output-byte/count bounds, tenant/address context, and
current model validation. Add requester to this internal call and filter through
existing Agent management predicate before returning agent configuration. Bound
scans and make truncation explicit. Authoring directory filtering also preserves
bounded pagination and cursor progression even when a scanned page has no managed
entries. Ordinary metadata/conversation read callers keep their existing policy.

Required actual proof additions: public CompanyAgentService create and copy as lead
without root; production Builder CreateAgent and ListAgents with real authenticated
run/requester evidence; forbidden sibling/foreign/agent and external-email requester;
new actor-specific placement rollback and gate-order competition; root-default
CreateDeskForAgent refusal for a non-root lead. The prior r1 proof matrix still
applies, including direct scoped deletion and role/binding revocation races.
