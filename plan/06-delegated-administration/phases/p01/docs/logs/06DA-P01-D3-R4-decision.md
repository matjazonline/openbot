# 06DA-P01-D3/r4 — identity-bearing creation, collisions and copy replay

Correction of06DA-P01-D3-F1from D3/r3. Keep the accepted D3/r2architecture and the
r3CompanyPersistence proposal unchanged: existing required AdministrationPersistence
supertrait, existing handle/constructor, explicit root-only fixture semantics. R3's
identity-free typed realization is REJECTED and is superseded by this revision.
No source edits since gate artifactca53630f…c022264; implementation proof pending.

1. Creation permission ALWAYS carries destination WorkflowId. Internal
ManagementTarget::DefinitionCreation(WorkflowId), CreateDefinition(WorkflowId) and
CopyTemplate(WorkflowId) retain it through application, public write/capture and
wrapper boundaries. Existing-only ManageDefinition(WorkflowId) still refuses a
missing row. There is no identity-free creation/copy permission.

2. DefinitionCreation selects authority while holding the established current
person/company lock: if that SAME company-qualified workflow already exists, ask
its current Definition authority (ANY current active binding unit, otherwise recorded
creating unit). Only actual absence selects the actual creating department's unit
predicate. Therefore expected=None on an existing unmanaged row refuses with NotFound
BEFORE draft/revision/unique-conflict or template lookup. Existing authorized creators
may still receive the ordinary conflict. A failed/stale role or inactive binding
never becomes creation authority for an existing target. No collision changes its
recorded department or adopts an existing definition.

3. The public definition save and copy persistence boundaries repeat this exact
identity-bearing decision before insert/PK conflict, under actor/company serialization.
Untrusted or stale prepared commands cannot bypass it. Capture wrappers use the
same Creation(existing-or-absent) target, avoiding separate copied existence policy;
existing editor operations never use this fallback. Public creation inserts persist
the explicit creating department only for genuinely new rows. Trusted native `_on`
constructors retain their separate reviewed caller contract/root default.

4. Trace and fix equivalent ingress/wrapper consumers. In particular HTTP
copy_template_draft currently handles Conflict by raw-reading the derived workflow
and returning its revision. That fallback MUST reauthorize the actual existing
Definition destination before any read/returned revision, using the same current
management predicate. It cannot treat successful preflight, possession of a command
key, template identity, or a generic root/AnyUnit check as replay authority. Unknown
or foreign targets retain the same refusal shape. A key replay after revocation or
binding ownership change is refused; an authorized identical copy replay remains
idempotent and does not append another event. Revision conflict is evaluated only
after the applicable current target permission, also in all save/publish/validate/
archive/capture wrappers. Broad root preflights in public management handlers must
not block a legitimate scoped manager before these exact checks. Lists/UI broader
migration still belongs to02, conversation/history policy is unchanged.

5. Required enduring proof, through REAL services/adapters/HTTP handlers:
   - Lead A can create a fresh definition/copy in A. Existing definition managed by
     sibling B: A's save(expectedNone) and copy collision/reused-key attempt return
     NotFound, not Conflict or a current revision/body. Ordinary/missing/foreign
     existing edits and authorized stale/conflict controls remain distinct and valid.
   - Create/copy legitimately, then revoke A's authority (or move management to active
     bindings solely in B while A still manages the recorded creating department).
     Retry the known identical command key and destination through the real HTTP
     copy handler and actual service. Refuse before returning existing metadata;
     assert unchanged revision, ownership, binding state and event counts.
   - Direct prepared save/copy command after the same change is refused at the locked
     persistence boundary. Include competing revocation/binding transition versus
     actual creation/replay writer where relevant to the existing D3race matrix.
   - A currently authorized existing-definition manager receives the ordinary create
     collision, and legitimate same-command template replay returns the same identity/
     revision without duplicate copy events. This positive control prevents solving
     the security gap by disabling idempotency or all scoped copies.
   - Capture an existing unmanaged draft with forged creation intent: no snapshots;
     capture a truly new authorized draft still works. Source/tenant/level/resource
     and native identity fences are retained, not replaced by target management.

B must recheck this decision/proof correction by F1 before dependent source edits.
After PASS implement all actionable corrections and the complete B3batch, then one
whole implementation review. This is not implementation acceptance, complete p01,
Plan05 integration or full-plan verification. Stop at knowledge's05/ph03dependency.
