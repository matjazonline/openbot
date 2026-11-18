# Plan05 p03 — knowledge follows the sealed department scopes

Status: tracing existing behavior/invariants before D3 architecture gate; no source edits.
Workflow implement-astra-reviewed-2-3. Root/A /root user-confirmed Sol6.1/xhigh;
independent B /root/p05_reviewer requested Astra/high, reused after accepted02.
Root owns TASK/HANDOFF/coordination; B owns REVIEW/notes/findings. Additional actual
model/context/compaction telemetry unavailable; retain durable checkpoints.

## Startup identity and authority

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan05-scopes.
Start/base/reviewbase/HEAD5fd8865225125771205f4e0a7a8a59f595e19905 (accepted02metadata).
Clean checkout at03entry; no preserved dirty changes. Accepted implementation
prerequisite2edd0a73cbe76bac2d559c52f8c4c5025402c88b, source
7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5,
independent05SC-P02-B1-R2-N1 PASS and root acceptance; F1/F2/F3 resolved. P01source
0713361f…90dabf/commite6eb5bc8 is included. Main stilld748c2ff; no Plan05 push.

Authoritative coordination /Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md,
root writer, startup revision11. ACK C001–C004/r1 and C005/r2; subject/service contract
is the accepted02 extension, not the original plan's incorrect existing-service
assumption. No03dependency on further service work. Required user checkpoint:
finish/review/full-verify03source, normally push main then finish04docs/evidence and
STOP before06. Doop explicitly separate: no checks/investigation/declaration/assertion
changes; every executed root libtest uses --skip doop --skip Doop.

Exact snapshot exclusions for03: plan/05-scopes/phases/p03/docs and exact
plan/05-scopes/COORDINATION.md. Do not exclude implementation/deliverable docs or
closed02records. Current base/prerequisite differ by metadata only; no runtime delta.

## Original requirements and acceptance/proof plan

Original plan/05-scopes/{README.md,03-knowledge-scopes.md,04-integration-and-final-verification.md}.
No backward compatibility: init schema edits and fresh owned databases, no backfill,
legacy decode, dual behavior or rollout. One cohesive03batch includes schema/search,
upload/move/UI, consumption/egress and host context. Costly schema/auth/transaction/
shared-contract choices require independent D3 PASS before dependent edits.

| Criterion | Required behavior and decisive assertion | Focused checks |
| --- | --- | --- |
| AC1 schema | Unit-owned documents/chunks, tenant-scoped restrict FK, uniqueness by unit/name/layer; denormalized chunk unit cannot disagree; department deletion refuses owned docs | knowledge_scope_schema_ real SQL negatives/roundtrip/fresh migrations |
| AC2 run search | Actual per-actor or base sealed read units filter SQL before ranking with unchanged level ceiling; no/missing/unavailable scope refuses without company fallback; topology changes affect later runs; root/child/native equivalent surfaces | knowledge_scope_search_ actual desk/lead/level/nonunion/missing/actor cases and inherited contexts |
| AC3 manual/upload | Manual viewer scope uses memberships/roles+ancestors+one-step lead, owner/admin all units; upload member/role/parent lead/owner/admin and level limit; no-department ordinary user has no target | knowledge_scope_upload_ authority matrix, valid foreign IDs and no-effects refusal, defaults/multiple unit selection |
| AC4 move/lifecycle | All same-document level layers move together in one authorized transaction; no copy/sideways grant; concurrent replace/move/delete remains coherent; historical proofs keep consumed unit and current egress guards | actual competing move/replace or mutation tests, rollback and denied target assertions |
| AC5 UI/context | Library grouped root-first/filterable by department, departments show owned docs/link/delete guidance, move target list follows authority; host context names knowledge readable units and local memory write | native markup/route/browser protocol tests for actual forms/navigation/feedback and escaped names |
| AC6 integration/publication | Focused phase checks plus one final whole Plan05 source gate before main publication; current claims bounded by evidence and explicit Doop deferral | fmt/whitespace/offline all targets/clippy/fresh migrations/SQLx/reachability/full DB/network/stock stack/lease and CI comparison; authoritative digest/B PASS/remote SHA |

Existing02inventory4808unique test names is the03baseline; baseline file
../p02/docs/logs/B1-R2-final-test-list-r1.txt. Compare actual runner names if tests
move/rename/remove. Preserve all failed/unavailable checks and stand-in limits.

## Initial traced surfaces / unsettled decisions

