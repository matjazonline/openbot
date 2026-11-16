# Attribution p05 persistent independent review notes

## AS-P05-G1-N1 — 2026-11-16 — Architecture gate AS-P05-G1

Workflow implement-astra-reviewed-2-3, p05 cohesive05A–05B architecture.
[TASK](TASK.md), [REVIEW](REVIEW.md),
[AS-P05-D1/r1](logs/AS-P05-D1-r1.md).
Reviewer `/root/attribution_reviewer`, Astra/high requested, retained independent B;
actual model/effort/context telemetry unavailable. Author `/root`, user-confirmed
Sol6.1/xhigh. No implementation/design authorship or subagents by B.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service; phase start/review-base/current HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
Decision SHA256
`c65c8f5653783df03d83a871adaf8ecb163e1f44fd1a07880df93523b49529b5`.
Source remains accepted p04
`a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`,
root AS-P04-ACCEPT-R1 and B AS-P04-B1-N2. Exact six exclusions are listed in
REVIEW; start/end snapshots and identity output are retained in logs.

Authoritative coordination:
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision9. AS-C001/r1, AS-C002/r1 and accepted-setup AS-C003/r1
acknowledged. Proposed AS-C004/r1 is included in the reviewed decision; its
downstream consumers still require accepted p05 implementation. Pinned
`logs/AS-P05-D1-coordination-r9.md` matched the authority byte-for-byte at review end.

### Original requirements and consequential decisions

Read original05, phase01's recorded API-first/performance/deployment dispositions,
the complete immutable decision and TASK AC1–AC9/consumer maps, and applicable
repository/service instructions. Independently checked the accepted service README
retry/authority/redirect/history rules and OpenAPI campaign/ad/link/GET/HEAD schemas,
security alternatives, fields, bounds and success responses. The design is not the
requirements source. All nine decision items were assessed; no major section was
accepted solely from the author's summary.

- Scope: separate campaign/ad/link/click identities, random144-bit placement code,
  company/store/parent composite constraints and checked link revision fit05A.1–2.
  Code length24 matches the accepted pattern. Existing IDs keep nonnil wire behavior
  when moved to the common local module; no Openbot source/schema ownership expands.
- Authorization: campaign/ad session-only and link session-or-key surfaces match
  OpenAPI. Read/ManageLinks and links:read/links:write remain separate from setup
  integration permission. Reuse of user_transaction and begin_merchant preserves
  tenant/session/key lock ordering and final local state checks. Merchant scope is
  fixed by the key, including optional list filters; public code grants no management
  authority. Full-route pre-body checks are required when the real link routes exist.
- Receipts: one extracted owner with user/merchant principal kind avoids cross-class
  UUID collision without dual paths. Caller-provided authorized transaction and
  typed principal must carry current authority before replay. Normalized payload,
  original precise UTC time, horizon/retention, secret-free key replay and atomic
  mutation/audit/receipt remain inherited p04 obligations. AC2/AC9 explicitly retain
  old consumers and adversarial cross-principal tests.
- History: shared tenant locking orders all relevant edits and clicks; immutable
  snapshots include campaign/ad names, product, placement, destination, link revision
  and scoped IDs. Current-dimension edits cannot rewrite prior evidence. Ad PATCH
  currently accepts only DimensionUpdate(name,enabled), so proposed AdUpdate is a
  bounded extension needed to exercise original05A.4 via production API/UI. Nullable
  product-clear versus omission must remain distinct through parse/fingerprint/SQL;
  no new product ownership is introduced. AS-C004 records this impact for later
  checkout/ledger/report readers.
- URL policy: reuse p04 exact live domain/store approval, reject decoded reserved
  ob_click keys, guard canonical serialized size, preserve unrelated duplicate query
  parameters/fragments, append the server click ID with the URL serializer and
  validate Location before commit. No destination fetch or user-controlled resolver
  is introduced. Domain eligibility is checked again at each redirect.
- Redirect: preliminary lookup supplies scope only; freshness and locked complete
  eligibility precede inserting the DB-timestamped unique click. Only successful
  commit acknowledgement permits302. Explicit HEAD204/no Location/no row matches
  the wire contract and prevents Axum's implicit GET handler from writing clicks.
  Separate GETs remain separate raw events; no delivery claim or IP deduplication.
