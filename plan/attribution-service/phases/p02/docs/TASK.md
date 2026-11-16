# Attribution p02 — standalone foundation

- Workflow: implement-astra-reviewed-2-3; installed skill directory
  `/Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3`.
- Root/A `/root`: Sol 6.1/xhigh, user-confirmed. B `/root/attribution_reviewer`:
  requested Astra/high, independent retained context; p01 activity complete.
- Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
  `attribution-service`; starting/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Original entry tree clean. Preserve accepted sequential p01 uncommitted artifact
  `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`,
  prerequisite accepted by AS-P01-B1-N2 with D1/r2 and D2/r1 decisions.
- Record root `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p02/docs`.
- Coordination authority absolute plan-root COORDINATION.md, writer root, revision3
  read at startup. AS-C001/r1 and AS-C002/r1 acknowledged. No pending requests.
- Exact snapshot exclusions for this sequential batch: p01/docs and p02/docs under
  plan/attribution-service/phases/, and exact plan/attribution-service/COORDINATION.md.
  All implementation/contract/plan deliverables remain included.
- Scope: services/attribution own Cargo package/lock/config/source/migration/assets,
  foundation tests/scripts/docs/Dockerfile and separate CI job. Root application
  source remains untouched in this phase.
- Foundation implementation complete; focused runtime and extraction checks passed; independent batch review pending; accepted/integrated no.
- Context controls/occupancy unavailable; use durable checkpoints, host defaults.
- No extra architecture gate: independent DB ownership was accepted in p01; the
  foundation uses established startup/pool/isolation patterns. No domain schema,
  identity grants or new business concurrency protocol is implemented in p02.

## Acceptance mapping

| Criterion | Path/case | Decisive proof | Focused checks |
| --- | --- | --- | --- |
| AC1 / 02A.1–3 | Standalone config/startup/health | Reject malformed/secret-bearing public origin, wrong database/privileged role; liveness cheap, readiness reflects DB; bounded shutdown stops listener | attribution_startup_ unit and real-DB HTTP tests |
| AC2 / 02A.4 | Own role/database and migrations | Fresh migrations pass as non-superuser owner; corrupted migration fails; distinct app role cannot connect/read other owned DB | attribution_storage_boundary_ real Postgres tests |
| AC3 / 02A.3,5 | Bounded HTTP/runtime/image | Connection/request/time/body bounds reject controllably; secure/redacted responses; non-root image runs without parent assets | startup/limits tests, container smoke |
| AC4 / 02B.1–4 | Package/source/config isolation | Resolved dependencies/source/config cannot reach parent/Openbot; service-only copy compiles/tests independently | attribution_boundary_ probes, extracted build and independent CI |
| AC5 / root instructions | Formatting/offline metadata and test discovery | Nonzero selected tests; fresh own metadata; locked/offline all targets compile | fmt, focused inventory/run, cargo check, sqlx prepare |

No native platform integration/pilot is claimed. No missing DB skipping. Container
runtime unavailable on current host; implement executable CI/smoke proof and retain
local unavailability explicitly until a runtime is available. Do not call it passed.

## Traced ownership and implementation choices

New service has no root-package dependency. Foundation owns config and lifecycle
in infra, HTTP admission/security/health in its adapter, and its pool/migrations in
Postgres adapter. Avoid empty domain/application ports until actual behavior needs
them. Embed service-owned assets. Use locked maintained dependencies; no git refs.

The migration creates only an application identity marker and its own SQLx history.
Validate nonprivileged dedicated DB ownership and refuse preexisting foreign public
tables before initializing. Never reset a database automatically. CLI migrate and
serve use the same migration/startup owner. No other application's tables or secrets
are read. Tests create uniquely prefixed databases/roles on the local maintenance
connection and clean only their own resources.

Read existing memory-service startup/test boundaries through Trail; reuse principles,
not its private source/dependencies or detached signal-task pattern. Runtime must
retain/join supervision and bound connection lifetime/idle/request/drain waits.
Service test/runtime stacks use stock 2 MiB; no raised bound or weakened root budgets.

## Resources and verification sequence

- Existing contract venv `/tmp/attribution-service-contract-env` remains p01-owned;
  use it for unchanged contract checks; prefer Homebrew python3.13 for new scripts.
- Planned cargo target `/tmp/as-attribution-target`, isolated from other workstreams;
  test DB/role prefix `as_test_`; maintenance URL supplied only to test process.
- Only 13 GiB free disk; use debug=0/incremental=false for service dev/test profiles.
  This reduces artifacts without raising stack/lease limits. Do not clean others.
- Run focused startup/storage/boundary tests and offline compilation for this batch.
  List tests first. Save logs and exact selected/executed counts.
- After the complete task, p10 runs both applications' full current CI gates,
  migrations/metadata/database/browser/boundary/container/recovery/integration tests.
  p01 tests alone cannot establish service functionality.

