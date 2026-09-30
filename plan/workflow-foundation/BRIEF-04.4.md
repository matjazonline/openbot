# 04.4 — approved provider replay contracts

Status: affected expansion Astra PASS and ROOT ACCEPTED2026-09-30;
implementation/correction actual-code review PASS; final evidence signoff/root acceptance pending.
Original04 Execution item3 remains authoritative. Accepted04.1–03 and phase03 preserved.
Root owns PROGRESS/RESUME acceptance; worker owns this brief/code/affected EXPANSION.

## Concrete contract and dependencies

Existing publication ApprovedActionPolicy has explicit SafeRepeat/ProviderIdempotency/
Reconcile; frozen policy is necessary but does not establish that the selected transport
implements it. RemoteAction currently lacks a registered provider contract and enforced
stable-key argument. Existing remote marker is always conservative and remote success is
only an observation;04.5 has not yet committed remote receipts.

04.4 adds a trusted-registration-only ProviderReplayContract, matched to exact frozen
company/target/ToolSnapshot (including schemas, capability, effect, recovery and policy
revision). Registered modes explicitly guarantee either safe repetition of the operation,
or provider deduplication of the service-supplied stable logical-operation key with an
explicit nonzero bounded retention interval covering the remaining run deadline.
Registration identity uses bounded TypeName, never HTTP method/header/MCP metadata/session
IDs. Types have private fields and no untrusted-deserialization constructor. The descriptor
is supplied synchronously by a required RemoteAction method; no discovery/network I/O.
Missing/unsupported/mismatched proof for a frozen replay mode fails before marker creation.
Reconcile operates without a replay descriptor and without a replay/idempotency promise.

The service captures this approved contract and validated ProviderInvocation before durable
reservation. Provider invoke receives the prepared invocation alongside FrozenAction, so
ProviderIdempotency gets its canonical stable key, never an attempt/session key. Persist
approved descriptor facts inside existing append-only dispatch current_policy JSON together
with current tool policy; no second job/attempt owner, no new lease/queue. Final entry checks
the exact persisted descriptor as well as existing scope/fence/authority/time checks.

This point does not issue a second reservation from an existing marker. Without committed
remote receipts, doing so could resend after a successful observed response.04.5 must first
return a matching committed receipt without transport and then integrate receipt-aware safe
replay, preserving sequential/competing claimant exclusion and durable first-dispatch proof.
04.5 must use persisted proof identity/retention since first marker, current registered proof
and current authorization; expired/changed/unsupported contracts cannot promote an unknown
to safe. SafeRepeat and ProviderIdempotency protocol integration remains an explicit04.5
dependency, not an04.4 completed-provider-retry claim.04.6 parking and04.7 evidence remain
required.04.9/10 own production adapter registration (trusted approved sources only), actual
key placement/dedup semantics, transport security and MCP tool-error/output envelopes.

## Affected files and acceptance

Application actions/replay.rs plus exports; dispatch.rs required provider contract/key port
and consuming reservation data; freeze.rs shared bounded digest helper. Persistence
workflow/action_dispatch.rs validates proof before first marker, persists proof and checks
it again at final entry. Existing dispatch test adapters implement required methods explicitly.
Separate replay test file reuses isolated fixtures and adds policy-configurable preparation.
No applied migration changes; current_policy JSON already retains matched facts append-only.

Meaningful tests: exact approved SafeRepeat/ProviderIdempotency first call; actual scripted
provider sees service key equal durable prepared intent key; changed argument/target/company/
tool/schema/policy/recovery/registration and retention mismatch rejects before provider I/O
and marker; finite retention expiry/deadline check; no inference from GET/PUT/client header,
MCP idempotent/read-only annotations or session/request IDs. Lost response, invalid output,
tool-error observation, timeout/cancel/crash/restart/reclaim cannot erase marker or authorize
an unsupported replay. Competing step/model callers on one logical invocation yield one
actual provider call and marker even with approved idempotency; restart blocks second call
pending04.5. Exact descriptor persists and cannot be changed by caller/DB mutation.

Required gates: all accepted workflow_action tests + new tests at stock2MiB through scoped
stack-budget; fmt/both whitespace checks; locked offline alltargets; strictClippy;
migrations/SQLx prepare/check as changed; graft build. Fresh40migration schema evidence
from04.3 can be reused only with unchanged migration/schema assumptions. No bounds raised.
Independent actual-code and correction review required before root point acceptance.

## Worker identity and context

