# Attribution p02 reviewer notes

## AS-P02-B1-N1 — whole foundation review

- Date/workflow: 2026-11-16, `implement-astra-reviewed-2-3`, p02, AS-P02-B1-R1; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`, requested Astra/high with independent retained context; actual runtime model/effort/context occupancy unavailable. Durable checkpoint here; no agent spawning or implementation/design edits.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`; start/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Digest: `c6bb668f041e2dd867ef036fde79186a9136397ee5b5d8e0b86ea9be001172f1`; exact exclusions `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/phases/p02/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 3, notices AS-C001/r1 and AS-C002/r1; read at startup and rechecked against the [immutable submission](logs/AS-P02-B1-R1.coordination.md) before return. No changed applicable requirements.
- Accepted prerequisite: p01 AS-P01-B1-N2, digest `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`; prior requirements/contract review reused. No new costly domain schema or protocol architecture gate was necessary for the foundation.

### Coverage and trace

Read the original phase02 requirements, applicable root and service AGENTS, current TASK mappings and review/evidence guidance. Used Trail discovery/skeletons before source and caller tracing for database validation; recovered tool-elided source bodies before judging them. Reviewed all new Rust production files and tests, module roots, configuration example, migration/SQLx identity query, Cargo/dependency/toolchain/stack declarations, Dockerfile/ignore rules, CSS ownership, all verification/fixture/extraction/boundary/container scripts, service README, and dedicated CI workflow. No root application source or existing CI was changed.

Traced config through CLI, database validation/migration and listener; HTTP admission/security/body/deadline behavior; connection semaphore and IO deadlines/forced shutdown; pool/readiness; owned test resource cleanup; resolved dependency and source boundary checks; extracted verification and image runners. Existing PostgreSQL tests use two distinct nonprivileged owners and exercise both CONNECT denial and actual SQLSTATE42501 read denial after deliberately granting CONNECT. HTTP/connection tests use competing production paths, not only sequential mocks. The bounded runtime tests support their narrow assertions. The foundation has no identity HTTP consumer; TASK explicitly assigns that contract-server proof to p03, with real two-app proof in p10.

The source boundary guard is correctly described as a regression guard rather than runtime database authorization. Nevertheless its required concrete import/wrapper/SQL cases have false negatives, proven by F2–F4. CI's exact environment bootstrap was not covered by the locally preinstalled venv, exposing F1.

### Checks and limitations

Executed reviewer inventory and focused Rust tests with `SQLX_OFFLINE=true`, `RUST_MIN_STACK=2097152`, isolated phase target `/tmp/as-attribution-target`, and local PostgreSQL maintenance access. Tests allocated their UUID-owned `as_test_*` resources through production fixture cleanup. **12 startup + 2 storage tests passed**, and the **5 existing boundary tests passed**; output retained in [reviewer-focused log](logs/AS-P02-B1-R1-reviewer-focused.txt). No unrelated root/full suite was run.

Inspected retained author foundation/extraction, formatting/offline compilation/migration/SQLx metadata and clippy evidence. Validated [manifest](logs/AS-P02-B1-R1.evidence.json): five PASS, one historical FAIL followed by correction, one NOT EXERCISED. The initial fixture PGDATABASE failure is corrected and is not an open finding. No removed tests. Author extraction proves the current service-only copy builds; it does not exercise every future regression the boundary checker promises to reject.

Executed isolated boundary probes with temporary synthetic packages/source, then removed them; retained [results](logs/AS-P02-B1-R1-boundary-probes.json). Actual `open_bots` import/package, nested local wrapper external include and quoted foreign SQL in a Rust raw string all returned no failures. Ordinary foreign SQL was rejected as the positive control. No prohibited dependency or foreign query was added to the implementation.

The first pip probe used the old contract venv and failed because that pip lacks `--dry-run`; [output retained](logs/AS-P02-B1-R1-pip-hash-probe.txt), not evidence of F1. Repeated with Homebrew Python3.13 pip in dry-run/ignore-installed/no-cache mode, installing nothing: it failed with the expected missing-hashes error for the first locked requirement; [decisive output](logs/AS-P02-B1-R1-pip-hash-probe-r2.txt). This reproduces clean CI bootstrap rather than relying on an already populated venv.

Production-container smoke remains **NOT EXERCISED** in submitted evidence (V1). Root reported newly available disk and an attempt to install a task-owned container runtime; no success or user waiver was supplied before return. No container proof is inferred from static image/runner inspection. Final branch/HEAD, whitespace, coordination, manifest and digest checks are in [reviewer-checks](logs/AS-P02-B1-R1-reviewer-checks.txt).

### Outcome and process feedback

**CHANGES REQUIRED**: F1–F4, with independent container verification gap V1. No whole-foundation/phase PASS. Correct all actionable findings together and retain the actual container result or an explicitly authorized disposition. No optional suggestions.

The cohesive foundation batch was suitable for review; runtime, tests and deployment/checker consumers could be traced together. Existing verification-contract guidance was sufficient, but two lessons are observable: clean-environment bootstrap must be checked when CI adds stricter installer flags (F1), and a boundary guard's mutation probes must use the repository's real identifiers and actual source encodings/locations (F2–F4). Candidate narrow scope: future boundary-checker reviews should include real crate names, every local Cargo target root and Rust raw-string SQL forms. This is evidence-backed feedback, not authorization to edit instructions.

## AS-P02-B1-N2 — correction and container recheck

- Date/workflow: 2026-11-16, `implement-astra-reviewed-2-3`, p02, AS-P02-B1-R2; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`, requested Astra/high, independent retained context; actual model/effort and context telemetry unavailable. Checkpoint saved; no workers or implementation/design edits.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`; start/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Digest: `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`; exact exclusions `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/phases/p02/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 3, AS-C001/r1 and AS-C002/r1; read at start, byte-compared with [R2 copy](logs/AS-P02-B1-R2.coordination.md) before return, unchanged. Accepted p01 contract/architecture evidence remains valid for unaffected scope.