## Next

Implement complete foundation, provision only disposable owned test databases,
validate focused paths and extraction, then freeze and request B's whole-batch review.

## B1 implementation and verification — 2026-11-16

Whole foundation batch spans service Cargo/lock/toolchain/config, bounded runtime/HTTP,
owned database marker/migrations/SQLx metadata, embedded CSS, dedicated-role fixtures,
verification/boundary/extraction scripts, documentation, non-root image and separate CI.
No existing root application/CI behavior was edited. No Rust tests were removed;
new discovery is 12 startup tests + 2 storage-boundary tests.

Contract impact: config -> CLI/startup -> dedicated pool/migrations -> readiness;
listener semaphore/deadlines -> Axum IO -> graceful shutdown; HTTP admission/body
limit/deadline -> response normalization and correlation/security headers. Tests
exercise these production functions. DB permission fixture uses two independently
owned databases, denies CONNECT in both directions, then deliberately grants CONNECT
and asserts SQLSTATE 42501 on table reads in both directions. Identity HTTP consumer
is not present until p03; the contract-faithful identity server will be exercised
with that consumer in p03, then real two-app integration in p10. Do not claim the
foundation has implemented login.

| Criterion | Executed decisive assertion/evidence | Status |
| --- | --- | --- |
| AC1 | 12 `attribution_startup_` tests: malformed config/secret redaction; foreign schema and privileged role rejected; corrupt migration aborts startup; live health/readiness/assets and listener stops; actual admission/body/deadline/IO bounds | PASS, `logs/AS-P02-foundation-r1.txt` |
| AC2 | 2 `attribution_storage_boundary_` tests: nonprivileged fresh migration/idempotent rerun; real bidirectional CONNECT and table-read denial | PASS, same log |
| AC3 | Competing claimant request and connection tests, idle/absolute deadlines, forced shutdown wakeup, stock runtime/test stacks; Dockerfile + real non-root smoke runner added | Runtime PASS; container execution NOT EXERCISED |
| AC4 | 5 executable boundary mutation tests including transitive wrapper, outside include/computed include, foreign/quoted SQL, private config/import and unpinned git; resolved real cargo metadata. Extracted service copy runs format/offline build/migrations/fresh metadata + 14 Rust and 5 boundary + 12 contract tests | PASS `logs/AS-P02-extracted-r1.txt`, boundary quoted-SQL extension `logs/AS-P02-boundary-r2.txt` |
| AC5 | Locked offline all-target compile, format, migration and SQLx prepare --check against disposable nonprivileged DB; nonzero selections recorded | PASS foundation/extracted logs |

Commands executed from `services/attribution` with explicit maintenance fixture,
`CARGO_TARGET_DIR=/tmp/as-attribution-target`, Homebrew python3.13, and extracted
contract interpreter `/tmp/attribution-service-contract-env/bin/python`:
- `scripts/verify.py prepare`: first failed because psql treats a URI in PGDATABASE
  as a literal name. Fixed by passing the URI directly to its dbname argument,
  without echoing credentials/client SQL. Successful regeneration in prepare-r2 log.
- `scripts/verify.py foundation`: SQLx migration/check, formatting, offline all-target
  compilation, boundary runner + 5 tests, list/run startup (12) and storage (2).
- `scripts/extracted-build.py foundation`: identical gates in service-only temp copy,
  then unchanged contract suite (12). No parent config, source, assets or DB required.
- `scripts/test_boundary.py` and `scripts/boundary.py`: final quoted-identifier
  probes and actual resolved graph pass after that checker enhancement; no runtime
  source changed after the Rust/extraction checks.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: additional lint
  check in `logs/AS-P02-clippy.txt` (PASS).

Initial dependency offline resolution failed on the cached yanked spin 0.9.8 index;
networked lock generation selected spin 0.9.9. Locked fetch then all subsequent
build/test commands succeeded offline. No yanked pin/shim introduced.

Container runtime is absent. Disk declined from 13 GiB at original start to 4 GiB
while other work uses this host; do not delete unrelated files or caches. A user
question asks to carry container execution to the p10/final CI gate. Until an answer,
this remains a required gap and does not count as accepted full p02. The smoke runner
and CI are reviewable implementation artifacts, not evidence of image execution.

Resources: own cache `/tmp/as-attribution-target`; unique `as_test_*` and `as_verify_*`
databases/roles are cleaned by their invoking fixture. Post-check catalog query
shows no remaining databases with those prefixes. One accidental local service
build cache was canceled and removed; no external task resources were touched.

Coordination revision 3 / AS-C001/r1, AS-C002/r1 rechecked before review. All applicable
p02 dispositions unchanged. New local AGENTS.md is the phase-required standalone
package guide, not an update inferred from review lessons.