Trail located application::knowledge service/types/search/visibility and knowledge
persistence::{rows,search,writes}, HTTP routes::knowledge::{mod,upload}, page
knowledge_library. Existing manual scopes carry only company/actor; documents are
currently company/name/layer-owned. Need exhaustive shared-symbol/caller and stored
field/consumer tracing before choosing D3, including SQL consumption readers and
UI/department/proof siblings. The accepted run_scopes snapshot is the authoritative
unit set; do not rebuild live topology for a running workflow or union actor groups.
Manual scope composition, logical document/layer identity on moves, locking and
historical-versus-current consumption semantics still need actual-code decisions.

## Resources and next action

Borrowed PG18 localhost51167 and all prior/shared/Plan06 databases must be preserved.
Proposed owned03DBs pa05_p03_schema/pa05_p03_core_test, external session Cargo/Node/
GNU caches. Do not point new schema/query tests at02databases once init changes.
P02owned pgvector container openbot-plan05-p02-memory-pg (context colima-openbot-plan04,
loopback32770,1GiBshm) may be transferred/reused explicitly for final memory gates;
no global Docker context change. Private02env is external session cache; create a
private03env with only owned DB URLs before checks. No03DB/port/check process yet.

Next trace shared contracts/callers, read exact source spans, record D3 decisions,
verification mapping and consumer dispositions; recheck/publish coordination if an
inherited shared contract changes, then obtain independent D3 architecture PASS.
Do not implement schema/auth/move/search until that gate passes.


## Traced contract impact and actual existing invariants

- KnowledgeDocuments has exactly two implementations (Postgres and application test
  store); callers include library service, six HTTP routes, workflow upload fixtures
  and root_teardown_test_support. KnowledgeScope is actor/company only; keep it as
  identity and use an explicit unit upload target. Layer DTO readers are persistence
  rows, HTTP/API/page/browser fixtures and tests; all must carry owning unit.
- VisibleKnowledgeQuery has one production producer (KnowledgeSearchRuntime), one
  SQL adapter, and SQL fixtures. StoredKnowledgeChunk also feeds the real embedding
  retrieval probe/ranking fixtures. Those equivalent producers need provenance data,
  not a workflow import into the generic ranking layer.
- Search's public result is deliberately only file/heading/text. Existing test
  knowledge_search_result_omits_levels_ids_and_filename_prefixes protects this.
  Keep IDs/classification private. Current consumption stores canonical result and
  highest level, not source items; replay deliberately preserves old content after
  replacement (knowledge_search_replay_uses_the_canonical_result_without_embedding_again).
- Shared frozen_selection is also used by memory checkpoint/save/provision code.
  It resolves the selected agent and base ceiling, and currently only agent.run.
  Do not turn its base authority into knowledge-unit authority or break memory.
  No direct native knowledge port implementation exists; preserve current support,
  exercise genuine native unavailable/read projection helpers and hosted descendants
  where supported, and never claim unsupported model/native dispatch was exercised.
- Root membership witness rows carry root_user_id; ordinary explicit non-root member
  rows do not. Role holders are separate. Root clearance is not an extra direct
  department placement for the no-department upload rule. Owner/admin remain global
  authority, with their actual current level unchanged.
- Quotas are company-wide10MiB/4096layers/8192chunks,10files/request, existing vector,
  token/body/query/deadline bounds. No limit increase or per-unit quota multiplication.
- Existing companies_delete_tree is an AFTER STATEMENT cleanup after ordinary tenant
  cascades; channels/departments/principals company FKs are deferred NO ACTION.
  Thus document company cascades already finish before department deletion. Reuse
  this owner: no speculative new cleanup trigger. Verify company/account teardown
  with unit documents, as well as individual department RESTRICT.
- RunScopeContext already renders memory write scope and searchable unit names in
  one sentence, frozen before history budgeting. Reuse it; no prepared-agent shape
  or format-version change is needed by this design. Current version8 and both SQL
  readers remain aligned. Add actual knowledge-path host-context assertion.
- Current library has no manual search route/form, only list/upload/delete; implement
  the original manual-search requirement using existing query/ranking/model gate.
  Upload parser currently accepts files only; new unit selection must be explicit,
  authorized before embedding and rechecked at the final write.

## D3 / revision1 — proposed architecture gate

All decisions below form one gate; no dependent source/schema changes before B PASS.

### A. Unit owner and logical document identity

