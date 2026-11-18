# Plan05 p03 — knowledge follows the sealed department scopes

Status: D3/r1 independently accepted; cohesive B1 implementation and focused-test integration in progress.
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

### Resume checkpoint — D3/r1 assigned

Reconciled HEAD/branch and source-clean state at resume. Coordination13 ACK:
C001–C004/r1, C005/r2, C006/r1 proposed. Retained independent B
/root/p05_reviewer resumed for D3/r1; its immutable TASK and coordination pins are
logs/D3-R1-{TASK,coordination}.md. Source snapshot
5e52c2b5725bf4e40e736e6b40de0f15be7157d3926d154562783f4a29e02ac2,
base/HEAD5fd8865225125771205f4e0a7a8a59f595e19905, exact exclusions unchanged.
No implementation changes or test claims at this architecture gate.

Created fresh owned empty databases pa05_p03_schema and pa05_p03_core_test on
borrowed PG18 localhost51167 without altering other databases. Private environment
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa05-p03-env.sh has only these
root schema/test URLs substituted from the closed02environment; external caches
and the explicitly preserved02memory fixture remain unchanged. No migrations or
tests run yet. Source remains frozen pending independent D3 disposition.


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

Accepted unchanged by independent 05SC-P03-D3-R1-N1 PASS at decision level.
Root reproduced the exact pinned source digest and inspected all saved review
notes/findings before activating C006/r1 at coordination14; ACK C001–C004/r1,
C005/r2 and C006/r1. This is not a behavioral or implementation acceptance.

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


## B1 implementation checkpoint

See current HANDOFF for source inventory, owned resources and active check session.
Core manual/unit/move/search/proof/HTTP/UI source is implemented but not independently
reviewed or behaviorally accepted. Fresh migrations PASS; historical offline library
check r3 PASS; later all-target and focused attempts retained as FAIL diagnostics.
Current knowledge_scope_ r2 run follows corrected imports; no behavioral PASS yet.
No threshold, Doop assertion/declaration, prepared format or memory service change.

Protocol-only existing SearchFixture boundary tests now synthesize source IDs while
loading actual sealed actor units. They prove canonical/checkpoint/classification/
lease behavior, not document retrieval or provenance truth. New actual runtime/SQL
provenance cases remain required; these stand-ins cannot satisfy AC2/AC4 alone.

Last checked coordination14; ACK C001–C004/r1, C005/r2, active C006/r1. Independent
D3/r1 notes/findings retained; no B implementation review assigned yet.


### B1 focused results and consumer audit update

B1-knowledge-scopes-r2: five fixture setup failures (missing required department
slug), retained. R3: six PASS / one FAIL; the remaining move/replace test used the
public anchor it concurrently replaced, correctly receiving stale-revision Conflict.
Correction anchors the unchanged private layer to exercise both VALID commit orders;
no production revision guard was changed. Shared helpers extracted to bounded
sibling scope modules. R4 currently tests the corrected seven plus four real
runtime consumption/refusal/empty/move-replay/cached-provider cases; no PASS yet.

Cached model continuation is an unconfirmed consumer gap: tool replay checks units,
but checkpoint current authority calls selection/history validation without a
visible consumed-knowledge unit check. R4 uses a real prepared/consumed invocation
then owned-database corruption with guards restored before the consumer, requiring
zero provider calls. Preserve the assertion and investigate production behavior if
it fails. This is within D3's existing current-owner replay invariant; no alternative
security decision or extra native capability has been adopted.

Private items additionally have a4096-byte bound implied by eight fixed UUID/level
records. Fresh migration r3 PASS on recreated OWNED pa05_p03_schema; owned core-test
base also reset. Other/shared/prior databases untouched. Question grammar is the
existing1000 characters; the1024 gate prose was a proof-plan typo, not permission to
expand the grammar.16KiB native form decision stays unchanged.

Browser source/fixture now checks required unit field order/selection, grouping,
move/search/busy controls; execution pending. Latest fetch: main remainsd748c2ff.

