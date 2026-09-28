# 02.7 — Lifecycle commands

Original phase02 publication item5 and EXPANSION.md are authoritative. Root accepted expansion
and this application scope on2026-09-27. Normalized production SQL/transactions remain03.1;
provider-use secrets/revocation remain04. No completion of production activation/CAS is claimed.

DefinitionService owns bounded save (invalid source retained), shared validate/freeze, idempotent
publication and archive. CompanyWorkflowDraft remains the draft owner; optional template origin
distinguishes scratch authoring and survives edits. PublishedCommand retains exact original draft
and immutable bundle. Publication replay reauthorizes and resolves through the atomic port before
returning original content; changed expected revision/version under one key conflicts.

BindingService owns configure/activate/deactivate. BindingStateRevision is distinct from immutable
BindingRevision, changes on every mutation and closes activity ABA. Association remains fixed.
Readiness checked on configuration and activation; deactivation can stop an archived/revoked binding.
Prepared commands have private construction; ports require atomic current authority/selectability/
readiness/CAS/history/audit checks, no default success methods. SQL implementation remains03.1.

Changed: application/workflow/lifecycle/*; templates/contracts.rs and its accessor tests;
authorization operation variants; workflow module export; domain BindingStateRevision export;
language docs. Prior uncommitted02.1–02.6 preserved. Baseline HEAD e5909d6 plus accepted uncommitted
work copied to /private/tmp/workflow-02-7-baseline/workflow and ids.rs. Point diff:
/private/tmp/workflow-02-7-delta.diff. No database/service created, no commit/reset/deploy.

Self-audit: authorization precedes reads; stored scope is checked; immutable saved/publication
objects survive later edits/archive; replay goes through atomic authority port; overflow fails;
inactive archive cannot be reactivated, deactivation preserves snapshots; fixed association prevents
resource rebinding by a stale command. All touched functions <80 lines. Source and fact bounds
reuse existing checks. Initial compile corrected two definition field names and one test import;
first runnable workflow suite passed150 tests at2MiB. Final checks/review pending.

Tests include invalid source retention/source diagnostic, validation without publish, stale edits,
source/title bounds, replay after edits/archive/directory failure, reauthorization, failed writes
leave state unchanged, scope/resource rejection, readiness revocation, immutable binding revisions,
and barrier-synchronized competing edits/publications/version identities/activity-vs-configuration/
archive-vs-publication. These are contract tests, not real database race evidence.

Implementer /root/implement_workflow, verified UUID01a0e471-5955-7cc3-9363-801696e741c0,
runtime turn_context gpt-6-astra/medium. Helper depth0 startup30729/258400=11.89%
at19:57:29.901Z; expansion62529=24.20% at19:58:14.271Z; implementation93854=36.32%
at20:06:02.841Z; audit99745=38.60% at20:08:08.650Z. Sources token_usage_record.usage /
task_started.model_context_window. Runtime log:
/Users/mac03/.codex/sessions/2026/09/27/rollout-2026-09-27T21-57-08-01a0e471-5955-7cc3-9363-801696e741c0.jsonl.

## Independent review and corrections

Reviewer /root/implement_workflow/review_02_7, verified UUID
01a0e47d-cd81-75e1-8cfa-3380e1352ea2, runtime Astra/medium, no edits/delegation.
Initial review found two P2s: source workflow-ID mismatch returned generic NotFound instead
of editor diagnostics; binding contract-test store omitted readiness recheck under its commit
lock. Clippy separately found a test MutexGuard requiring lexical scope before await.

Corrected together: source workflow.identity diagnostic names /workflow_id at its scalar span,
with validation/publication/no-write regressions; binding pure validator is now crate-visible
and reused under the test store lock; entry/resume barriers force resource revocation after
activation preflight and prove commit rejects without revision/activity/commit-count changes.
Deactivation still works. Guard scoped lexically. Focused actual-code review PASS, no additional
findings. One correction round. Final reviewer self-measure79819/258400=30.89% at20:15:53.410Z
(token_usage_record.usage/task_started.model_context_window); parent verified80245=31.05%
at20:16:05.419Z (token_count.info.last_token_usage/token_count.info.model_context_window).

Corrected checks:151workflow tests at2MiB PASS and locked offline all-target check PASS.
Commands and logs:
- SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
  — /private/tmp/workflow-02-7-tests-corrected.log
- SQLX_OFFLINE=true cargo check --locked --offline --all-targets
  — /private/tmp/workflow-02-7-check-corrected.log
- SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  — /private/tmp/workflow-02-7-clippy-corrected.log (final status recorded below)
- cargo fmt --all -- --check; git diff --check; graft build — PASS,
  /private/tmp/workflow-02-7-{fmt,diff,graft}.log.

Fresh implementer audit122251/258400=47.31% at20:16:11.874Z, token_usage_record.usage /
task_started.model_context_window. Natural rotation after02.7, no50% threshold claim.
Graft savings estimates: implementer~3,101,872; reviewer initial~2,520,682 + correction~18,420.
These estimates compare whole-file reads, not measured runtime token savings. No SQL/DB gate
is claimed: production CAS/activation/publication/admission races remain03.1, provider-use04.

Final Clippy-Dwarnings PASS (corrected log,60s); all02.7 library gates now pass, independent
review PASS. No task processes or databases require cleanup. Root acceptance pending; next
ordered point is02.8 representative fixtures, assigned only after root accepts this checkpoint.