Add non-null unit_id to knowledge_documents and knowledge_chunks in init. Document
FK(company_id,unit_id)→departments(company_id,id) ON DELETE RESTRICT; unique layer
becomes(company_id,unit_id,name_key,access_level). Retain(company_id,id,access_level)
and add the exact composite target required for chunk FK(company_id,document_id,
unit_id,access_level)→document tuple, ON UPDATE CASCADE ON DELETE CASCADE. A chunk
cannot disagree with its document's unit/level/tenant. Unit changes cascade from
one owner update, rather than independent dual writes. No compatibility/backfill.

A logical document is the existing name_key group within one unit, with one row per
level; no new group table. Add an explicit typed upload target (identity scope plus
DepartmentId), layer owning-unit metadata, and a move command keyed by visible anchor
layer id, expected anchor revision and target unit. Upload replacements match the
unit/name/layer tuple; company-wide quota deltas subtract only that exact replacement.
No new index without production evidence; required uniqueness/FK indexes only.

### B. One current manual authority projection, separate from frozen runs

Persistence loads current scoped person/root membership/roles under existing
account/company authority prefixes; app policy reuses run_scopes on the person's
explicit placement chains and held lead facts. Multiple manual placements union
the individually resolved sets, enforcing the existing256read-unit budget; no
sideways/grandchild expansion. Exclude root clearance witness rows from placements;
explicit roles still count, including root roles. With no placement, resolver gives
readonly root. No-department ordinary person therefore reads permitted root layers
but has no upload target. Owner/admin read and may upload to every tenant unit up
to their own level; this exception is validated in persistence, never tool arguments.

Writable units = explicit member/role units plus direct children of units where the
person holds lead; ancestors do not become writable. One projection is reused for
list/filter/upload-target preflight/replace/delete/move, so UI choices are not an
independent grant policy. A target is always loaded with its company; valid foreign
unit/document IDs refuse without embedding or writes. Upload and mutation final
transactions reauthorize current grants and levels after parsing/embedding.

Manual owner/admin search uses an adapter-private all-units selection after fresh
authorization; workflow VisibleKnowledgeQuery always names explicit bounded units.
The common SQL applies either the validated manual all selection or unit_id=ANY
beside level/model predicates before candidate limit and ranking. Regular manual
search uses the same resolver-derived explicit set. No client-supplied all flag.

UI unit choices are a bounded metadata projection, not authority: at most256plus
selected/default entries, a visible truncation notice and Departments links for
other units. Owner/admin list/search still covers all authorized documents within
existing company quotas, and direct target authorization is not capped by picker
contents. Ordinary resolved unit sets fail controlled at257; never silently truncate
an authorization set. Defaults use the single explicit placement, otherwise ask;
privileged no-placement defaults root; a selected writable filter may be the target.

### C. Run query and private consumption provenance

Resolve unit read rows from the exact available per-acting-principal snapshot using
the selected manifest agent. The existing base ceiling/frozen_selection helper stays
separate because memory uses it too. No union of other agents' groups, live roster
recomputation, or principal-owned knowledge. Verify current owner identity without
rewriting historical IDs/labels. Missing/unavailable/foreign actor/owner context
becomes knowledge.scope_unavailable before embedding, never a company-wide query.

Add document_id/unit_id provenance to stored chunks. Ranking still returns the
same public file/heading/text schema, with a private consumed-item vector for each
actually retained nonempty passage (document id,chunk id,unit id,level). Highest
classification remains the maximum actually shown; trimmed/omitted candidates do
not become exposed facts. Existing deterministic ranking/UTF8/byte limits stay.
Workflow observation wraps this generic result with the exact queried unit-id set,
including empty successful results, and validates it against the current loaded
snapshot before recording. Generic ranking/offline probe do not manufacture run
permission and retain their existing public result contract.

Consumption adds immutable queried unit_ids and private items beside result/level.
Bound units≤256, items≤8 with existing result byte limits; strict shapes, no nulls,
items' units within queried set and classification consistent with shown passages.
Extend existing deferred live/canonical proof guard to derive the acting principal
from the canonical prepared selection.agent and require the exact actor read set
and live tenant unit owners. It cannot borrow another captured agent's reports.
No live document/chunk/unit FK on historical consumed items: uploads/deletes/moves
must not rewrite or block retained proofs. Existing whole-row immutability covers
new fields; ON CONFLICT replay must preserve exact private proof too.

Preserve existing canonical replay semantics after replacement or movement: already
consumed old facts and their old unit provenance remain historical, while every new
query uses the document's current unit. Replay still checks current agent/tenant/
classification/audience/source/fence and applicable frozen unit authority; deleted
owners or unavailable scope cannot authorize old content. Moving a document does
not lower the consumed watermark or rewrite previous results. Explicit tests
separate fresh-query location changes from historical canonical replay.