### Resume verification checkpoint (current)

Coordination15 ACK C001–C004/r1, C005/r2, C006/r1, C007/r1. User now prefers
Astra6.2/high review, explicitly authorizing the prior Astra/high if unavailable.
No source review assigned yet; the old decision-level PASS does not cover source.

- B1-knowledge-scopes-r4 identified a cached-provider consumer gap. Actual resumed
  agent execution must recheck consumed knowledge owners even when no tool callback
  is replayed. The existing agent_preparation::current_on authority prefix now calls
  knowledge::require_consumed_units_on using the existing sealed actor loader.
- The stronger legitimate lifecycle test (move report docs, delete empty owning
  department, resume with historical scope rows retained) reproduced an unexpected
  provider request in B1-deleted-owner-continuation-red-r1.txt. The correction passes
  that same strict zero-exchange test; immutable historical proofs remain unchanged.
- B1-knowledge-scopes-r5: 17 PASS/1 FAIL. All lifecycle and cached consumer tests pass.
  Remaining upload test incorrectly equated transport polling with file collection.
  It now supplies a complete foreign-unit field followed by a permanently pending
  file body and requires timely404, zero embedding and zero stored layers.
  B1-upload-body-r6: 1 PASS. The mechanism proves the actual boundary and permits
  multipart parser lookahead, without changing production behavior.
- Two new actual Deployment tests are being integrated for the original desk/lead/
  ancestor/level/frozen-topology/host-context matrix and per-acting-agent isolation.
  They are not yet compiled/exercised. B1-all-targets-r5 in progress.

The new retained learning explains why missing scope rows alone did not prove the
legitimate owner-deletion case. It must be included in the eventual source artifact.
No Plan05 publication: live remote main confirmed d748c2ff. Doop stays separate.

### Focused proof complete; final gate started

B1-knowledge-all-r7 ran227 selected:224PASS/2FAIL/1ignored. The two FAIL were
new Deployment fixture setup, while the existing retrieval/checkpoint/lease/egress/
history/HTTP/mutation regressions all passed. Failures are retained. Subsequent
r8 compile errors and r9–r13 setup/expectation diagnostics remain visible; no
production guard, assertion or resource threshold was weakened to clear them.

B1-knowledge-scopes-r10:20PASS/1FAIL. The remaining Deployment matrix is now PASS
in B1-deployment-r14 (seven actual runs/14 strict scripted exchanges): different
desks, ancestor/root isolation, level3 excludes actual level5 content, direct-child
lead reads with no grandchild expansion, frozen old topology/labels after reparent,
and a fresh admission reflects the new topology. The test uses a real lower admin
with genuine department-derived desk view grants; owners stay9. Lead entitlement
is an acting-principal fact independent of its current desk, as accepted p02/README4
specifies, so the no-lead desk matrix runs before assigning that role.

B1-deployment-r11 additionally passes actual same-run two-agent isolation: the lead
sees the report document; the other selected agent does not, with different stored
queried-unit vectors and unchanged public file/heading/text passage shape.
B1-knowledge-scopes-r10 also passes the real-observation forgery tests: foreign/subset
queried sets, foreign item units, false levels and passage/item mismatch leave no
partial checkpoint/classification/proof effects. Direct SQL reinsertion with an
incomplete queried set is rejected by the actual deferred live guard; malformed
private JSON, foreign units, duplicate/multidimensional arrays and wrong levels are
rejected by the SQL shape predicate. Fault setup restores the immutable trigger
before the guarded write, then rolls back and proves the real proof unchanged.

Linux supervisor/descendant scope: B1-linux-scope-test PASS, all8categories. Built
owned openbot-plan05-network-scope:20261117 from retained pinned Ubuntu context.

Current source adds one further assertion of the actual combined memory-write/
searchable-units host sentence; it awaits the upcoming full gate. No product source
changed after the earlier focused production passes. Active exec15686 performs
SQLx regeneration on ownedpa05_p03_schema, then the auditable final-check.sh runner.
Active browser focus independently dumps the unchanged production page fixture with
the prior compiled binary and runs the actual native upload/move/search protocol.

