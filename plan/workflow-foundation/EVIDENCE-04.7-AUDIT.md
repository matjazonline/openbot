# 04.7 bounded actor-audit correction evidence

Implementation checkpoint only; root owns acceptance. Affected expansion independently
PASS in `/private/tmp/workflow-04.7-audit-expansion-check.md`; full04.7 remains unverified.
No04.8 work. Preserve baseline `655aaaba3132c561809f97fd25c89f600bca1fa9` and all
pre-existing modified/untracked files; pre-correction72file snapshot lives in
`/private/tmp/workflow-04.7-resume-provider-frozen/`.

Changed files in this assignment:

- New `migrations/20260930184000_workflow_action_audit_commands.sql`: locked invariant
  preflight, retained partial uniqueness for other kinds, unique command audit reference,
  exact deferred command/audit binding in both directions, immutable reconciliation
  events, and exact execution equality in the existing reopen guard. No writer change.
- New `src/adapters/persistence/workflow/action_reconciliation_audit_tests.rs`, wired
  through the existing `action_reconciliation_receipt_tests.rs` child module.
- `EVIDENCE-04.7-GAPS.md`: actual passing provider/audit evidence, explicit remaining gaps.
- This checkpoint record. Root's concurrent PROGRESS edit is outside this assignment.

Exact retained PG identity verified: database workflow_admission, user mac03,
data `/private/tmp/workflow-admission-pg-e3aa`, port55439, max_connections200.
Applied184000 migration SHA256
`64b34421dabb22c5585c86756f3f28ef94e584450f887e5e6b4eafdb210f3ba8` is now immutable.
All earlier migration files remain unchanged. Migration log:
`/private/tmp/workflow-04.7-audit-migrate.log`; catalog evidence:
`/private/tmp/workflow-04.7-audit-schema.log`.

Exact commands and results:

```
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx migrate run
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests
cargo fmt --all
cargo fmt --all -- --check
git diff --check
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=false RUST_MIN_STACK=2097152 cargo sqlx prepare -- --all-targets
```

Migration PASS. Original seven provider/receipt tests PASS7/0 in
`/private/tmp/workflow-04.7-audit-provider.log`; after focused tests, combined PASS10/0
(2290filtered) in `/private/tmp/workflow-04.7-audit-focused.log`. New tests exercise
real same-key and distinct-key contention, later new command audits, exact replay
without run/fact/command/event changes, orphan/NULL audit, exact actor/kind scope,
unique command audit reference, UPDATE/DELETE/kind laundering, and retained other-kind
uniqueness/NULL semantics. fmt/check and whitespace PASS. SQLx prepare PASS21.25s;
no .sqlx changes. No bypass, backfill, bound raise, stage, commit, deploy or new agents.

Remaining affected audit obligations: populated legitimate-history byte preservation/
expired-proof replay and malformed-history migration rejection; wrong company/run/
execution/refusal linkage; exact deferred reopen/historical witness/ordinary retry;
deferred full rollback with real provider effect retained. Full original04.7 matrix,
broader stock2MiB affected suites, locked offline all-target check, strict Clippy,
dedicated fresh-schema inspection, SQLx prepare --check and graft refresh/full independent
actual-code integration review remain pending. Focused tests do not replace those gates.

Freeze/ownership: no edits during subsequent Astra review. All command sessions drained;
retained PG remains running for root/next owner's explicit transfer. Audit-only frozen
files and hashes are in `/private/tmp/workflow-04.7-audit-frozen/`; independent review
must compare the pre-correction snapshot to this checkpoint, not assume HEAD captures
the pre-existing untracked implementation. Root alone updates PROGRESS/RESUME/acceptance.

Worker `/root/reconciliation_audit_check`, verified UUID01a0f44f-73f6-7973-a662-41aa3f1681ef.
Implementation startup depth0 97,207/258,400=37.62%@21:59:02.921Z; milestone
119,772/258,400=46.35%@22:05:06.970Z, usage/window sources. Fresh final sample returned
to root after evidence/freeze. No waiver; bounded return before50% required.
