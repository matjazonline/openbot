# Plan05 p03 independent review notes

## 05SC-P03-D3-R1-N1

- **Identity:** 2026-11-17; implement-astra-reviewed-2-3; p03 D3/r1 architecture gate;
  independent B `/root/p05_reviewer`, requested Astra/high, further actual-setting
  telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`,
  branch feature/plan05-scopes; phase start/review-base/HEAD
  `5fd8865225125771205f4e0a7a8a59f595e19905`; source digest
  `5e52c2b5725bf4e40e736e6b40de0f15be7157d3926d154562783f4a29e02ac2`.
  Exact exclusions: `plan/05-scopes/phases/p03/docs` and
  `plan/05-scopes/COORDINATION.md`. No implementation delta at this gate.
- **Inputs/authority:** original `plan/05-scopes/{README,03-knowledge-scopes,
  04-integration-and-final-verification}.md`, accepted p02 scope contract/implementation
  2edd0a73 (reviewed source7a555e95…b51a5), and all A–E decisions plus criterion and
  verification mappings in [immutable TASK](logs/D3-R1-TASK.md).
  Authoritative coordination is
  `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`,
  root writer, revision13; C001–C004/r1 and C005/r2 acknowledged, C006/r1 proposed.
  Current file byte-matches [pinned coordination](logs/D3-R1-coordination.md).
  [Start snapshot](logs/D3-R1-reviewer-start-snapshot.txt) and
  [final snapshot](logs/D3-R1-reviewer-final-snapshot.txt) reproduce the artifact.
- **Ownership/lifecycle coverage (A/D):** inspected current document/chunk keys,
  company cascade, revisioned replacement and delete, quota/authority orchestration,
  and the AFTER STATEMENT company-tree cleanup. The composite unit/tenant/level FK
  makes denormalized chunk ownership follow the owner update. Existing name_key
  grouping is sufficient for the specified all-layer move if the proposed transaction
  loads every source layer, rejects target groups and authorizes every layer's level.
  Company `FOR NO KEY UPDATE` serialization in `company_level_authority::lock_team`
  is an existing common prefix for current library writes. D3 explicitly defines both
  replace/move orderings, stale revisions, collision/rollback and teardown cases;
  it does not depend on a speculative second cleanup owner or historical proof FK.
- **Manual authority/HTTP coverage (B/E):** checked current KnowledgeDocuments port,
  access/list/preflight/replace/delete paths, upload body admission and multipart
  parser, knowledge routes and department detail/pane. The proposed one current
  projection keeps level clearance separate from explicit member/role placement and
  direct-parent lead authority. Root witness rows do not silently grant upload;
  owner/admin all-unit authority is adapter-owned. Reusing run_scopes for manual
  placement unions keeps one-step reports and ancestors consistent while preserving
  the fixed 256-unit bound for ordinary scopes. UI picker truncation cannot truncate
  authority, and privileged all-unit queries retain company quotas. Explicit target
  validation before file bytes/model work plus final reauthorization addresses stale
  UI selection. Required field order is a deliberate new protocol, with no old
  files-only default. New manual search reauthorizes at SQL after embedding, keeps
  model work outside the transaction, and uses existing bounded ranking. The 16KiB
  form bound has an explicit encoded-input boundary test obligation. Department
  listings, filters, move form authority, feedback and escaping are mapped to actual
  sibling UI paths rather than assumed from the library page alone.
- **Run/provenance/replay coverage (C):** inspected actual KnowledgeSearchRuntime,
  VisibleKnowledgeQuery/SQL candidate filtering, generic rank/bounded-result behavior,
  `knowledge_checkpoint::{load_call,frozen_selection,observe_on,replay_on,
  record_consumption}`, accepted `run_scopes::{rows_on,context_on}` and both current
  SQL consumption/harness guards. The shared frozen_selection helper is also used by
  memory and currently accepts agent.run; D3 keeps new knowledge-unit authority
  separate and does not claim unsupported native model dispatch. Exact selected
  actor groups must be used, with live unit identity checks and no topology refresh
  or actor union. Public passage file/heading/text stays unchanged; private item
  provenance follows only retained nonempty passages, while the queried set also
  covers empty successful reads. Deferred canonical proof, immutable fields and
  ON CONFLICT replay must preserve those facts. The constant scope-unavailable
  observation has no data, items, units or level and cannot overwrite a completed
  protected result. This explicitly distinguishes absent authority from a successful
  empty query. Historical replay after replacement/move is consistent with the
  current replay test and immutable consumed fact semantics; fresh queries use the
  current owner. Deleted owners/current source or fence failures still refuse replay.
- **Impact/proof mapping:** exhaustive indexed constructor search confirms the
  generic StoredKnowledgeChunk producer siblings include the embedding retrieval
  probe and ranking fixtures; they need provenance fields but cannot supply run
  permission. Actual SQL/persistence, the application test store, HTTP routes and
  teardown fixtures are identified in the plan. D3 maps source-unavailable cases,
  same-DB foreign owner/tuple negatives, actor isolation, SQL-before-ranking,
  empty/private proof shape, direct SQL refusal, canonical replay, both competing
  move/replace orders and current UI protocol behavior. These are suitable required
  assertions, not executed evidence. Source review must check that the unavailable
  branch preserves invocation authorization and the existing deferred/live guard,
  and that every claimed unit/item proof matches the actual retained passages.
  No schema compatibility, broader limits or unrelated service rerun is implied.
- **Checks/limits:** read-only graph/source/requirement inspection and artifact /
  coordination comparison only; private p02 environment used only for snapshot
  tooling. No Rust/SQL/browser test, database write, new resource, Doop investigation,
  publication or source edit. All p03 behavioral and final repository gates remain
  NOT EXERCISED. This review covers the complete recorded architecture, not the full
  future implementation or exhaustive unchanged UI/service source. Final source
  verification remains before the authorized phase03 main checkpoint; p04 final
  documentation and STOP before06 remain mandatory.
- **Outcome:** **PASS AT DECISION LEVEL** for D3/r1. No actionable or optional findings;
  [finding record](agent-review-findings.md#d3-revision-1). C006/r1 can be activated by
  root without changing accepted C005/r2. Implementation/source acceptance pending.
- **Process feedback:** a combined gate was appropriate because schema ownership,
  consumption replay, manual permissions and move transactions share authority
  assumptions. The recorded distinction between root clearance witnesses and actual
  placements, and the explicit no-data unavailable proof, make the proposed behavior
  reviewable. Existing verification-contract guidance is adequate; no new instruction
  is supported by this gate. No prior phase records were changed.

## 05SC-P03-B1-R1-N1

2026-11-18. Independent source review, implement-astra-reviewed-2-3, complete p03 batch 05SC-P03-B1-R1. B /root/p05_reviewer_62 assigned Astra 6.2/high under C007/r1. No independent host effort telemetry. Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan05-scopes, start/review-base/HEAD 5fd8865225125771205f4e0a7a8a59f595e19905. Frozen source bd0bc89e1a9374f2c259145d6f9859a8af92f93e4bf49070da17d794f7b2828a excludes only plan/05-scopes/phases/p03/docs and plan/05-scopes/COORDINATION.md. The base-to-HEAD delta is empty. Reviewed the tracked and untracked p03 changes. Authority /Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md matched [retained revision 15](logs/B1-R1-coordination.md). C001–C004/r1, C005/r2, C006/r1 and C007/r1 acknowledged.

- Read original Plan05 README, 03, 04, the independent D3/r1 decision, repository/subsystem guides and the [criterion mapping](logs/B1-R1-TASK.md). Confirmed the composite tenant/unit/chunk FK, logical-name per-unit ownership, restrict delete, company teardown fixture, SQL level/unit filtering before the row limit, and exact sealed per-agent access. Reviewed ranking, retained items, ceiling and byte bounds, actor/lease/canonical proof enforcement, exact replay and the no-data exception. Followed the separate existing preparation authority for cached model continuation. The retained red owner-deletion case and subsequent zero-request test exercise the real persisted checkpoint. Protocol fault tests with synthetic IDs exercise transaction/fence semantics. Real retrieval/Deployment tests supply independent item/actor proof. Inspected consumer updates in the retrieval probe, reused library and workflow test fixtures, and bytecode ignore/retained learning with lighter review of their mechanical fields.
- Traced current team/level authority, placements versus root witness, visitor/admin paths, manual resolver, upload parser/model timing and final write rechecks, move/replace fencing, grouping, Departments card, move/search native forms and both response surfaces. Checked actual competing moves and deterministic blocked move/replace orders, assertion of preserved identities/content, empty-success vs unavailable scope, historical vs current source movement, and malformed/subset/replay proof cases. The Deployment observes actual model messages for both desks, local/root/child, lead and level, reparent and per-selected-agent cases. The Chromium fixture exercises generated HTML and native wire encoding against a fixture HTTP responder. Production route/DB tests exercise the server handlers separately.
- Independently ran snapshot, coordination comparison, git diff --check and the manifest retention validator. Inspected the retained [evidence manifest](logs/B1-R1.evidence.json), [test discovery](logs/B1-R1-test-accounting.json), focused red/green logs, deployment-r11/r14, knowledge-scopes-r10, knowledge-all-r7, native browser and Linux network scope. Manifest has 7 PASS, 4 FAIL and 2 NOT EXERCISED. Some red cases are superseded by narrower exercised cases. The initial eligible inventory shows 21 additions and zero removals. Final-source inventory, new manual-revocation assertion, strengthened host sentence and completing final-gate log were pending. No duplicate full suite or Doop checks run by B. Root executes the final gates independently.
- **CHANGES REQUIRED.** Four implementation findings in the [findings record](agent-review-findings.md#b1-source-round-1), including a hidden filename disclosure and three medium defects. Correct the aggregate, freeze the new digest, and recheck the affected paths and final evidence. The cohesive batch was reviewable. The evidence table was helpful in distinguishing the checked company case from the unchecked unit case and bounded option counts from omitted-destination reachability. Future boundary tests should cover the first omitted item and the surrounding component's independent permissions.

## 05SC-P03-B1-R2-N1

2026-11-18. Independent aggregate correction review by B /root/p05_reviewer_62, assigned Astra 6.2/high, implement-astra-reviewed-2-3. Phase p03 B1/R2, feature/plan05-scopes, /Users/mac02/orca/workspaces/openbot/plan03-04. Start/review-base/current HEAD 5fd8865225125771205f4e0a7a8a59f595e19905. Frozen implementation digest 2a1e2771ddb751adc80b3a6b20a8bbd404a9b71fa7e1bd69604e212b288b19d8. Exclusions remain plan/05-scopes/phases/p03/docs and exact plan/05-scopes/COORDINATION.md. Compared the live coordination file with [pinned revision 15](logs/B1-R2-coordination.md). C001–C004/r1, C005/r2, C006/r1 and C007/r1 remain applicable. Reviewed original README/03/04, D3/r1 and the four R1 findings. The [author evidence](logs/B1-R2-TASK.md) records current checks and separate final gates.

Traced both required port methods across the Postgres implementation, test adapter, manual search and move views. Checked the SQL classification aggregate, per-unit preflight and read-time permission recheck, cursor scoping and ordering, anchor/selection independence, native rendered links/form status and error mapping in the Department caller. The change to the shared memory resolver extracts its existing literal unchanged. Reused R1 coverage of the unchanged storage/retrieval/workflow/lease design. [Four finding dispositions](agent-review-findings.md#b1-r2-resolution-history-for-f1f4-2026-11-18) · [Original review](REVIEW.md)

Inspected the production paths and decisive fixtures behind the [26 passing focused tests](logs/B1-R2-focused-r1.txt), including the actual Deployment host sentence, revocation mid-embedding, genuine controlled scope overflow and first omitted destination. The native destination test follows links through the router and posts a URL-encoded form. The separate Chromium test exercises generated markup and native form behavior with a fixture responder. The current browser log reports one passing test. The focused run predates only the boxed test helper extraction. Reviewed the exact retained code for that extraction. B independently ran the retention validator on the [R2 manifest](logs/B1-R2.evidence.json), verified 9 declared PASS and 2 NOT EXERCISED, reproduced the source digest, compared coordination and checked patch whitespace. Per assignment B performed no DB or full suite execution.

**AWAITING EVIDENCE.** F1–F4 are resolved on the reviewed source. No additional actionable issues found in the correction delta. The final immutable manifest must cover the corrected network-isolated all-target/stock-stack run, final test discovery, applicable browser, lease, image, real memory and other required gates before the whole batch receives an independent PASS. Earlier full runs precede this source correction. This aggregate recheck was appropriately sized. No new instruction change is supported beyond the boundary-fixture lesson recorded in R1.

R2 evidence continuation, same source and date. Inspected [corrected binary inventory](logs/B1-R2-test-accounting.json), 4749 eligible names versus 4723 at p02, 26 added and zero removed with identical deferral filters, and successful [fixture production](logs/B1-R2-browser-fixtures.txt) and [Chromium native protocol](logs/B1-R2-browser.txt). The [Trail build/check disposition](logs/B1-R2-trail-disposition.md) covers five lexical matches to unrelated subsystems and records the automated 400KB scan cap. The schema edits that those matches identify were reviewed directly in R1 and remain unchanged through R2. No source finding arises from that advisory check. Final corrected-source all-target results and remaining final integration evidence are still pending.

## 05SC-P03-B1-R2-N2

Final evidence continuation of the same R2 correction review, 2026-11-18, implement-astra-reviewed-2-3. Independent B /root/p05_reviewer_62, assigned Astra 6.2/high, phase p03 in /Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan05-scopes. Start/review-base/HEAD 5fd8865225125771205f4e0a7a8a59f595e19905. Staged source digest **62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907**. Exact exclusions are plan/05-scopes/phases/p03/docs and plan/05-scopes/COORDINATION.md. Matched the authoritative plan/05-scopes/COORDINATION.md against the [final revision-15 pin](logs/B1-R2-final-coordination.md), with C001–C004/r1, C005/r2, C006/r1 and C007/r1. D3/r1 and the original criteria remain applicable. [Final mapping](logs/B1-R2-final-TASK.md) · [Evidence manifest](logs/B1-R2-final.evidence.json) · [R2 correction review](#05sc-p03-b1-r2-n1)

- **Source identity.** B compared the SHA-256 of each of 60 working files and its actual indexed Git blob with the captured pre-staging fingerprint, checked executable bits and checked the exact set of indexed paths. Independently creating a temporary HEAD-only Git index and running the original snapshot against the current working files exactly reproduced the complete earlier R2 snapshot, including digest 2a1e2771ddb751adc80b3a6b20a8bbd404a9b71fa7e1bd69604e212b288b19d8. The normal index reproduced the staged digest. Git's staged whitespace check passed. These checks establish that the previously examined source, corrections and assertions are byte-identical.
- **Scope criteria.** All 26 knowledge scope regressions passed in the corrected-source full run at 2048 KiB, corroborating the focused run. This covers revised F1–F4 fixtures at final source, including the extracted native-ID test helper. Schema owner/FK/negative cases, move/replacement contention, manual/agent unit authorization, real Deployment passage/context and separate acting agents, deleted-owner replay, classification and egress are covered as recorded in R1 and R2-N1. Current formatted source, SQLx prepare, fresh migrated schema checksum, locked offline all-target compilation, Clippy, graph refresh and reachability were exercised. The corrected test inventory accounts for all 26 added names with zero removals.
- **Full-suite result.** The network-isolated stock-stack command ran the entire eligible library inventory with 4,706 passes, two failures, 27 ignored and 85 user-deferred cases filtered. It stopped before running other targets. The separate current-source binary and source-module targets passed all 6 and 8 cases at the same stack setting. The two failures passed in the exact contemporaneous serial rerun, then passed again within real role/account writer and native recovery/competing-worker families (8 passes and 3 pre-existing provider ignores). Serializing the harness preserves the competing actors, Postgres locks, peer restart and worker interleavings inside those tests. [Full run](logs/B1-R2-final-stock-all-targets.txt) · [Exact rerun](logs/B1-R2-full-failures-focused-r2.txt) · [Related families](logs/B1-R2-failure-families-r1.txt)
- **Failure disposition.** The role test observes two sessions waiting on the fixture's principal, releases that principal and expects the notification projector to encounter the role writer's company lock later. The projector's principal FOR SHARE and the role writer's FOR KEY SHARE are compatible after release. Observing the initial waits does not establish that the company is still locked when the projector reaches its separate NOWAIT. An Applied result alone therefore does not establish a broken exclusion guarantee. The native test's failure was a 15-second wait for its local-agent call to become ready, before the outcome/receipt/zero-or-one-native-call assertions. The exact reruns exercised all eight cut points and those assertions. Neither failure was a knowledge operation or a stack overflow. Both tests also passed under both earlier full schedules. The original two failures remain recorded. Their precise aggregate scheduling causes remain unknown. The independently exercised affected assertions and current repeated boundary tests are sufficient for the p03 acceptance criteria.
- **Other final gates.** Inspected the final-check summary and component output. The lease suite passed 139 cases under unchanged budgets, including knowledge context 23 ms and observation 5 ms maxima against 2000 ms budgets. The current browser fixture, Chromium protocol, generated CSS, knowledge production retrieval and rejection of missing/tampered artifacts, memory backup/restore, 13-step stand-in-provider journey, seven real OpenBot/memory integration cases, and memory production image checks passed. Verified the independent reused source comparisons and execution log SHA-256 values for the plugin, attribution and accepted p02 service contracts. The [CI audit](logs/B1-R2-CI-audit.md) records the explicit Doop deferral. The initial normal/stock library results inside the parent log serve only as earlier baseline evidence.
- **Inherited macOS download failure.** The parent verification runner exited 1. Its only failing component was the GitHub 404 on fresh download of the pinned ONNX runtime. The native script unconditionally re-downloads that archive, including with valid cached files. B checked the unchanged script/manifest blob IDs against origin/main and independently ran the manifest's size/SHA-256 verification successfully on the actual host artifacts. The upstream 404 and aggregate failure remain recorded. The local pinned assets were available and the independent native/service/image execution supplied the required affected runtime evidence. Fresh acquisition on this macOS host remains broken. [Diagnostic](logs/B1-R2-memory-runtime-url-diagnostic.txt) · [Parent output](logs/B1-final-check-r1.txt)
- **Outcome.** **PASS** for the complete p03 source on staged digest 62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907, with the failed aggregate runs and fresh-download limitation above. F1–F4 are resolved. The final manifest validated with 26 PASS, 3 FAIL and zero NOT EXERCISED entries. The three failures represent the full stock invocation, parent runner and that runner's fresh-download component. No additional blocking findings. Root owns acceptance, tracked evidence retention and publication. The final check did not require another DB or service run by B. No additional instruction change is supported.
