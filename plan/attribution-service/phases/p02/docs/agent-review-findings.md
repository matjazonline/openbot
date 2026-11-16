# Attribution p02 reviewer findings

Shared review identity for the findings below: 2026-11-16, `implement-astra-reviewed-2-3`, phase p02, whole batch AS-P02-B1-R1; [TASK](TASK.md), [REVIEW](REVIEW.md), [AS-P02-B1-N1](agent-review-notes.md#as-p02-b1-n1--whole-foundation-review). Reviewer `/root/attribution_reviewer`, requested Astra/high, actual runtime metadata unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch `attribution-service`, start/base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`, digest `c6bb668f041e2dd867ef036fde79186a9136397ee5b5d8e0b86ea9be001172f1`. These are implementation/verification defects, not defects in a gated domain decision.

## AS-P02-B1-F1 — Clean CI cannot install the contract requirements

- Classification: actionable defect, **P1 / high**, CI correctness/verification.
- Evidence: `.github/workflows/attribution-service.yml:35–36` creates a fresh venv then runs pip with `--require-hashes`. `services/attribution/contract/requirements.lock:1–19` has version pins but no hashes. On a clean environment pip refuses the first requirement before the service dependency fetch/build/test steps run.
- Reproduction: Homebrew Python3.13 pip with `install --dry-run --ignore-installed --no-deps --no-cache-dir --require-hashes --break-system-packages -r services/attribution/contract/requirements.lock` exited1 with “Hashes are required ... missing” for attrs26.1.0. Nothing was installed; [retained output](logs/AS-P02-B1-R1-pip-hash-probe-r2.txt). The earlier old-pip unsupported-option attempt is separately retained and is not the reproduction.
- Required: make the locked requirements and exact CI install mode consistent, then exercise fresh environment bootstrap followed by contract validation. Keep dependency reproducibility and do not treat existing installed packages as proof of the clean job.
- Instruction lesson: existing reproducible-build and verification-contract requirements apply; the local prepared venv did not exercise CI's stricter producer path. No general skill change required.
- Resolution history: **OPEN**, 2026-11-16, B1-R1.

### F1 resolution — 2026-11-16, AS-P02-B1-R2

**FIXED** at digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`, unchanged base/HEAD. The same 19 pinned requirements now carry release-artifact SHA256 hashes. Inspected the retained fresh venv hash-required install and successful 12 contract tests; CI's exact installation mode is supported. [AS-P02-B1-N2](agent-review-notes.md#as-p02-b1-n2--correction-and-container-recheck).

## AS-P02-B1-F2 — Boundary checks use the wrong Openbot crate identity

- Classification: actionable defect, **P2 / medium**, independence regression guard.
- Evidence: the actual root `Cargo.toml:2` names `open_bots`; phase01's forbidden dependency names that package explicitly. `services/attribution/scripts/boundary.py:22,52` checks only `openbot` as package/import. Its tests at `test_boundary.py:32–35` use the same wrong identifier, so both checker and fixture pass while missing the actual dependency.
- Reproduction: isolated `use open_bots::Config;` source and resolved metadata for a revision-pinned `open_bots` package both yield `[]` (no boundary failures); [probe log](logs/AS-P02-B1-R1-boundary-probes.json). This is a checker false negative, not a claim that the submitted service currently imports Openbot. The outside-path check alone cannot reject an Openbot package obtained as a pinned git dependency.
- Required: reject the repository's actual package/crate identity and cover it in resolved metadata and source/import regression cases, including the intended wrapper/alias surfaces. Keep approved HTTPS identity configuration possible.
- Instruction lesson: use literal identifiers from original requirements/current Cargo metadata in boundary probes; the existing criterion/producer mapping already calls for this. Cause beyond the observed naming mismatch is unknown.
- Resolution history: **OPEN**, 2026-11-16, B1-R1.

### F2 resolution — 2026-11-16, AS-P02-B1-R2

**FIXED** at digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`, unchanged base/HEAD. Actual `open_bots` and corresponding package spellings are rejected in resolved metadata; direct imports/extern aliases are covered. New pinned-source metadata case and expanded import test passed in reviewer execution. [AS-P02-B1-N2](agent-review-notes.md#as-p02-b1-n2--correction-and-container-recheck).

## AS-P02-B1-F3 — In-service local wrappers bypass source and asset inspection

- Classification: actionable defect, **P2 / medium**, independence regression guard/coverage.
- Evidence: `boundary.py:24–29` accepts local dependency targets located anywhere inside the service, but lines45–49 only scan the root `src`, `migrations`, `assets` and `.cargo` directories. A local package under `wrapper/src` is in resolved metadata yet its source/import/include checks never run. Plan02B.2 expressly requires wrappers not to bypass the boundary.
- Reproduction: metadata for a local `service/wrapper` package/target plus `wrapper/src/lib.rs` containing `include_str!("../../../private-openbot.env")` returns no failures; [probe log](logs/AS-P02-B1-R1-boundary-probes.json). Existing wrapper test covers only a manifest outside the service, not an accepted local package importing an outside asset. Extraction may expose a missing file in an individual build, but does not satisfy the specified source boundary check.
- Required: include every resolved local package/target and its relevant source/config/assets/build inputs in the boundary audit, or explicitly reject local wrapper layouts not audited. Add a probe for an in-service local wrapper carrying the forbidden reference.
- Instruction lesson: the resolved dependency graph and source scanner must cover the same local producer set. Existing contract-impact guidance already requires wrapper/equivalent-surface coverage.
- Resolution history: **OPEN**, 2026-11-16, B1-R1.

### F3 resolution — 2026-11-16, AS-P02-B1-R2

**FIXED** at digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`, unchanged base/HEAD. Checker explicitly rejects extra local packages and custom target roots outside audited `src/`; README documents the single-package layout. Reviewer-executed in-service wrapper and custom-target cases pass by rejecting those unaudited inputs; actual package metadata also passes. [AS-P02-B1-N2](agent-review-notes.md#as-p02-b1-n2--correction-and-container-recheck).

## AS-P02-B1-F4 — Quoted foreign SQL in Rust raw strings is not checked

- Classification: actionable defect, **P2 / medium**, SQL boundary guard correctness.
- Evidence: `boundary.py:65–66` extracts regular quoted string fragments with a regular-expression pattern. In valid Rust `r#"SELECT id FROM "companies""#`, extraction stops at the SQL identifier's quote, leaving `SELECT id FROM ` without a table match; the remaining fragment is not treated as SQL. The new quoted-SQL tests encode ordinary escaped strings via `json.dumps`, so they do not cover Rust raw strings commonly used for SQL.
- Reproduction: `sqlx::query(r#"SELECT id FROM "companies""#);` yields no boundary failures, while `sqlx::query("SELECT id FROM companies");` is rejected; [probe log](logs/AS-P02-B1-R1-boundary-probes.json).
- Required: handle supported Rust raw-string SQL content, including quoted/schema-qualified identifiers, or fail closed on uninspected SQL string forms. Preserve positive owned-table cases and add actual raw-string mutation cases. Database permissions remain the runtime boundary, but the required source regression guard must reject this literal foreign query.
- Instruction lesson: test source encodings as they appear in production rather than only the fixture serializer's encoding. No general rule beyond existing decisive-assertion guidance is required.
- Resolution history: **OPEN**, 2026-11-16, B1-R1.

### F4 resolution — 2026-11-16, AS-P02-B1-R2

**FIXED** at digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`, unchanged base/HEAD. Complete raw/ordinary Rust string extraction preserves embedded identifier quotes; raw foreign/schema-qualified queries at multiple delimiters are rejected while the owned-table control passes. Reviewer executed all nine checker tests and actual source audit. [AS-P02-B1-N2](agent-review-notes.md#as-p02-b1-n2--correction-and-container-recheck).

## AS-P02-B1-V1 — Required container execution evidence pending

- Classification: required verification gap; no separate source-defect severity assigned.
- Submitted manifest explicitly records production image build/smoke NOT EXERCISED. The required non-root/read-only startup, actual runtime dependencies, embedded assets, database-loss readiness and graceful process exit cannot be accepted from Dockerfile/runner inspection.
- Root is attempting a task-owned runtime after disk became available. No accepted successful output or authorized deferral was provided by review return. Keep the requirement open until new immutable evidence or an explicit user disposition is reviewed. A passing source subset is not full p02 acceptance.
- Resolution history: **OPEN**, 2026-11-16, B1-R1; implementation findings F1–F4 independently block acceptance.

### V1 resolution — 2026-11-16, AS-P02-B1-R2

**SATISFIED** at digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`. Actual image build and two final full smoke runs passed after fixing PostgreSQL fixture readiness to wait for TCP. Reviewed all retained assertions/results and independently verified unchanged runtime hashes, image identity and runtime UID10001. Image `sha256:ff8df74f600c431b8234111731d3985f49caa50c3bd5b7f00b1cdce7f4554a5c`, local Linux arm64; hosted CI/other architectures not claimed. No waiver used. [AS-P02-B1-N2](agent-review-notes.md#as-p02-b1-n2--correction-and-container-recheck).
