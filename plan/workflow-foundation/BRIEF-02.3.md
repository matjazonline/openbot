# 02.3 — Operator templates and company copies

Expansion only, Astra/medium `/root/astra_02_3`; verified `CODEX_THREAD_ID`
`01a0e413-5469-7de1-8246-56493f6c65ac`. Original phase02 Publication and binding
item 1 and its relevant acceptance remain authoritative. No blocking product decision.
Preserve accepted uncommitted 02.1/02.2; parent owns PROGRESS/RESUME. Implementation
is Astra/low under the user's override; independent review is Astra/medium.

## Deliverable and boundaries

Implement an application-owned, bounded template catalogue and an authorized operation
that creates an independent company draft through a narrow persistence port. An operator
offers starter source; a company owner/admin copies it into that company's namespace.
The copy owns its source and metadata immediately. Subsequent template replacement,
withdrawal or catalogue reconstruction cannot change it. Origin is informational provenance,
never a live reference from which to resolve company content.

Choose **trusted composition-time catalogue construction** now: operator-supplied entries
are supplied by application assembly through checked constructors, with an immutable
read-only catalogue after construction. This is real bounded reference-data behavior, not
a pretend mutable production database. A newer supplied catalogue models operator updates.
No remote fetching, package installs, upgrade negotiation, propagation, operator HTTP
editor, server wiring or invented configuration key. Document that distribution is not yet
wired. Do not import HTTP `require_operator` or infra config into application code.
The trusted builder is not an authenticated user entry point; future operator ingress
must enforce the existing account/config operator policy before invoking it.

Company draft persistence gets a required trait and explicit test implementation only.
No production in-memory draft store, SQL, migrations, endpoint or fabricated persisted
success. Phase03 supplies normalized storage. The copy orchestration is useful now and
feeds item5's later general save/validate/publish/archive commands; do not implement those
commands early merely to test copying. Publication, bindings, dependency freezing and
execution remain subsequent points. A copied draft is neither `OwnedVersion` nor admitted
executable content.

## Contracts and reuse

- Add a compact module such as `application/workflow/templates.rs` (split contracts/tests
  when useful), exported from workflow `mod.rs`; add domain IDs/revisions in
  `domain/workflow/ids.rs` or a focused authoring module. Reuse `CompanyId`, `WorkflowId`,
  `VersionId`, `WorkflowActor`, and current owner/admin membership policy. New concepts
  need distinct `TemplateId`, `TemplateRevision`, `DraftRevision` and checked content
  identity where appropriate. `RunRevision` is run CAS state, not a draft revision.
- Suggested concepts: `WorkflowTemplate`, `TemplateCatalogue`, `TemplateOrigin`,
  `CompanyWorkflowDraft`, `CopyTemplateRequest`, `PreparedTemplateCopy`,
  `WorkflowDraftCopies`, and `TemplateCopyService`. Keep authoritative fields private,
  provide read-only accessors, and require checked construction. Avoid cloneable mutable
  shared source or externally forgeable prepared writes. Generic title/description text
  does not need gratuitous wrappers; identifiers do.
- Entries contain template ID/revision, bounded title/description, exact bounded v1 source
  and source/validation identity. Validate offered source with existing decoder plus
  `registry::compile` and supplied bounded `CatalogueFacts`/dependency facts. No alternate
  YAML parser, binding vocabulary, schema engine or hardcoded mock handler. Supplied facts
  validate an illustrative template; they do not establish tenant grants or become a
  published dependency bundle. Retain only what the starter/provenance actually needs.
  Revision and source hash must identify the selected snapshot; hash is not authorization.
- Bound individual metadata, entry count and total catalogue source/metadata bytes **before**
  allocating/compiling every entry; reject duplicate template IDs/revision ambiguity.
  A simple deterministic bounded list is enough; do not add a pagination framework.
  Existing 256 KiB YAML and compiler/fact budgets stay in force. Fail with actionable
  located compiler diagnostics; do not stringify away `Diagnostic` fields.