For a valid prepared knowledge invocation with absent/unavailable scope, persist
only a canonical constant knowledge.scope_unavailable refusal, zero level, no source
items and no queried units; permit this exact no-data proof in the DB guard. A normal
success/empty-success requires available authority. Never overwrite a previously
completed protected result with a refusal; its unauthorized replay fails closed.
This needs a scoped pending/unavailable state and private observation wrapper, not
a company fallback or an unrecorded model-visible result. Model/native capabilities
that do not exist are not added by this work.

### D. Atomic move, delete and upload races

Move locks through existing company serialization and scoped current authority,
loads the visible anchor/revision and all source unit/name_key layers, validates
write authority on source+target and every layer's level, then updates every layer's
unit/revision atomically. Above-level/unknown targets refuse generically without
higher-layer metadata. Reject any existing same-name target group rather than
silently merging distinct documents, even if levels do not overlap. No copy,
backfill or staged owner transition. Same-target/retry respects optimistic revision.

Replace still targets explicit unit/name/level. Company serialization orders it
with a move: replace-first moves its new content; move-first permits a subsequent
explicit upload at the old unit as a new document identity, without copying or
changing the moved identities. Competing moves cannot split layers or overwrite
a stale revision. Test competing production callers and rollback/no-partial effects.
Individual department deletion is restricted while documents remain; normal company
and account teardown reuse existing after-cascade cleanup. Reparenting changes no
document owner; frozen old runs retain unit IDs, later admissions see new topology.

### E. HTTP, manual search and UI

Keep KnowledgeScope as actor/company identity; add explicit upload target instead
of making unit optional/defaulting old requests to root. Multipart has a required
unit_id UUID field before files. Browser native form emits the required department
control first; API follows the same protocol. Initial company/account admission
precedes multipart work; validate unit target before collecting file bytes, then
existing preflight/final write rechecks. Missing/duplicate/foreign/malformed unit
refuses; no legacy files-only acceptance. Body/reader/file/model bounds remain.

Add bounded authenticated API/UI manual search with optional department filter and
existing query grammar, embedder/ranking, actor/company admission and query deadline.
No network/model work inside DB transactions. Reauthorize current manual visibility
at the actual SQL read after embedding; model output remains file/heading/text.
Use a16KiB form body bound so schema-valid1024four-byte characters fit URL encoding;
existing tokenizer budget remains independently enforced, with boundary tests.

Library groups by department root-first, then logical filename/layer; optional unit
filter and bounded writable upload selector, native progress feedback and restored
failure state. Move uses upload-authorized choices and whole-document action; delete
remains layer-specific with revision fencing. Departments pane lists visible owned
documents, links to filtered library and explains move-before-delete. Escape unit/
file/query/error text and URL/attribute values. Reuse existing native form/busy and
query/ranking helpers. No new SSE/partial-read concurrency protocol or styling change.

### Focused verification map for D3

- Schema: actual owned same-DB tenants, invalid chunk-unit/tenant pairs, same-name
  layers in two units, restrict delete, company/account teardown with documents.
- Runtime: actual scripted Deployment/hosted tool on two desks and per-actor groups;
  local/root/child/lead/level predicates before ranking; exact snapshot after tree
  change; source-unavailable gives controlled refusal/zero embedding/no fallback.
  Missing-table-row corruption may be injected only in an owned fixture with guards
  restored before the actual consumer, clearly distinguished from a valid producer.
- Proof: actual unit/item persistence, empty-result queried set, foreign/other-actor/
  mismatched facts/direct SQL refusal, canonical replay and existing deferred-live
  statement/commit faults and watermark/egress regressions. No orphaned test modules.
- Manual/write: member/role/parent lead/owner/admin/outsider/no-placement/multiple/
  over-level/foreign ID matrix; pre-embed and final reauthorization; move all layers,
  collision, stale/denied/rollback, both production move/replace commit orders.
- UI: real native forms/default/required selection/filter/search/move/feedback;
  grouping/department links/escaping/foreign requests, actual browser protocols.
- Interfaces: all KnowledgeDocuments impls, layer/chunk/query/observation constructors,
  embedding retrieval probe, HTTP siblings, workflow/history/lease/teardown fixtures.
  Public passage schema and prepared format8 intentionally unchanged.
- Final03once: fresh owned init/schema+SQLx, focused updated behavior and all required
  repository/CI source gates before main push; all failures/unavailable checks retained.
  Auditable harness skips only user-deferred Doop execution and reuses demonstrably
  unrelated independent service evidence; no assertions/thresholds/declarations altered.