Retained final-check.sh copies repository gates, defers exactly the three named
Doop gates underC001, and adds --skip doop --skip Doop to executed root libtests
(including stock stack/lease). Repository scripts/CI/assertions/budgets are unchanged.
Plugin and attribution sources have no diff from accepted Plan04; their independent
checks reuse retained Plan04 evidence. Memory service source has no diff from accepted
p02 (180-test service suite retained); extracted service proof is reused while current
real integration, backup/journey and production image gates execute. The real-memory
wrapper owns a new same-server pgvector base, unique ports, and exact cleanup.
Root all-target/DB/stock gates own a new openbot_check_UUID database. Linux-only proof
is separately retained, never relabeled as exercised locally on macOS.

Coordination15 and applicable notices unchanged/ACK. No independent source review,
acceptance, commit or push yet. Astra6.2/high preferred with explicit previous-model
fallback underC007/r1. Next finish gates, resolve failures with affected checks,
freeze current artifact and evidence, review/accept/publish03 then04docs andSTOP.

Final-gate progress: all early static/protocol gates PASS, retained165manifests
validate, locked offline all-target check PASS, Clippy -Dwarnings PASS, fresh runner
migration PASS. Owned runner DBopenbot_check_72463015231446a4b4d5f6bc2e045d59.
SQLx --check/compiled tests/fullDB/browser/stock/lease/image/live-memory gates pending.
Focused browser1PASS in1.7s. SQLx regeneration PASS, metadata unchanged.

A pre-final snapshot was attempted before independent source review; it is NOT a
review input. The first attempt lacked the private GNU-tool PATH (realpath-m); the
successful pre-final digest40b743fc…4041d includes two disposable Python bytecode
files generated by repository checks. Let the running gate finish, clean only its
untracked bytecode artifacts, then freeze the actual source with the unchanged exact
record exclusions. Do not commit bytecode or broaden exclusions. No B source review
has been assigned and no artifact/source PASS or phase acceptance is claimed.

Coverage audit addition during final root suite: original D3 manual current-read
criterion still needed a real grant-revocation-during-embedding assertion. Added
knowledge_scope_search_manual_rechecks_revoked_grant_after_embedding via actual HTTP
request/library/SQL path. Extends only existing test Model with a bounded one-shot
query gate; production unchanged. Current root suite binary predates this test.
The already-scheduled stock full-suite gate will compile/run the final test source;
rerun final Clippy/all-target check and inventory after this test-only addition.
No full-suite restart is needed absent a material production defect.

## B1 source freeze / independent review R1

Frozen source digest bd0bc89e1a9374f2c259145d6f9859a8af92f93e4bf49070da17d794f7b2828a; start/reviewbase/currentHEAD remains
5fd8865225125771205f4e0a7a8a59f595e19905. Exact exclusions unchanged: this p03/docs
and exact plan/05-scopes/COORDINATION.md. All source is frozen during B review.
Added standard __pycache__/ ignore to the existing generated-build-artifact owner
.gitignore; generated bytecode is neither delivery source nor a review input. The
tracked historical archive bytecode is untouched. This resolves the temporary hold
on B assignment without broadening snapshot exclusions or ignoring source changes.

Whole source implementation is complete. Independent source review may proceed in
parallel with the already running final gate. The new manual-revoke test and stronger
host-sentence assertion remain explicitly unverified until current-source checks run.
B must not issue final source PASS before final evidence is supplied. Initial R1
manifest retains actual red/green/focused evidence and pending required gates; no
active partial log is hashed or labeled PASS. Reviewer derives criteria from original
05README/03/04 and coordination15, not this author summary.

Review inputs: logs/B1-R1-TASK.md and B1-R1-coordination.md pin current author record
and notices C001–C004/r1,C005/r2,C006/r1,C007/r1. Parent prerequisite source/review
identities unchanged. Old inventory has21added/0removed; final inventory must include
new manual-race test and final binary hashes. Reviewer model preference/fallback
will be recorded with the actual assignment handle. No acceptance/commit/push yet.

