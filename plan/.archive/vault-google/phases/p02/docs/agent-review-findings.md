# p02 independent findings

## VG-P02-F1 — config-scoped uniqueness can duplicate one provider authorization

- Date/workflow/round: 2026-11-07; implement-astra-reviewed-2-2; [VG-P02-R1](agent-review-notes.md#vg-p02-r1), whole p02 B1 architecture gate VG-P02-D1/r1. [Task](TASK.md), [review](REVIEW.md).
- Reviewer: /root/reviewer, requested gpt-6-astra/high, actual runtime settings not separately exposed; independent context, no authorship. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`, detached; start/base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`; source/prerequisite digest `48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81`.
- Classification: actionable defect, **P2**, architecture / persistent identity. Originates in proposed gate decision, not code implementation.
- Evidence: D1/r1 point 2 and finalized schema detail (TASK.md:42) scope canonical authorization uniqueness by owner/provider/**config**. Phase01A requires one Google authorization per local owner, Google subject, and OAuth client. The accepted Google adapter key already hashes subject+client (`src/adapters/integrations/google_credentials.rs:51`); registry registration permits different AuthConfigIds to name the same client (`src/application/integrations/registry.rs:38`). Two code-owned app registrations with distinct config labels but the same actual OAuth client therefore allow two rows with the same canonical Google authorization key. They would later have different row locks and could refresh the same authorization concurrently, contrary to README's shared credential record invariant.
- Required correction: make persistence uniqueness follow the adapter's canonical authorization identity without allowing a config-label alias to split it, or establish and enforce an equally strong configuration invariant that rules this out. Preserve separate provider/config identity in AAD and immutable account identity. Add a database case using different config IDs for the same client/subject and a case proving distinct true client identities remain independent. This does not authorize implementing OAuth/refresh.
- Instruction lesson: original requirement is already explicit; invariant review must distinguish configuration labels from provider-defined authorization identity. No instruction changes authorized.
- Resolution: **open**, 2026-11-07.

## VG-P02-F2 — teardown retry protocol leaves poison intents immediately eligible

- Date/workflow/round/settings/worktree/base/digest: same as VG-P02-F1; architecture VG-P02-D1/r1, [VG-P02-R1](agent-review-notes.md#vg-p02-r1).
- Classification: actionable defect, **P2**, architecture / recovery and fairness. Originates in proposed gate decision.
- Evidence: finalized drain design (TASK.md:44) selects a bounded SKIP LOCKED batch, performs cleanup+completion transactionally, rolls back on failure/cancellation, and relies on fixed poll/backoff. It does not persist per-intent failure eligibility or isolate a failing oldest batch. After rollback those same rows remain the next eligible batch; repeated row-specific failure can monopolize every poll and indefinitely delay other owners' teardown. A process-wide sleep reduces polling rate but does not advance or exclude the poison rows. This misses `src/application/AGENTS.md:58–66`, including its required two-iteration poison-batch test, and the plan's bounded restart/retry recovery criterion.
- Required correction: specify how row-specific cleanup failures leave a durable retry/backoff or terminal/manual-recovery state while retaining teardown intent and allowing healthy work to progress. Define cancellation/crash behavior separately from recorded failures and add the required full poison-batch, two-iteration no-clock-advance test plus a healthy-intent progress case. Preserve immediate account denial and never mark failed cleanup complete.
- Instruction lesson: existing queue guidance already covers this; no new instruction warranted. Fixed worker polling is not evidence of per-record retry isolation.
- Resolution: **open**, 2026-11-07.

## VG-P02-F1 — architecture resolution 2026-11-07

**Fixed in decision**, independently rechecked in VG-P02-R2 against VG-P02-D1/r2 ([immutable decision](logs/VG-P02-R2-decision-r2.md)). Uniqueness now uses owner namespace/subject, provider and canonical authorization key, without config-label partitioning. Required cases distinguish config aliases from actual client identity, owner and namespace. The existing immutable config AAD remains separate. Source digest is unchanged `48c978f0dbbcaf84db7b4901eac45b6aad1efd33a66f54ed65aef0c83f64de81`; code/DB enforcement remains for implementation review.

## VG-P02-F2 — architecture resolution 2026-11-07

**Fixed in decision**, independently rechecked in VG-P02-R2 against VG-P02-D1/r2. Each intent has its own transaction and cleanup savepoint, with committed per-record attempts and backoff after cleanup failure, terminal failed state after eight attempts, bounded polling/locking/statements, and real poison-batch restart/terminal/concurrent-drainer tests. Cancellation and crash roll back actual work; successful cleanup and completion commit together. Immediate owner retirement/disconnection remains durable, so terminal cleanup failure cannot reactivate credentials. Implementation and failure-isolation tests remain unverified until batch review. No open architecture findings.

## VG-P02-F3 — rendered account deletion sends no form body

- Date/workflow/round: 2026-11-07; implement-astra-reviewed-2-2; [VG-P02-R3](agent-review-notes.md#vg-p02-r3), whole p02 B1 implementation review. [Task](TASK.md), [review](REVIEW.md); gate VG-P02-D1/r2.
- Reviewer /root/reviewer, requested gpt-6-astra/high, actual runtime settings not separately exposed; independent, no source authorship. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`; start/base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`; digest `8fbbd3aa4eac6d69e05d57420e41345875ffe259b4ca5dcbec5e8815d3ad2773`.
- Classification: actionable defect, **P2**, user-facing correctness / management integration. Implementation deviation from phase02C, not a gated design defect.
- Evidence: `src/adapters/http/pages/vault.rs:58` renders an hx-delete form with hidden expected_revision; bundled HTMX 2.0.4 defaults DELETE to URL parameters. `src/adapters/http/routes/ui_vault.rs:145` requires Form<RevisionForm> from the body. Clicking Remove account therefore requests DELETE /ui/profile/vault/<id>?expected_revision=1 with an empty body, and extraction returns 422 before the gateway can delete anything.
- Reproduction: [browser script](logs/VG-P02-R3-delete-browser.cjs) extracts the actual form markup and loads the actual vendored HTMX; [captured request](logs/VG-P02-R3-delete-browser.log) confirms query revision/body null. [Axum extractor harness](logs/VG-P02-R3-delete-extractor.rs), [result](logs/VG-P02-R3-delete-extractor.log) confirms 422 with that exact request encoding. No production server or external request used. Bundled Playwright browser was unavailable; [failure](logs/VG-P02-R3-delete-browser-unavailable.log) retained, rerun succeeded using installed Chrome.
- Required correction: align the rendered DELETE request and handler revision extraction while preserving strict revision/owner/CSRF validation. Add a routed request test using the actual HTMX DELETE encoding (and preferably the rendered-form browser case); current direct remove(State, Path, Form) invocation bypasses the mismatch.
- Instruction lesson: exercise browser-generated method/encoding at the route boundary for destructive forms; constructing extractors directly cannot establish HTTP usability. No skill/AGENTS changes authorized.
- Resolution: **open**, 2026-11-07.

## VG-P02-F4 — managed cache writer can change consent without authorization revision

- Date/workflow/round/settings/worktree/base/digest: same VG-P02-R3 artifact as F3, gate VG-P02-D1/r2.
- Classification: actionable defect, **P2**, correctness / authorization revision invariant. Implementation deviation from phase01D and accepted version table.
- Evidence: `src/adapters/persistence/integrations/accounts.rs:131–140` validates only that a new managed payload has the same ExternalIdentity, then always uses CredentialChange::Refresh, preserving authorization_revision. It never reads or compares old authorization facts. The Google validator's identity contains subject/client, not scopes, so changing scopes from read to read+send passes the equality check and would persist changed consent under the old authorization revision. Scope removal has the same problem. This is a real new production private writer, currently called only from tests; no current OAuth/execution exposure is claimed.
- Verification: whole method inspection plus [exact-source adapter/transition harness](logs/VG-P02-R3-scope-projection.rs), [result](logs/VG-P02-R3-scope-projection.log), proving both scope lists validate to identical identity and Refresh preserves authorization. This harness does not invoke the private database method; the missing comparison is directly visible in its complete implementation.
- Required correction: ensure production managed writes classified as routine cache refresh cannot change authorization facts without the appropriate authorization-revision transition. Alternatively keep a fixture-only cache writer out of production until the later lifecycle phase supplies that invariant. Add a regression demonstrating consent/scope changes cannot retain a valid old authorization snapshot; keep ordinary cache refresh authorization-stable. Do not implement OAuth/network refresh as part of this fix.
- Instruction lesson: compare every fact represented by an authorization revision before choosing a cache-only transition; stable external identity alone is insufficient. Existing phase01 requirements already state this.
- Resolution: **open**, 2026-11-07.

## VG-P02-F5 — SQL JSON constraints accept missing/null required values

- Date/workflow/round/settings/worktree/base/digest: same VG-P02-R3 artifact as F3, gate VG-P02-D1/r2.
- Classification: actionable defect, **P2**, persistence invariants / verification. Implementation deviation from schema-boundary rules.
- Evidence: `migrations/20260817000000_init_schema.sql:12516–12521` does not require metadata label presence; for `{}` its label predicates evaluate to SQL NULL and CHECK accepts the row. The external-identity check beginning at line12530 similarly accepts JSON authorization_key:null with a non-null separate authorization_key column because equality evaluates to NULL. Both shapes violate the bounded typed decoder and cannot be loaded as valid account metadata/identity; one accepted malformed row can abort its owner list and operator rotation inventory.
- Reproduction: [rollback-only SQL](logs/VG-P02-R3-sql-repro.sql), [result](logs/VG-P02-R3-sql-repro.log) against the explicit disposable phase DB: both INSERTs succeeded and were rolled back. No persistent fixture/user state changed. SQL null-label was separately observed to fail; the defect is missing label and nullable identity-key comparison, not all null cases.
- Required correction: make required JSON field/type/equality conditions reject SQL NULL, and extend DB constraint tests for missing required fields and null/wrong-type identity keys. Verify fresh migration and regenerate SQLx metadata as required; preserve fallible decoding as defense in depth.
- Instruction lesson: PostgreSQL CHECK accepts NULL; required JSON paths need explicit presence/type predicates or a whole-predicate true check. Existing persistence instructions require DB invariants and adversarial cases; no instruction edit authorized.
- Resolution: **open**, 2026-11-07.

## VG-P02-F3 / VG-P02-F4 / VG-P02-F5 — design follow-up 2026-11-07

VG-P02-R4 passes correction decision VG-P02-D1/r3 ([retained revision](logs/VG-P02-R4-decision-r3.md)). Required private cache-update authorization comparison, Query-only DELETE input and SQL required/type checks address the proposed remedies. These three implementation findings remain **open** until code and required regressions are independently rechecked. Source remains `8fbbd3aa4eac6d69e05d57420e41345875ffe259b4ca5dcbec5e8815d3ad2773`.

## VG-P02-F3 — resolution 2026-11-07

**Fixed**, independently rechecked in VG-P02-R5 against digest `e74fd0b7bb6eeec83c21a0da777a92f8f51f2f102894b517b312b10fd87fe601`, same worktree/base/HEAD. DELETE now extracts Query<RevisionInput>; disconnect remains Form<RevisionInput>, without fallback decoders. Independently listed and ran the real-session/origin/body/owner test, including actual empty-body authenticated DELETE with query revision and persisted removal assertion: [list](logs/VG-P02-R5-management-list.log), [run](logs/VG-P02-R5-management-run.log), 1 passed. This matches the actual rendered-form HTMX encoding reproduced in R3.

## VG-P02-F4 — resolution 2026-11-07

**Fixed**, independently rechecked in VG-P02-R5 against the same corrected digest. Required private CredentialCacheUpdate validation has no default; static adapter explicitly denies, Google compares decoded subject/client and scope sets, workspace fixture compares its own decoded permission sets. write_managed reads/decrypts prior ciphertext under its existing owner/account lock and invokes the required comparison before sealing/writing. Denials preserve ciphertext and all versions; equivalent reordered scopes and token-only updates keep authorization revision stable. Independently listed/ran the database regression for both registered schemas: [cache test list](logs/VG-P02-R5-cache-list.log), [cache test run](logs/VG-P02-R5-cache-run.log). No unresolved acceptance defect remains from this finding; future provider/network lifecycle remains outside scope.

## VG-P02-F5 — resolution 2026-11-07

**Fixed**, independently rechecked in VG-P02-R5 against the same corrected digest. SQL requires label presence and string canonical identity keys, so absent paths/null equality cannot make the checks pass. Independently ran rollback-only metadata missing/null and managed null-identity INSERT regressions that require SQLSTATE 23514 and avoid masking by uniqueness/immutability. Included in the 2 passing cache_update_tests selection ([run](logs/VG-P02-R5-cache-run.log)). Author fresh migration/SQLx preparation evidence inspected. Original findings and reproductions remain retained. All p02 findings now resolved.
