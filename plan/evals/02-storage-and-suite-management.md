# 02 — Storage and suite management

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 01. Add durable, company-qualified suite/revision/run ownership through application
ports and PostgreSQL adapters. Keep the first authoring surface a bounded structured form/JSON
editor and import/export beside the workflow; no separate dataset service or labeling application.

## Batch 02A — Tenant-qualified persistence and authorization

Persist suite drafts/revisions, cases, run manifests, attempts and grading evidence under explicit
company identities. Use purpose-specific transactions and composite tenant foreign keys to bind
suite, workflow, subject, case, run and result. Derive the company/actor from trusted session or job
authority; imported payloads cannot select foreign ownership or credentials.

Apply existing membership and access-level authorization in application use cases for creation,
editing, versioning, execution, cancellation, reads, export and deletion. Suite/run visibility must
be at least as restrictive as all selected source content, fixture data and outcomes. A less
privileged case cannot lower the enclosing report's restriction. Authorization/storage failures
propagate; they do not mean missing data, an empty permitted suite, or successful deletion.

Use revision checks for draft writes, idempotent run admission keys scoped to actor/company and
logical request, and immutable revision contents. Same admission key/different payload conflicts.
Retain run identity/digests needed for comparison even when bulky evidence expires; an expired
artifact is unavailable evidence rather than a passing empty result.

Change the init schema, migrate a fresh disposable database and regenerate `.sqlx` for SQL changes.
Do not add compatibility readers/backfills for old suite or report formats.

**Focused checks:** `eval_storage_`, `eval_authorization_`, and `eval_admission_` database-backed
tests for cross-company relations/CRUD/exports, restricted source data, stale draft writes, and
duplicate admissions. Race two real writers/admitters and assert one accepted logical result.

## Batch 02B — Dataset authoring and safe case capture

Offer case creation/editing, review/version creation, split selection, and validated bounded
import/export using current form, CSRF, escaping and revision patterns. Import is inert data:
reject executable grader code, arbitrary file paths, remote fixture URLs and secret-bearing
connection objects. Export requires the same content authority as reading all included cases.

Mark cases as development/regression or held-out; preserve that split in immutable revisions.
Run failures can be manually promoted into a draft case by an authorized reviewer who selects and
redacts required inputs and supplies the correct expectations. Do not automatically ingest raw
production messages, credential-bearing HTTP/MCP traces or another company's knowledge/history.
Judge-generated expectations require human review before becoming reference labels.

Use synthetic/sanitized repository examples for CI. Actual company datasets stay in authorized
storage, not git. Keep case lineage/source access provenance and explain that running a held-out
case repeatedly during tuning weakens its value as held-out evidence.

**Focused checks:** `eval_dataset_` tests for malformed/oversized imports, duplicate case IDs,
unknown fields, unsafe fixture references, split/version immutability and restricted capture/export.
Browser form fixtures verify escaping, CSRF, stale-write recovery, and accessible errors.

## Exit criteria

Authorized users can create/review a reproducible suite revision. Foreign or insufficiently
privileged users cannot retrieve cases, expected answers, runs or exports by knowing their IDs.
