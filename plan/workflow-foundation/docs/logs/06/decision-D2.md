# 06-D2 — usage measurement and priced analytics (06B1)

2026-10-19, implement-astra-reviewed-2.1. Proposed gate; no dependent implementation.
No backward compatibility: edit squashed init, recreate only task test databases;
no backfills, dual reads/writes for legacy data or additive migrations.

## User requirements and dependency boundary

Original phase06 remains required; user's2026-10-19 revision in phase06 plan requires
agent/workflow model-call and token analytics, optional per-million provider/model
prices for displayed calculations, optional workflow token cap with manual approval
to continue, and explicitly NO monetary caps.

The added usage feature makes06B too broad for one review. Stable cohesive batches:
-06B1: durable per-call usage measurement in existing Rig execution, company-scoped
 pricing and model/workflow analytics, including domain/application/SQL/HTTP/tests.
-06B2: workflow Rig execution, root token allowance/approval continuation, current
 capability authority and saved action calls (separate architecture gate before edits).
-06B3: explicit context/memory/classification/agent-choice handlers.
-06C: remaining caller/skill migration, alternate runtime removal and final phase06 gate.
No later batch starts dependent implementation before prior acceptance. This division
is not a waiver: workflow model calls and cap continuations remain incomplete until06B2.
Phase08 still owns ingress replacement. User-requested analytics UI belongs here,
rather than being deferred to the general phase09 UI.

## Owners and persistence

Existing Rig RunCheckpoint.reservations and turns are the authoritative request and
response identities. Reserve before provider I/O; commit model turn before tools or
next generation. Reuse these boundaries. No second job, attempt, lease or request queue.

Add model_usage_calls as a payload-free projection written in the SAME transaction
as checkpoint reservation/settlement. Columns: company, request UUID (unique within
company), task, agent, optional exact workflow execution/run/root/workflow/version
attribution, provider/model, request timestamp, reserved input/output tokens, nullable
settled input/output and usage_source, settlement timestamp. Store no prompts, model
outputs, tool arguments or credentials. Request identity and reservation facts are
immutable; settlement is single-assignment, with exact replay accepted and differing
replay refused. SQL constraints enforce coherent NULL/settled shapes, nonnegative
bounded counters, positive reservation total, tenant composite task/agent/workflow
provenance, and single settlement. Do not use ON CONFLICT to overwrite existing truth.
Company/task deletion follows existing checkpoint retention (cascade). Agent provenance
uses existing composite agents(company_id,id), as task_harness_runs already does;
agent deletion follows the existing referenced-checkpoint restriction, without silently
reassigning or anonymizing usage. Workflow foreign keys preserve exact tenant/run/
execution/version provenance; standalone calls carry NULL for all workflow columns.

Legacy HarnessRunStore writes the projection from its existing checkpoint transaction.
Project only each reservation once and its matching committed turn once. SQL errors
roll back BOTH projection and checkpoint, preventing successful output before durable
usage. The writer is an internal adapter helper reusable by06B2's workflow checkpoint
adapter; workflow attribution must be derived from the exact workflow job/execution,
never accepted from HTTP/model arguments.06B1 DB tests exercise valid workflow-linked
usage against real admitted/claimed workflow fixtures; no workflow model-handler claim.

Unknown calls retain reserved estimates across recovery. A row with no settlement
is an unconfirmed request, NOT proof the provider was actually entered or charged.
Provider-reported, estimated and mixed settled usage retain existing TokenUsageSource.
Do not reclassify Mixed as fully reported. Analytics display completed and unconfirmed
counts separately. Duplicate checkpoint writes never inflate calls/tokens.
Both AgentRunner production construction sites (thread/dispatch.rs:497 and1014)
attach HarnessRunStore and claimed-task context.06B1 instruments that durable path.
Direct in-memory Rig execution and alternate-runtime classifier calls remain explicit
06C migration obligations: before phase06 acceptance every production model entry
must persist usage or be removed.06B1 does not claim full coverage of those paths.

## Pricing and display

Company-owned model_prices keyed by(company,provider,model); optional separate input
and output USD prices per million tokens, both required to enable a complete estimate.
Use exact fixed-point integers (micro-USD per million), checked wide arithmetic,
nonnegative bounded values and strict decimal parsing. Zero is a valid free rate;
missing is distinct. No floating-point monetary arithmetic. No currency conversion.
Prices are current reporting rates, not spend authorization or a provider billing
promise. Editing rates recalculates historical displayed estimates, explicitly labeled
"Estimated cost at current configured rates". No monetary reservations/caps.
Unpriced models still run and appear with cost unavailable. Mixed reports show priced
subtotal and unpriced-call count rather than presenting a partial sum as complete cost.
Estimated/unknown token contributions remain identified when showing cost calculations.

## Application/HTTP boundary

Add a cohesive application usage/pricing port/use-case, concrete Postgres adapter,
and a company-admin usage page using existing authenticated company scope. Company
owners/admins can read company-wide analytics and manage prices; ordinary users cannot
use aggregate access to infer private workflow activity. Recheck current membership
in the persistence read/write transaction; no raw pool in route orchestration.
Use existing CSRF and Origin policies. Route paths are company-scoped; foreign company,
workflow or agent identifiers are refused without exposing existence. Prices require
expected revision on update/delete so competing edits cannot silently overwrite.

UI groups by provider/model or workflow (with standalone agent activity labeled),
showing calls, completed/unconfirmed counts, input/output tokens, usage confidence,
and estimated cost. Include an optional agent/workflow filter using exact tenant
ownership. Bound time range (default30days, maximum90), group cardinality/pagination,
and scanned records with explicit truncation status; never silently show a bounded
subset as complete totals. A read-only JSON endpoint exposes the same authorized
projection. Pricing form uses existing theme controls, accessible labels, native
validation, disabled/pending submit feedback, escaped values and private/no-store
responses. No SSE/live-update protocol is needed; normal navigation/refresh is enough.

## Manual continuation obligation for06B2 (not implemented by06B1)

Token allowance is optional and independent of prices. Before additional model work,
account for durable reservations and settled usage across root descendants. Exhaustion
parks with an admin decision; approval adds explicit token allowance and resumes saved
state without resetting counters/repeating effects. Denial/deadline/cancel never reopens.
Existing decisions currently UNIQUE per execution and action approvals allow only one
per step; agent steps require multiple sequential waits.06B2 must revise this contract,
including exact decision/wait identity, immutable history, attempt uniqueness/retry
accounting and current authorization, with competing claim/approval/cancel tests.
Do not merely lift uniqueness or weaken legacy task guards. No06B2 schema changes underD2.

## Verification

06B1 focused proof: production Rig reserve/settlement writes observable rows; duplicate
replay, changed replay, crash/unknown, partial usage, repair calls, two competing writers,
SQL fault rollback, cross-company/task/agent/workflow provenance rejection; current
membership revocation and price-edit CAS races. Analytics groups/filters count correctly,
bounds/truncation honest, cost exact at boundaries and missing/zero prices distinguished.
HTTP unauthorized/foreign ids, CSRF mutation, escaping, invalid rates and conflict feedback.
Existing harness, compiler/publication and affected HTTP tests at2MiB. fmt/diff, locked
offline all-target check/strictClippy; fresh migration/schema and SQLx prepare/check.
Record all commands, results and logs; no claims for unavailable browser checks.
Final phase06 full all-target database suite, stock-stack/lease gates and graph refresh
remain required after06B2/06B3/06C. No resource limits raised by06B1.