Implementer /root/action_replay UUID01a0f231-8cf2-7de3-bc73-3f3954dd1938,
turn_context verified gpt-6.1-sol/high. Startup22715/2584008.79%12:02:21Z;
reconciliation80297/25840031.07%12:08:44Z. Depth0 usage/runtime capacity.
Fresh reviewer /root/action_replay/replay_review UUID01a0f232-3d81-7070-adf8-22ecd5465e08,
verified Astra/medium, startup29294/25840011.34%12:03:08Z. No deeper agents.
TaskPG retained stopped; no source implementation edits yet.

## Expansion acceptance and implementation checkpoint

Independent reviewer actual-seam/original-criterion review PASS, no blocking findings.
Reviewer51232/25840019.83%12:11:54Z, usage/runtime. Root accepted foundation scope
with explicit04.5 receipt-owned retry obligation; no completed retry/production claim.
TaskPG restarted retained exact endpoint; baseline affected source snapshots
/private/tmp/workflow-04.4-baseline. No applied migration/schema modification.

Application proof is a versioned typed registration descriptor matched through canonical
snapshot+target SHA256; private fields and no Deserialize. Transport invocation gets the
service-owned key only for explicitly approved provider idempotency. Captured descriptor
is persisted in existing immutable current_policy->provider_replay JSON and compared at
final entry. Six new isolated DB groups exercise approved first-call/key+competing tool,
exact mismatch/unsupported/retention rejection, policy-only insufficiency, ambiguous
output/lost/cancel observations, real metadata observation with no promotion, marker crash
and swapped descriptor refusal. Existing adapters explicitly supply None for Reconcile.

First compile found missing AppResult import; corrected. New exact queries are runtime SQL;
SQLx preparation still required. Prior fresh40migration evidence reusable (no schema edits).
Milestone98073/25840037.95%12:18:46Z;102721/25840039.75%12:21:18Z depth0.

## Review correction and final gates

Initial actual-code review found P2: the lease window underestimates the run deadline
after lock waits and cannot prove retention covers the actual remaining run horizon.
Corrected provider validation to use a positive rounded-up microsecond duration read
from the already locked run's authoritative database clock. Existing conservative I/O
deadline remains unchanged. Seventh regression holds a competing run lock700ms with
2sec deadline/900ms contract, confirms the post-wait actual horizon exceeds retention,
and asserts zero marker/provider calls. Reviewer correction PASS/no remaining findings,
UUID01a0f232-3d81-7070-adf8-22ecd5465e08 Astra/medium,
71044/25840027.49%12:29:38Z. Final evidence signoff pending.

Final frozen-source gates PASS (2026-09-30):
- env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 scripts/stack-budget.sh --offline workflow_action_:
  36/36/0ignored, accepted29+7new, tests12.26s;
  /private/tmp/workflow-04.4-stack-budget.log.
- cargo fmt --check; git diff --check; git diff --cached --check: no errors.
- env SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS22.08s;
  /private/tmp/workflow-04.4-offline-alltargets.log.
- env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  PASS109s; /private/tmp/workflow-04.4-strict-clippy.log.
- Both exact task URLs above: cargo sqlx prepare -- --all-targets PASS34.85s and
  cargo sqlx prepare --check -- --all-targets PASS21.88s;
  /private/tmp/workflow-04.4-sqlx-{prepare,check}.log. No .sqlx changes.
- graft build PASS14273nodes/21204edges/714cards; summary output
  /private/tmp/workflow-04.4-graft-build.log (progress truncated, final counts retained).
- Fresh40migration PASS evidence reused /private/tmp/workflow-04.3-fresh-migrations.log:
  no migration/schema edits in04.4, immutable applied migrations preserved.

Earlier35/36 passing development runs are in /private/tmp/workflow-04.4-first-stack-budget.log
and /private/tmp/workflow-04.4-corrected-first-stack-budget.log; final log supersedes both.
No bounds raised; no combinedphase04/full-library or actual safe retry claimed.04.5 must
integrate committed receipts and supported replay before any existing marker can redispatch.
Final gate boundary123072/25840047.63%12:35:22Z depth0 usage/runtime sources;
final evidence dispatch125509/25840048.57%12:36:28Z.

Combined independent actual-code/correction/criterion/final-evidence PASS, no outstanding
blockers. Reviewer01a0f232-3d81-7070-adf8-22ecd5465e08 Astra/medium,
77940/25840030.16%12:37:07Z usage/runtime sources; interrupt confirmed completed and
quiescent. Root alone accepts04.4 before04.5. TaskPG STOPPED CLEANLY/retained; no active
build/test/SQLx/approval. No stage/unstage/commit/reset/deploy. Natural implementer retirement
handoff with code/brief/expansion/checks complete. Nextworker uses existing04.5 obligations,
not another04.4 rediscovery. All previous modified/untracked files preserved.