Independent R1 reviewer ownership update (2026-11-18): user-requested model override
gpt-6.2-astra/high was accepted by collaboration.spawn_agent. New independent B
/root/p05_reviewer_62 owns p03 REVIEW/notes/findings/logs for this source review;
prior /root/p05_reviewer is completed/idle and retains historical D3/p01/p02 entries.
New B started without inherited conversation. Root/A unchanged; additional runtime
telemetry unexposed. C007/r1 satisfied at assignment, fallback unnecessary. Full
assignment logs/B1-R1-assignment.md pins current source and all required constraints.
Source remains FROZEN bd0bc89e…b2828a while root only updates workflow records and
executes checks. Final verification incomplete; no source PASS/acceptance/push.

## Independent R1 findings / cohesive corrections in progress

B /root/p05_reviewer_62 completed whole-source review 05SC-P03-B1-R1-N1 with
F1 high hidden-destination filename disclosure, F2 explicit manual-unit admission
after embedding, F3 omitted257th native destination unreachable, and F4 knowledge
scope-limit suppressing independently authorized department detail. All four are
OPEN and must be corrected together before one recheck. Read B-owned findings for
exact spans and decisive checks. No architecture decision changes are required:
these enforce already gated D3 generic privacy, explicit authority ordering, bounded
reachable picker and independent department surface. Source freeze R1 is now ended
for author corrections; B is completed/idle. No source PASS/acceptance/main push.

Current process ownership: full runner parent PID58771 is SIGSTOPped temporarily
to prevent later gates from advancing during corrections. Its already-started
R1stock child script32064/Cargo32070/libtest32077 is still running on sourceR1.
Let that child finish naturally; do not kill and leak randomly named OwnDatabase
fixtures. The preceding full normal all-target/DB and18native-browser protocols
passed. The R1stock result is historical, not final corrected-source evidence.
This is an implementation pipeline hold, not a user-requested goal pause.

Implement all four plus actual HTTP/native/over-limit regression checks. New bounded
manual-query revocation test was compiled for R1stock and will be exercised there;
its final result is still pending. Planned corrections: generic hidden-only collision
refusal while keeping visible explanation; pre-embed explicit-unit admission plus
unchanged final read reauthorization; native bounded destination pagination retaining
source anchor; card-local controlled knowledge scope refusal preserving roster/desks.
Preserve existing limits, explicit targets and all Doop deferrals.

After correction batch, run focused current checks, SQLx regeneration, final static/
offline/Clippy/reachability/inventory, and current-source full/stock proof (oldR1
stock cannot accept new behavior). Regenerate current browser fixtures/protocols
before resuming parent if it uses the earlier generatedR1markup; then CONT parent
for remaining lease/image/real-memory gates. Preserve root runner owned database
openbot_check_72463015231446a4b4d5f6bc2e045d59 until its eventual cleanup.
Review R2 uses same original exact exclusions and same Astra6.2/high B handle.

### R2 aggregate corrections implemented and focused proof

All four R1 findings are implemented in one cohesive correction batch, without
raising limits or changing D3 authority/schema decisions. B-owned findings remain
open until independent R2 recheck.

- F1 move SQL returns only existence/visibility of a collision: hidden-only returns
  the shared whole-document forbidden response; visible collisions retain the
  explanatory conflict. Actual low-level API and native POST cases prove both,
  with identical saved rows/revisions after each refusal.
- F2 new required KnowledgeDocuments::preflight_search admits current requested
  unit/actor/company before the model. Actual SQL read still reloads authority after
  embedding. Both API/UI foreign and unreadable unit refusals prove zero model calls,
  even when the sole model slot is held. Existing mid-embedding revocation also PASS.