- Failure semantics: storage rejection and actual unknown commit outcome return503
  without Location. The proposed wire proxy observes successful PostgreSQL COMMIT
  and suppresses its acknowledgement; independent DB observation must establish
  the retained row. A different GET produces another ID. Disconnect is scheduled
  only after observed commit, followed by independent durable-row inspection. These
  planned cases distinguish postcommit transport loss from a precommit error; a
  mocked error or a rejected SQL statement alone cannot satisfy AC6. Cancellation
  does not promise deletion of already committed rows.
- Privacy/admission: bounded heuristic booleans/version only, no retained raw
  IP/UA/referrer and no trust in forwarded client headers. Public16 and tenant8
  admission combine with existing64 connections/32 global requests/10s and DB
  deadlines; no raised existing bound or unbounded click-worker queue. Actual
  saturation/release and competing disable/redirect tests remain required.
- UI/performance: existing setup selection/mutation ownership is reused for new
  accessible catalog forms, stable retry identity and public tracking URLs; public
  markup gains no credential. Real browser fixture follows redirect through the
  application and a fixture destination. Phase01 explicitly leaves production SLOs
  to deployment; the100-request/concurrency8/p95<1000ms check is an initial measured
  implementation budget, not an approved production target.

### Existing owner/consumer inspection

Trail discovery preceded reads. Inspected `lock_tenant` at
`services/attribution/src/adapters/identity/tenants.rs:10–18`, `begin_merchant` at
`adapters/setup/merchant.rs:102–130`, user_transaction at
`adapters/setup/postgres.rs:99–124`, receipt/mutation/fingerprint at188–268, and
approved_destination at440–452. Exact literal searches supplemented absent caller
edges and included test-only audit inserts and retained receipt consumers. There
is no current production catalog/redirect caller to preserve beyond the named
shared owners. Existing URI and receipt tests were inspected/reused as foundation
context, not treated as p05 behavior proof. No missing cross-application consumer
or reverse lock order was identified.

### Verification and limitations

Executed artifact checks only: source start/end snapshot, immutable decision hash,
branch/HEAD, pinned coordination byte equality and git diff --check. No p05 source
implementation exists, so no p05 runtime tests or evidence manifest are expected at
this gate. No servers, database fixtures or reviewer-owned resources were started.

All AC1–AC9 remain **planned, not exercised**: scope/constraint/permission cases;
concurrent receipts/principal classes; actual302 URL preservation/domain checks;
production metadata edits and immutable click history; concurrent GET/HEAD/classifier
semantics; actual COMMIT acknowledgement loss and client disconnect; admission and
freshness; browser and measured latency; inherited p04/p03/foundation/contracts,
format/offline/fresh migrations/SQLx checks. Later whole-plan/container/pilot
dispositions remain p10. This review establishes neither implementation behavior,
public DNS/proxy deployment nor production performance.

### Outcome and process feedback

**PASS for AS-P05-D1/r1**, SHA256
`c65c8f5653783df03d83a871adaf8ecb163e1f44fd1a07880df93523b49529b5`.
No actionable findings or unresolved gate questions; no findings file is needed.
This covers the scoped AS-C004/r1 changes as written and permits implementation,
while root retains phase acceptance and downstream dependency decisions.

The cohesive gate is appropriate: authorization, receipts, snapshots and public
redirect durability share transaction boundaries. Its explicit postcommit fault
fixture requirements and real product-edit consumer prevent misleading stand-in
proof. The p04 review lessons are reflected in decisive planned assertions, but
their execution remains outstanding. No new instruction change is supported by
this architecture inspection.

## AS-P05-B1-N1 — 2026-11-16 — Whole-phase implementation review AS-P05-B1-R1

Workflow implement-astra-reviewed-2-3, p05 cohesive05A–05B batch.
[TASK](TASK.md), [REVIEW](REVIEW.md),
[author evidence manifest](logs/AS-P05-B1-R1.evidence.json).
Reviewer `/root/attribution_reviewer`, Astra/high requested, retained independent B;
actual model/effort/context telemetry unavailable. Author `/root`, user-confirmed
Sol6.1/xhigh. B authored only assigned review records, no project edits or subagents.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service; phase start/review-base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
Reviewed artifact
`a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d`;
exact six exclusions remain the literal paths recorded in G1/REVIEW. Start/end
snapshots and identity output retained under `logs/AS-P05-B1-R1-reviewer-*`.
Architecture AS-P05-D1/r1 remains unchanged, SHA256
`c65c8f5653783df03d83a871adaf8ecb163e1f44fd1a07880df93523b49529b5`.