- Each copy receives a fresh workflow ID and initial draft revision; its company comes
  from the authorized request, never the template. Source `workflow_id` must agree with
  that new owned ID. Implement one compiler-owned source rebasing helper using the parsed
  `/workflow_id` location (or an equally precise checked representation), then re-decode
  and check the result. Never global string-replace a UUID: literals, comments, child pins
  and other IDs must remain untouched. Preserve source formatting outside that scalar.
  Reject nil/colliding IDs at the proper boundary, including accidental template-root reuse.
- Copy source and descriptive metadata as owned values; origin records the selected template
  ID, revision and original identity. Do not copy company IDs, grants, credentials, live
  references to mutable template objects, or operator ownership. Child pins/resource slots
  remain source declarations; copying never installs children or binds foreign resources.
  Rebased source is an **unpublished draft**: item5 must compile it against company-resolved
  facts and item2/3 must freeze/authorize dependencies and resources. Do not carry the original
  compiled hash as though it were the rebased draft's compiled identity.

## Application and port behavior

Authorize every copy for the destination company using current membership. Extend the
existing `WorkflowOperation` with a precise authoring/copy operation and use
`RelatedAssociation::Company`; do not copy/paste the membership implementation. Existing
`LifecycleAuthorizer` already uses `PrincipalAccessPersistence` and
`manages_company_operations()`. Operator catalogue privileges confer no company privilege.
Authentication/authorization precedes template content lookup and write preparation;
infrastructure failures propagate. Missing/inaccessible company state uses existing
non-disclosing `NotFound` semantics.

Require the selected template revision in copy requests so a stale selection cannot silently
copy different content. A catalogue read returns one immutable matching snapshot. Since
this catalogue is immutable during an operation, no cross-store template lock/CAS is needed.
The persistence port must atomically insert the whole owned draft, never upsert/overwrite an
existing workflow ID. Its prepared input includes company, actor, owned source/metadata and
origin; document required current membership verification at the durable commit boundary
for phase03. Return an explicit conflict for destination collision. Do not add silently
successful defaults. If readback is needed, keep it company-scoped and verify returned
company/workflow identity before exposing content. A read API is optional here, not grounds
for inventing an entire authoring repository.

General expected-revision edits/idempotent publication belong to item5. Do not introduce
a template-update concurrency protocol when immutable catalogue replacement suffices. If
implementation does introduce CAS or concurrent insert semantics, add competing-claimant
tests with actual shared state, not two sequential canned responses.

## Existing code anchors

- `domain/workflow/ids.rs:60–96`: existing checked names and UUID newtypes.
- `application/workflow/authorization.rs:14–30,64–94,143–185`: actor, operation port,
  membership/association checks. Indexed callers found in `service.rs:51–119` and
  tests `tests.rs:384–400`; preserve admission/cancellation behavior.
- `application/workflow/contracts.rs:58–63,189`: `OwnedVersion` and run-only revision.
- `compiler/wire.rs:14–19,292–370`: typed parse, source identity and scalar locations;
  `compiler/compile.rs:47–61,73–93`: private compiled source/hash/graph contract.
- `registry/mod.rs:66–87`: bounded real catalogue compilation; `registry/facts.rs:6–31`
  supplies explicit illustrative contracts. This **step registry** is separate from the
  workflow-template catalogue; avoid naming/documentation that conflates them.
- `docs/workflow-language-v1.md`: current compiler boundaries; amend truthfully with new
  library copy contract and remaining lack of production persistence/API.

## Acceptance and verification

1. Offer template v1, copy to company A, construct edited v2, copy again: the original
   A draft retains exact rebased source, metadata and origin; the new copy sees v2.
   Withdrawal also cannot affect existing snapshots. Draft metadata mutation (when modeled)
   cannot alter catalogue content. Two companies get distinct workflow IDs and independent
   source ownership; source ID equals each draft ID, while template source remains unchanged.