Frozen for independent B1-R1 review at `c6bb668f041e2dd867ef036fde79186a9136397ee5b5d8e0b86ea9be001172f1`; exact exclusions above. Manifest and immutable coordination copy are retained in logs. `trail check` completed successfully. Container disposition remains pending.

## B1-R1 review and combined corrections

B returned AS-P02-B1-N1, CHANGES REQUIRED, at c6bb668f. Findings
AS-P02-B1-F1 (CI required hashes absent), F2 (actual package open_bots), F3
(unaudited internal wrappers), F4 (quoted SQL in Rust raw strings). Required V1
container evidence remained pending. B independently reran 14 Rust + 5 boundary
tests; these passed but did not distinguish the four defects. No phase acceptance.

A addressed all four together:
- F1: add published SHA256 hashes for all non-yanked release distributions of the
  same 19 pinned Python dependencies. Keep CI's --require-hashes. Fresh venv
  `/tmp/as-p02-contract-clean` bootstraps with that exact install mode and passes
  all 12 contract tests, retained in AS-P02-clean-bootstrap-r2.txt. This strengthens
  the p01-owned tooling lock without changing its versions or wire contracts.
- F2: reject actual open_bots plus openbot/open-bots resolved package identities,
  direct imports, extern-crate aliases and private environment key prefixes.
- F3: explicitly constrain this independent service to one local Cargo package
  and targets within audited src/. Reject local wrappers and outside custom
  targets rather than add speculative multi-package scanner support.
- F4: tokenize complete raw/ordinary Rust strings before scanning SQL; retain
  quoted and schema-qualified SQL identifiers. Probe actual r# strings with
  multiple delimiter lengths, including positive owned-table queries.
- Boundary inventory increases from 5 to 9 tests; all original names retained.
  New probes cover real package with pinned git source, local wrapper/outside
  asset, custom target, raw SQL. New tests+resolved metadata+12 unchanged contract
  tests pass in a service-only temp copy (AS-P02-boundary-extracted-r2.txt).

Runtime Rust, Cargo/lock/config, metadata/migrations/assets, Dockerfile and smoke
runner are unchanged since R1 and the successful focused/extracted runtime evidence.
Only boundary checker/tests, README clarification and contract requirements lock
changed. Reuse unaffected runtime tests; do not rerun unrelated root tests.

Environment gap recovery: Homebrew colima/docker installed after disk freed to24GiB.
Own VZ profile as-attribution-p02 runs with 2 CPUs/4GiB RAM, no mounts or default
Docker context change. Actual image build in progress; release compilation passed.
The pending user deferral question is unnecessary if actual container proof passes;
no waiver has been assumed. Review/recheck waits for complete correction evidence.


## Frozen B1-R2 correction artifact

Digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d` at unchanged base/HEAD and exact original exclusions. Coordination
revision3/notices unchanged and pinned in R2 logs. All four correction claims and
V1 container evidence ready for independent recheck; not accepted until B passes.

Actual non-root image built successfully, image sha256
ff8df74f600c431b8234111731d3985f49caa50c3bd5b7f00b1cdce7f4554a5c, Linux arm64.
Runtime inputs (22 hashes in AS-P02-runtime-inputs.json) did not change between
image build and R2: only Python tooling, its lock and README changed. Final image
copies only the compiled binary; those changed tool/docs inputs are absent from it.
Reuse R1 runtime/extraction/lint results for unchanged source. The image build began
on c6bb668f source; current runtime hashes retain the source identity for recheck.

Initial container smoke failed at fixture SQL immediately after PostgreSQL readiness,
before app launch. Diagnostic rerun passed, identifying transient fixture startup
rather than a deterministic app failure. Hardened readiness to TCP because the
Docker image's temporary bootstrap server accepts Unix-socket readiness before its
final listener. Two independent final runner invocations passed: UID/non-root image,
read-only filesystem/capability denial, fresh migrations, HTTP assets/health/readiness,
DB-loss503/health204, SIGTERM exit0 and no credential leakage. Initial failure and
both correction successes retained, no weakened assertion. All UUID containers and
networks cleaned; task-owned Colima runtime retained for reviewer checks. Pending
user deferral question is superseded by actual proof; no waiver needed or assumed.

Only local arm64 image execution claimed; hosted CI has not been run remotely.
Full plan/system verification remains p10, and no p03+ implementation has begun.

## p02 acceptance — 2026-11-16

Root recomputed unchanged `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`, compared authoritative coordination revision3
with the review's immutable copy, and validated R2 evidence. B explicitly passed
AS-P02-B1-N2 and closed F1–F4/V1. p02 is accepted in phase and integrated into this
sequential worktree. This is foundation acceptance only, not plan completion.
Task-owned Colima profile as-attribution-p02 and its data disk deleted successfully;
logs/AS-P02-container-cleanup.txt retains cleanup. Installed tools remain available;
no unrelated profiles/resources touched. Next active phase: p03.
