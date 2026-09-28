# 02.6 — Admission snapshots

Original phase02 publication item4, with shared EXPANSION.md contracts. Replace the
structural-only version admission seam with required WorkflowBindings selection:
logical binding ID + source/input/idempotency, never caller-supplied parameters.
PreparedAdmission retains Arc<ConfiguredBinding>, hence exact revision, validated
parameters, resource IDs and immutable PublishedBundle (including limits/dependencies).
Service authorizes before reads, validates returned scope, bounds aggregate input/params,
validates input schema, and hands one immutable command to the existing atomic owner.

Replay selection returns the previously captured binding when logical binding/key match,
including after deactivation/archive; reauthorize every request. Admission equivalence
uses company/key + logical binding + input + association + stable trigger, not current
configuration. Commit must close selection/activation/revocation races; stale new-run
selection fails without writes. Phase03 owns SQL/real races/history boundary, not this
library change. No second run/config owner or alternate admission service.

Current resource/secret use remains adapter-owned: snapshots contain only RuntimeResourceId;
ResourceDirectory inspects current non-secret facts, never credentials. Every actual use
must reauthorize, enforce revocation and resolve current secrets at dispatch through phase04;
admission/activation readiness cannot grant future use. No credential cache is introduced.

Acceptance/checks: existing scope/auth/source/dedup/cancellation/claim tests retained;
new saved-snapshot/revision/resource/bundle independence, schema/aggregate-bound rejection,
replay after binding replacement, changed logical binding conflict, scoped lookup errors.
Existing competing-admission test remains; production races are mandatory in03.
Workflow suite at2MiB, locked offline all-target check/Clippy, fmt/diff, graph refresh,
independent Astra/medium review. Baseline /private/tmp/workflow-02-6-baseline/workflow.

Main verified gpt-6-astra/medium session01a0e460-1518-7cd3-b95b-144348931c7f.
Startup23781/258400=9.20% at2026-09-27T19:38:37.242Z; expansion reconciliation
70864/258400=27.42% at2026-09-27T19:39:58.377Z. Sources token_usage_record.usage /
task_started.model_context_window. All remaining selected scope already expanded in
EXPANSION.md; no new blocking decisions. Existing uncommitted02.1–02.5 preserved.

## Self-audit before independent review

Changed contracts/ports/service, ResourceDirectory use-time contract, language docs and
existing shared tests; new tests/snapshot_cases.rs. Actual diff against baseline inspected:
authorization precedes reads, returned binding scope is verified, private immutable checked
values replace structural-only ownership envelope, no caller parameter override, one atomic
admission owner. Original typed causality/cancel/claim behavior retained. Compiler builder
makes mismatched graph/version envelope impossible (obsolete forged-envelope test removed).
Resource-less structural admission test replaced by missing/wrong slot constructor rejection;
current non-secret readiness tests from02.5 remain. In-memory store simulates saved selection,
stale-head rollback and original concurrent admissions; not production race evidence.
Diagnostic is structured rather than Display; corrected initial test decoder import and diagnostic
message mapping compile failures before final gates. Full SQL/resource-use enforcement remains
03/04 as scoped, not claimed complete. All changes remain uncommitted.

First runnable suite117pass/24fail: shared legacy fixture omitted schemas that full
publication compilation requires. Corrected shared fixture to explicit unconstrained
schemas and bounded output-schema literal. New snapshot tests already passed; rerun
all gates after correction, no acceptance claimed from the failed run.

Final gates PASS:141workflow tests2MiB, locked offline all-target check and Clippy-Dwarnings.
Logs /private/tmp/workflow-02-6-{tests-verified,check-verified,clippy-verified,fmt,diff,graft}.log.
Independent review pending; stable pre-point tree /private/tmp/workflow-02-6-baseline/workflow.

## Independent review — PASS

/root/review_02_6, verified session01a0e46b-f0f3-7540-a1f8-0d4be52b873f,
Astra/medium reviewed actual pre-point diff, callers, immutable bundle/config integration,
authorization/schema/bounds/replay/stale-selection and existing causality/cancel/claim tests.
No actionable findings; all02.6 library criteria and02.4/02.5 integration accepted.
Production transaction/races03 and actual credential/revocation enforcement04 remain scoped
future gates. Parent final measurement56397/258400=21.83% at2026-09-27T19:52:15.177Z,
token_count.info.last_token_usage/token_count.info.model_context_window. No reviewer edits.
Main final point measurement116282/258400=45.00% at2026-09-27T19:52:22.434Z,
token_usage_record.usage/task_started.model_context_window. Natural early handoff before02.7,
no50% threshold claim. No remaining checks for02.6 library scope, no resource cleanup.