2. Real authorizer tests cover owner/admin success, member/outsider rejection, operator-only
   outsider rejection, company isolation and membership reader errors. Assert rejected calls
   never write. A selected revision mismatch/missing template and destination collision fail
   without overwriting. Exercise readback ownership mismatch if a reader is introduced.
3. Rebase tests include the template UUID inside literals and child pins, quoted scalars,
   source comments and byte-boundary conditions. Only root identity changes; resulting
   YAML remains bounded/typed. Copying a template with company-specific declarations never
   grants their access or bypasses later compilation/publication validation.
4. Malformed/unknown types/invalid graph or schema offered templates retain located compiler
   errors. Catalogue tests reject duplicate identities, oversized metadata/source, too many
   entries and excessive aggregate bytes before expensive work. Keep existing typed/null,
   references, dependency recursion and registry suites as regression coverage.
5. Run focused/new plus all `workflow` tests at stock 2 MiB; `cargo fmt --check`, offline
   all-target check and Clippy (`-D warnings`), and `git diff --check`. No DB/provider tests
   are needed without changed adapters/SQL. No bounds may be raised to clear failures.
   Refresh graft after implementation; report tests, touched files and explicit deferred gates.

The phase acceptance concerning immutable **published dependency bundles** and unauthorized
**resource bindings** is only partially relevant here: prove template-copy independence and
absence of implicit grants now; do not claim later publication/binding guarantees complete.
No cleanup of prior work, commit/deploy/reset, abandoned adapter or compatibility scaffold.

## Independent review — accepted

Reviewer Astra/medium `/root/astra_02_3`, verified thread
`01a0e413-5469-7de1-8246-56493f6c65ac`. Inspected actual new/untracked source,
integration changes, tests and documentation. No material correctness findings.

- Catalogue construction preflights borrowed entry count, metadata/source sizes, aggregate
  bytes and duplicate IDs before decoding or retaining entries. Real bounded registry
  compilation validates offered source; retained source identity and compiler validation
  identity have separate types/meaning. Facts are discarded, never granted or installed.
- Company copy uses the existing real membership authorizer before lookup/preparation.
  Owner/admin, outsider/member/operator-only denial, foreign-company denial and reader
  errors are covered. The required insert port states atomic ownership recheck and collision
  semantics; its test implementation uses shared state and competing calls, with revocation
  and no-overwrite assertions. There is no production fake store or endpoint.
- Draft source/metadata/origin are private owned snapshots, with independent company/workflow
  identity and revision. Update/withdrawal tests prove existing copies unchanged. Rebase uses
  the parsed root scalar span and rechecks the bounded result; tests preserve quoted/plain/
  block scalar forms, comments, literal UUIDs and child pins, and cover exact byte limits.
  Drafts carry neither an inherited compiled identity nor executable/publication authority.
- Accepted 02.1/02.2 mechanisms remain in place: decoder limits and structured diagnostics,
  typed compiler/bindings, registry contracts and source/compiled identity distinctions.
  Authorization change is an added copy operation, preserving admission/cancellation policy.

Verified recorded 119/119 workflow tests in `/private/tmp/workflow-02-3-workflow.log`,
offline all-target check in `-check.log` and Clippy in `-clippy.log` (same prefix).
Parent verified the 2 MiB test environment, command options, fmt and refreshed graft;
independently passed `git diff --check`. Reused accepted 02.1/02.2 review evidence rather
than repeating unaffected compiler review or database/provider work.

**02.3 is accepted; combined completed scope is 02.1–02.3 only.** Immutable published
bundles, binding validation, authoring commands and the rest of phase02 remain outstanding.
User stop boundary is immediately before 02.4 (Publication and binding item 2): save state
and stop, without beginning that point. No implementation or parent progress files changed
during this review.