Authoritative coordination is
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision11; pinned `logs/AS-P05-B1-coordination-r11.md` matched
byte-for-byte. AS-C001/r1–AS-C004/r1 active and acknowledged. Accepted p04
`a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`
and its B AS-P04-B1-N2 provide prior evidence, but affected shared owners were
rechecked here. Root owns subsequent phase acceptance and dependency release.

### Requirements, consumers and failure paths examined

Reviewed against original05, applicable root/service instructions, service network
contract README/OpenAPI, accepted gate and actual TASK AC1–AC9/consumer maps.
Trail discovery/skeletons preceded exact source reads; scoped exhaustive references
covered shared receipts/audits and their test-only consumer. Inspected all catalog
production model/SQL/HTTP paths, shared management extraction, changed IDs/destination/
merchant helper, schema, shell assets, all catalog test cases and the expanded
browser/SQL consumer. Unchanged identity/freshness/crypto foundations were reused,
not uniformly rereviewed as new code. No root producer or cross-application source
change required a new root test run.

- **AC1/AC2:** session-only campaign/ad routes and session-or-key link routes;
  current Read/ManageLinks or links scopes; explicit foreign company/store/parent
  predicates and composite SQL FKs. Key detail/update lookups apply the fixed store
  filter before mutation. Lists use bounded encrypted company/resource/filter
  cursors. Competing exact creates yield201+200 with one effect/audit/receipt;
  changed normalized body/time conflicts and current authority precedes replay.
  Forced equality of actual user/credential UUIDs creates separate receipt kinds
  through both real auth paths; revoked key replay fails. Receipt extraction retains
  UTC precision, database-time horizon and p04 metadata-only secret replay.
- **AC3/AC4:** destination input and canonical serialized2048-byte bounds, IDNA
  exact live approval and decoded ob_click rejection. Actual302 preserves duplicate
  query pairs and fragment while adding one click ID. AdUpdate absent/set/null
  semantics are distinct in application serialization, SQL and published contract.
  Production campaign/ad/product/link PATCH operations leave prior click JSON
  unchanged and later clicks capture updated names/product/revision; null clears
  product. Parent/store/domain disablement blocks redirects. Shared p04 expiry
  behavior is inherited through approved_destination/approved_domain, with its
  existing expiry tests retained. Disable-only link changes remain possible after
  domain revocation; they do not admit new clicks.
- **AC5:** public preliminary lookup only supplies local scope; authoritative
  freshness and tenant lock precede complete link/ad/campaign/store/domain checks.
  DB defaults timestamp clicks at insertion; scoped immutable dimension snapshots
  and bounded booleans/version are written in that transaction. Location is
  constructed and checked before insert/COMMIT; successful commit alone permits302.
  Explicit HEAD checks eligibility and returns204 without Location/row. Concurrent
  GET fixture yields101 distinct durable IDs for101 requests. Stored columns/writes
  contain no raw IP, UA or referrer. Classifier is documented heuristic evidence,
  not a human-identity assertion; forwarded headers confer no authority.
- **AC6:** inspected the entire test-only wire relay. It forwards actual PostgreSQL
  frames and recognizes CommandComplete(COMMIT), then mode1 drops the acknowledgement.
  The request exercises the real assembled router and redirect SQL using that pool;
  independent original-pool SQL sees one committed row before503/no Location, and
  a new GET produces a second click. This is not a precommit failure stand-in.
  Mode2 withholds the acknowledgement while the real TCP HTTP consumer disconnects
  after observed commit; releasing it preserves that row and the next GET adds one.
  CHECK rejection separately proves failed storage returns503/no Location/no row.
- **AC7:**16 public permits before database resolution, shared8-per-tenant permits,
  existing global32/connection64/deadlines unchanged. Actual public and tenant
  contenders prove429 and recovery; health remains available. `pg_blocking_pids`
  establishes both queue orders of production link disable and redirect, yielding
  one accepted earlier click or a denied waiter, then no later click. Durable stale
  authority and authoritative deleted state fail closed. The real merchant TCP
  header-only request receives401 without supplying its promised body.