- F3 bounded keyset unit_choices reuses current write authority. A native destination
  page follows a company-scoped writable cursor, selects an omitted unit and returns
  to an unfiltered source anchor. MoveQuery separates return-unit, selected target
  and cursor; no raw UUID input or raised picker bound. The actual route flow reaches
  department257, selects it, moves successfully with revision2 and refuses genuine
  foreign source/target/cursor IDs.
- F4 only the controlled knowledge scope-limit becomes a card-level unavailable
  state. Independently authorized department detail/roster/desks stay rendered;
  other errors propagate. The adapter maps the shared resolver budget to the owned
  knowledge error constant. Actual255/257-child cases prove both limit paths, no
  document exposure and unchanged saved documents.

B1-R2-focused-r1:26PASS,0FAIL in22.05s, all knowledge_scope_ including every new
regression, actual Deployment, cached replay, native/UI and competing mutations.
Afterward only the large new native test's foreign-ID block was extracted behind a
boxed helper, preserving test names and assertions; upcoming current-source static/
full/stock checks will exercise that final test structure. No source frozen forR2 yet.

R1normal:4703PASS/27ignored/85Doop-filtered plusbin6/source_modules8PASS; R1stock
completed naturally4704PASS/27ignored/85Doop-filtered (1884.30s), including the
then-new manual grant revoke case. Those full results predate production corrections
and are baseline evidence, not acceptance of corrected source. Parent58771 remains
STOPped with no active stock child. Root will finish affected/static/schema/browser
checks, resume the remaining parent gates, then obtain current-source whole-suite
stock/all-target proof before acceptance. No Doop work, source reviewPASS or push.

## B1 R2 source freeze and verification continuation

Current source frozen at2a1e2771ddb751adc80b3a6b20a8bbd404a9b71fa7e1bd69604e212b288b19d8;
start/reviewbase/HEAD remain5fd8865225125771205f4e0a7a8a59f595e19905.
Exact original exclusions: plan/05-scopes/phases/p03/docs and exact
plan/05-scopes/COORDINATION.md. Coordination15 rechecked; ACK C001–C004/r1,
C005/r2,C006/r1,C007/r1, no requirement change. Current fmt/whitespace/SQLx
prepare/offline all-target/Clippy and8source-module tests PASS. New R2manifest
retains only completed immutable evidence; pending checks explicitly unexercised.

All-target whole corrected-source gate now exec52059: network-isolated
scripts/stack-budget.sh --offline --all-targets -- --skip doop --skip Doop,
unchanged2048KiB on owned pa05_p03_core_test. This one full current execution
also checks the stack bound and final boxed regression structure. Parent58771
remains held to serialize expensive DB/build gates; CONT after full suite ends.
Native current browser chain exec89384 continues independently. No final source
PASS/acceptance/push; aggregate F1–F4 recheck may proceed while checks complete.

Current native fixture producer1PASS and browser protocol1PASS (1704ms), final
source test inventory4749vs4723:26added/0removed;85Doopnames removed from BOTH
inventories. Logs B1-R2-browser*.txt and B1-R2-test-{inventory,accounting}.*.
Trail graph refreshed; trailcheck reports five lexical conflicts, not semantic
contradictions; exact source spans and original learnings reviewed in retained
B1-R2-trail-disposition.md, rationale must accompany the publication commit.
The tool also reports a400KBdiff cap; independent whole-batch review remains required.

Root resumed owned parent58771 after source corrections/static/current browser checks and independent F1–F4 source resolution. Remaining gates use its separate owned runner database and Docker fixtures; current full suite retains pa05_p03_core_test. Host12cores/load4.4 permits independent fixture work; no shared database reset or source mutation. Budget thresholds remain unchanged; monitor resource contention and retain actual results. Source freeze unchanged.

### Final gate: native artifact download failure, assets remain valid

Current139lease tests and original50%work gate PASS: knowledge.context max23ms
of2000 and knowledge.observe5ms. Current generatedCSS check PASS. Current knowledge
production image/retrieval/missing-and-tampered model/serve tests PASS. Memorybackup
actual restore1PASS. Other parent gates and full corrected-source2MiB suite active.