### Correction coverage and evidence

Independently reviewed corrected requirements lock, checker/test code, README boundary clarification, container runner's readiness delta, and TASK's finding-to-assertion mapping. All 19 Python versions remain pinned; 474 release-artifact hashes are now declared. Inspected the retained clean Python3.13 venv install under the exact `--require-hashes` mode and its 12 contract-test successes. CI's clean bootstrap no longer relies on an already populated environment.

F2 now checks `open_bots`, `openbot` and `open-bots` resolved identities and actual Rust imports/extern aliases. F3 rejects additional local packages and Cargo targets outside audited `src/`, making its documented single-package policy consistent with the scanner. F4 extracts complete ordinary/raw strings and tests quoted/schema-qualified foreign references at multiple raw delimiters, retaining positive owned-table cases. The original five test names are preserved; four new cases bring the inventory to nine. Rechecked these mechanisms against the original R1 probes and original phase02B.2 requirements.

Executed **9/9 boundary tests** and the **actual resolved Cargo metadata/source boundary check**, both PASS. Verified every one of the 22 [runtime input hashes](logs/AS-P02-runtime-inputs.json) against the current tree. Rust source/config/Cargo inputs/assets/migration/Dockerfile are unchanged, so R1's reviewer 14 Rust tests and inspected offline/migration/SQLx metadata/extraction/lint proof remain applicable; did not repeat the unchanged runtime suite.

V1 now has an actual successful release image build and two independent final smoke successes. Reviewed the runner's non-root/read-only/capability restrictions, fresh owned PostgreSQL migration, embedded asset assertion, readiness503 after DB loss with health204, process exit0 and redaction assertions. The initial smoke failed during fixture bootstrap before application launch; the corrected runner waits for PostgreSQL's final TCP listener instead of its temporary Unix-only initialization server. Initial failure and diagnostic/final runs are preserved. Inspected [final smoke evidence](logs/AS-P02-container-smoke-r2.txt) and build output. Image contains only the unchanged compiled binary from the build stage; Python/docs corrections do not change runtime inputs.

Using only `DOCKER_CONTEXT=colima-as-attribution-p02`, independently inspected image `sha256:ff8df74f600c431b8234111731d3985f49caa50c3bd5b7f00b1cdce7f4554a5c`, arm64, configured user attribution:attribution; ran a disposable read-only/no-network `id -u` container and observed **10001**. It self-removed; no default Docker context or parent-owned runtime was altered. [Reviewer focused output](logs/AS-P02-B1-R2-reviewer-focused.txt) retains these checks.

Validated [R2 manifest](logs/AS-P02-B1-R2.evidence.json), current artifact identity, coordination, HEAD and whitespace; [final check log](logs/AS-P02-B1-R2-reviewer-checks.txt). Manifest retains the historical failed smoke and its successful correction, not an unresolved application failure. Local Linux arm64 image execution is proved; remote hosted CI and other architectures are not claimed. No unrelated root checks were run.

### Outcome and process

**PASS for p02 foundation batch digest `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`**. F1, F2, F3 and F4 are FIXED. V1 is SATISFIED by execution evidence; no waiver or deferral was needed. No new findings or optional suggestions. This is foundation acceptance only: p03+ business/identity behavior and the final full-plan gate remain pending.

The combined correction delta was appropriately scoped. Fresh environment evidence resolved the CI mismatch, and actual container proof resolved the prior gap without weakening assertions. The existing R1 instruction-feedback candidates remain supported; no additional instruction change is warranted by this recheck.
