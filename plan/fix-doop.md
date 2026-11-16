# Fix Doop verification and private acceptance

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: deferred follow-up, 2026-11-16. The user separated this work from Plan03 so that a passing
principal-addressing implementation can be published and work can continue with
[04-root-unit](04-root-unit/README.md). This plan does not authorize skipping checks or claiming
that Doop acceptance passes. Execute it separately; do not interrupt the authorized Plan04 sequence.

## Current evidence

| Issue | Observed result | Retained evidence |
| --- | --- | --- |
| Native lifecycle helper reaches its 80-second deadline | Actual Ubuntu private acceptance stopped after company 1's disabled stage; the native helper received SIGTERM before completing both companies. A local unprofiled HTTP diagnostic completed just under the same deadline with about 24 seconds of CPU. | [Linux helper output](03-principal-addresses/phases/p04/docs/logs/R21-acceptance-all-disk-lifecycle-record-helper-failure.txt.txt), [local timing](03-principal-addresses/phases/p04/docs/logs/R23-native-unprofiled-cpu-and-db.txt) |
| Pinned service browser assertions are intermittent | The identical 2,728-test service suite passed in R19; R20 had 2,726 passes and two failures: drawing reply input was empty, and a rendered private image pixel was white instead of green. The precise cause is unproved. | [R19 service success](03-principal-addresses/phases/p04/docs/logs/R19-service-gate-5.log.txt), [R20 failures](03-principal-addresses/phases/p04/docs/logs/R20-service-failure-plain.txt) |
| Complete private acceptance is unfinished | Current corrected-source acceptance has not passed all four required profiles: all/disk, all/bucket, e2e/disk and e2e/bucket. R21 stopped during the first profile's lifecycle helper; its cleanup receipt confirmed removal. | [Acceptance result](03-principal-addresses/phases/p04/docs/logs/R21-acceptance-acceptance.json.txt), [report](03-principal-addresses/phases/p04/docs/logs/R21-acceptance-all-disk-report.json.txt), [cleanup receipt](03-principal-addresses/phases/p04/docs/logs/R21-acceptance-all-disk-cleanup-receipt.json.txt) |

The catalog conversion correction and Chrome temporary-path correction are already implemented.
Catalog validation is reused only within the same row conversion, preserving strict rejection,
ordinary quarantine and current authority. Host Chrome receives child-only `TMPDIR=/tmp`; its
profile, HOME, sandbox, TLS settings and process ledger retain their existing ownership. Actual
Ubuntu reproduced Chrome's fatal long socket path and launched successfully with a short temporary
base. Do not restore the removed standalone browser preflight: independent review demonstrated
that it lacked the private probe supervisor's recovery and bounded shutdown guarantees.

The managed service remains pinned to revision `01584b34f6cadcf21767127280cc6e320a6d748a`,
fingerprint `351a2180b4ca8cf7489d21cbdc3aa3ce7d275fc5b08ba56eb045592a43e34e1f`, Bun `1.3.10`
and its reviewed lockfile/patch in [source.json](../integrations/doop/source.json).

## Batch 1: diagnose and fix lifecycle progress

1. Start from `scripts/doop-test/openbot-helper.mjs`,
   `src/adapters/persistence/doop_live_lifecycle_tests.rs` and
   `src/adapters/persistence/doop_live_workflow.rs`. Trace callers and preserve private stdin,
   output limits, exact helper markers and cleanup ownership.
2. Measure the unmodified helper under the real Linux PostgreSQL/TLS/image setup. Record wall time,
   CPU time and stages without exposing credentials. Stack sampling perturbed a local provenance
   deadline, so that sampled failure is diagnostic evidence, not an acceptance result.
3. Investigate the test driver's full `run_view` polling every 20 ms. It repeatedly decodes and
   validates immutable provenance while waiting for terminal state. A candidate correction is the
   existing test-harness pattern: poll `workflow_runs.state` scoped by company and run, then obtain
   the original complete, authenticated `RunView` once terminal. The unreviewed prototype was
   removed from implementation and is retained only as a [proposal patch](03-principal-addresses/phases/p04/docs/logs/R24-deferred-doop-polling.patch.txt).
4. Preserve the 25-second driver bound, 80-second helper deadline, real workers, cancellation guard,
   bounded joins, final provenance validation, and both companies' current-key, revoked-key,
   disabled and disconnected assertions. Do not add a cross-read authority cache or increase limits.

Focused checks: input/output boundary and helper protocol tests, affected native Doop tests, exact
two-company lifecycle execution, formatting, offline compilation, Clippy and SQLx regeneration for
any query change. A local HTTP diagnostic cannot substitute for actual private HTTPS acceptance.

## Batch 2: establish the service browser failure cause

1. Reproduce the two service assertions with the pinned source, lockfile, Bun, Chrome, clean
   environment and worker count. Preserve both the R19 pass and R20 failures.
2. Establish whether the cause is fixture isolation, focus/readiness, rendering completion or a
   service defect. Test the unchanged baseline under equivalent contention before attributing it.
3. Fix the responsible layer without weakening the text, pixel, authorization or timing assertions.
   Do not skip tests, enable retries to hide failures or declare the entire service flaky.
4. If the packaged service needs a change, explicitly regenerate and review its managed patch,
   fingerprint and dependent source/image expectations. Keep dependency revisions pinned.

Focused checks: both failing browser files and adjacent interaction/rendering cases, followed by
the exact pinned service gate. Preserve the sandbox and actual private image/font assertions.

## Batch 3: integration and final verification

1. Run the current shared `scripts/check-doop.py` gate against an invocation-owned PostgreSQL
   database and the real installed sandboxed Chrome. The service gate and actual image build must
   succeed; all four private acceptance profiles must execute their required assertions.
2. Retain service outputs, exact source/image/helper identities, each executed report and confirmed
   cleanup receipts. A missing browser, missing helper, unexecuted assertion or unconfirmed cleanup
   remains a failure or an explicit gap.
3. Run the repository's full verification gate once after all changes: formatting, patch whitespace,
   locked offline all-target compilation, Clippy, fresh migrations, SQLx metadata, source reachability,
   database-backed tests, network isolation, browser protocols, stack and lease budgets. Use focused
   checks during each batch and after corrections; keep existing thresholds.
4. Obtain independent source and evidence review, refresh the Trail graph, validate retained
   manifests and record all unresolved or unrelated failures explicitly before claiming completion.

Download authenticated GitHub log archives with `/OWNER/REPO/suites/CHECK_SUITE_ID/logs`, taking
`check_suite_id` from the run API. Artifact downloads use `/OWNER/REPO/actions/runs/RUN/artifacts/ID`.
Match downloaded artifacts to API size/digest, retain only task evidence, and never extract browser
cookies or tokens. The existing Plan03 phase records retain all earlier investigations and reviews.