Memory model preparation --download failed HTTP404 for pinned macOS ONNX1.24.4
archive. IndependentHEAD also404 and release-tagAPI404, retained diagnostic. Both
prepare-model.py and manifest exactly equal origin/main blobs (retained JSONproof),
so this is pre-existing download availability, not a Plan05change. Existing cached
model/runtime files remain available; prepare-model.py --verify PASS checks every
expected file's size and SHA256 with no altered pin, bypass or stub. Fresh native
download remains FAIL and must be reported separately; current real-model/production
checks use those verified assets. Do not claim entire runner/CI green. B must
independently assess this disposition against required evidence at final review.
Logs B1-R2-memory-{model-verify,model-provenance,runtime-url-diagnostic}.* retain proof.
No source changed; frozen2a1e2771…b19d8 remains.

## Final R2 evidence and whole-run failure dispositions for independent review

Current full stock/all-target invocation completed101:4706PASS/2FAIL/27ignored/
85Doopfiltered,1995.49s. All26knowledge_scope_ cases PASS including final boxed
helper. Cargo stopped after libfailure; current bin6/source_modules8PASS separately
at2048KiB. No source, test, deadline or threshold was changed to obtain reruns.

Failed role-writer contention case returned Applied instead of expected Deferred.
Actual test queues projector/rolewriter on a principal then releases both; its
observation establishes queued writers, not that the writer's company lock still
exists at the projector's later NOWAIT. Native cutpoint case hit the unchanged15s
ready() deadline. Both unchanged tests PASS together under serialized isolated
current-source rerun(2PASS24.12s), then related real role/account writer and native
cutpoint/competing-worker families PASS8/3existing native-provider ignored(71.80s).
BaselineR1normal and stock both passed these cases. No deterministic Plan05defect
reproduced, and exact full-run scheduling cause is not proven; retain both original
failures and bounded rerun evidence, do not claim one all-green full suite.

Parent full runner completed1 solely native artifact --download404; all other
non-reused/non-Doop component gates PASS. Current real-memory7tests189s, image85s,
knowledgeimage390s, backup25s/journey13steps28s/lease139tests+50%budget370s PASS.
Cached native --verify PASS; source/script/manifest exactly main. Original full
runner early library/stock results predate R2and are baseline only. Actual inherited
independent evidence reuse identities are retained. CIaudit updated accordingly.

Publication staging changes ONLY snapshot representation:60implementation paths
byte/modeidentical with exact staged path set. Final source digest
62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907; same base/HEAD
5fd8865225125771205f4e0a7a8a59f595e19905 and exact original exclusions.
B1-R2-final.evidence.json pins complete immutable evidence, including FAILresults;
no required check is left unexecuted. B independently assesses scope, cause and
sufficiency before explicit final PASS. Source remains frozen. No acceptance/push yet.

## Root acceptance — source phase complete, publication next

Independent B /root/p05_reviewer_62 returned FINAL PASS05SC-P03-B1-R2-N2 for
62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907. B independently
reproduced prior unstaged source with a HEAD-only temporary index, compared all60
blobs/modes, checked final before/after staged digests/coordination15, independently
verified native cached hashes and complete evidence. F1–F4resolved; no remaining
affected verification gap. Root accepts that bounded complete source scope, retaining
full-run2FAIL with current exact/family green reruns, native fresh-download404 and
Doopdeferral. No whole-run/CIgreen or fresh-native-download claim.

Coordination16 changes status only; identical applicable notice revisions satisfied
for p03, publication C003next and p04documentation obligations remain. Source must
still reproduce exact digest before authorized commit; staged retention follows.

Publication retention gates PASS: all168tracked manifests validated by skill checker and repository CIwrapper with --require-tracked. Staged whitespace PASS. Final source reproduces accepted62d667d9…5907. Main freshly fetched stilld748c2ff and ancestor of localHEAD; no merge needed. Normal authorized source commit/push follows.