- **AC8:** browser uses actual forms/API/PG and a local DNS protocol fixture, with
  identity authority explicitly a stand-in. A real committed link-create response
  is aborted in transport; retry preserves exact key/time/body and creates one link.
  Product editing flows through the rendered form. Chromium follows the service302
  to an invocation-owned HTTPS destination; independent SQL asserts one click with
  edited product P2. P04 secret/rotation/revoke/member behavior remains in the same
  journey. Accessible labels, keyboard submit and text-node escaping were examined.
  Browser host mapping/self-signed TLS bypass are test-only. Pagination/edit wiring
  was inspected and API pagination exercised; not every browser error path or
  pagination combination was separately driven.
- **AC9:** shared ActorKind/Receipt owner, setup callers and merchant audit fixture
  move together; no dual path or legacy decoding. Nonnil ID macro behavior remains.
  Catalog router is composed once inside the existing global boundary. Reset-only
  migration adds catalog/click constraints and changes receipt/audit discriminator;
  no backfill or Openbot schema dependency. Service README accurately distinguishes
  CDN configuration, implementation budgets and later-phase behavior. AdUpdate wire
  extension matches the gated AS-C004. Reviewed learning l-70a31dc0 against its actual
  local HTTPS browser fixture and retained prior stable-select-label learning.

### Independent execution and inspected evidence

Service cwd, Rust1.95.0/offline mode, `/tmp/as-attribution-target`, stock2MiB service
test stack, invocation-owned PostgreSQL fixtures and pinned Playwright:

- `cargo test --locked --offline --lib catalog -- --nocapture`: **14 PASS**;
  [output](logs/AS-P05-B1-R1-reviewer-catalog.txt). Warm100-request/concurrency8
  fixture measured p50=6ms, p95=29ms, p99=39ms, below its1000ms p95 regression bound.
- `cargo test --locked --offline --lib setup -- --nocapture`: **18 PASS**, browser
  explicitly ignored in this selection; [output](logs/AS-P05-B1-R1-reviewer-setup.txt).
- `cargo test --locked --offline --lib attribution_setup_browser_real_forms_and_postgres -- --ignored --nocapture`:
  **one PASS/12 checks**; [output](logs/AS-P05-B1-R1-reviewer-browser.txt).
- Expected-artifact manifest validation: **nine retained PASS entries**;
  [output](logs/AS-P05-B1-R1-reviewer-manifest.txt). Snapshot/decision/coordination/
  branch/HEAD and git diff --check confirmed at review end.

Inspected actual current author outputs for21 identity tests,14 foundation tests,
fresh migration and SQLx metadata check, formatting/offline all-target compilation,
10 boundary tests,13 contract tests, and setup's split18-test evidence. Current
catalog/browser claims are supported by the assertions above, not only the green
manifest. Intermediate harness/environment failures and their corrections remain
in TASK/logs. Browser test retains its prior name and assertions while extending
coverage; no tests were removed. Dynamic catalog SQL is exercised against real
fresh schema; SQLx macro metadata remains separately checked.

Independent runs finished and fixture harnesses closed their servers, relays,
browsers, temporary certificates and UUID-owned databases/roles. No reviewer-owned
process or resource remains. B did not rerun unaffected full/root suites or current
extracted/container gates. This fixture latency is not a production SLO; public DNS,
production Openbot deployment, real-store/native integration and p06–p10 checkout,
commerce/reporting/retention/full-plan evidence remain outside this review's result.

### Outcome and process feedback

**PASS for `a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d`.**
No actionable findings; no findings file is needed. Root owns phase acceptance and
later integration; this review does not release deployment or claim full-plan
completion.

The cohesive phase was reviewable because schema/history/authorization/receipt and
redirect effects share clear transaction owners. The explicit gate requirements for
real COMMIT acknowledgement loss, competing production writers and actual product
edits are reflected in runtime assertions. The browser learning is scoped to an
observed fixture failure and preserves the actual navigation consumer. No additional
AGENTS/skill instruction is supported by this round; a clean review alone does not
establish uniform correctness or broad process effectiveness.
