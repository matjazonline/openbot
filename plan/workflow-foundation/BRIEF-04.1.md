# 04.1 — frozen shared action contract

Original04 Action contract remains authoritative; EXPANSION2026-09-30 reconciles remaining
scope and cross-point ownership. Root accepted the foundation-fragment decomposition before code.
Status: foundation fragment verified by implementer/reviewer; awaiting root acceptance.
The original04.1 integration obligations remain open as detailed below.

Deliverables: application workflow/actions common service with step/direct-tool ingress;
bounded schema-valid frozen operation, canonical digest, stable logical key, immutable policy
context, invocation-scoped approval subject and receipt vocabulary; normalized additive
SQL intent and model tool-call mapping adapter. No dispatch, legacy shim or grant inference.
Existing task_attempts is sole execution-attempt owner. Effect records reference it when04.3
implements dispatch (no new attempt ledger in this fragment). Receipt persistence/settlement
and agent replay integration remain04.5/06; protected dispatch
remains absent pending05. Provider integration04.9–04.11; legacy removal06/08.

Acceptance: shared ingress gives same canonical identity; caller attempt cannot change identity;
argument changes create distinct invocation/new pending decision; same model-call identity cannot
be rebound; invalid schemas/arguments/bounds reject before writes; company/execution relationships
are constrained; competing equivalent prepares persist one intent/mapping; committed preparation
survives adapter restart. Preparation is never a successful effect or approved dispatch.

Discovery references: publication/types.rs:69–97 ApprovedActionPolicy; registry/facts.rs:13–19
ToolContract; compiler/schema.rs:23–82 compile/validate; lease.rs:19–24 WorkflowFence;
persistence/workflow/activation.rs:44–111 run-first activation; domain/workflow/causality.rs:61–64
ActionRef. No full historical source rediscovery. Graft estimate tally recorded at checkpoint.

WorkerUUID01a0f1b9-bd6e-7141-a51c-75e77724ffcf Sol6.1/high startup28052/25840010.86%
09:51:42Z; reviewer/root/actions_contract/review04 UUID01a0f1ba-49f5-7f83-b45c-e92cce5f918f
Astra/medium startup28936/25840011.2%09:52:08Z. Sources usage/runtime capacity.
Worker owns code/this brief/phase04 expansion/taskPG; root owns PROGRESS/RESUME acceptance.
Retained PG must never reset: /private/tmp/workflow-admission-pg-e3aa;
both URLs postgres://mac03@127.0.0.1:55439/workflow_admission. Exact commands/logs follow.

## Implemented foundation and explicit remaining original obligations

Both ActionService entry points share the same freeze path and ActionIntents SQL owner.
Operation identity includes company/run/logical execution/key/target/approved ToolSnapshot
and canonical arguments; context changes conflict with an existing intent rather than grant
dispatch. No attempt enters identity. Model-call ID is immutable within its company/run/execution;
changed arguments need a new invocation and new model-call identity. ApprovalSubject binds its
invocation and argument digest. All preparations remain Unevaluated or ApprovalRequired.
The SQL tables cannot contain approved/dispatched states. Attempts and authoritative effect
receipts have no new persistence owner yet. No provider call or successful tool output is exposed.

Original04.1 common production dispatch and bypass removal are NOT accepted by this fragment.
04.2 owns current authorization/ceiling/target-resource proof and error propagation;04.3–04.8
own fenced dispatch, attempt linkage, receipts, uncertainty, evidence reconciliation and cancel.
04.9 registers local/HTTP tools;04.10 explicitly routes workflow and direct MCP providers through
the service;04.11 message publication/delivery aggregation.06 Rig/checkpoint integration and08
exhaustive legacy removal must replace REPLACEMENT-MAP harness approvals/native tools/legacy
MCP journal callers before claiming identical production grants/effects or removal of bypasses.
Frozen target IDs and company context are content here; use-time access proof remains04.2.
Protected actions remain undispatched until05. No compatibility wrapper was introduced.

Files: new application workflow/actions modules (contracts/freeze/service/tests), persistence
workflow/actions.rs and action_tests.rs; application/persistence module declarations;
activation_tests child registration; additive20260930090000_workflow_action_intents.sql.
Immutable tenant/execution/digest FKs and triggers protect SQL intent/model-call content.
No applied old migration or compiler semantic revision changed; no resource limit raised.

## Criterion evidence and independent review

- Same step/tool preparation + real competing connections: workflow_action_competing_step_and_tool_prepare_one_intent_restart_replays.
- Changed arguments/fresh pending invocation, immutable saved model ID, context drift + transaction rollback:
  workflow_action_changed_arguments_need_new_call_and_context_changes_conflict.
- Schema/scope/size/depth/external-reference rejection: application workflow_action_freeze_rejects_schema_scope_depth_bytes_and_untrusted_restore.
- Company/execution/digest FK, frozen SQL mutation rejection and cancelled new-intent suppression:
  workflow_action_schema_fk_immutable_and_cancelled_new_intent_fail_closed.
- Operation/target/contract identity and canonical object/array semantics: other two pure freeze tests.
- JSONB numeric normalization and replay across negative zero, integral/exponent spelling,
  large floats, schema numbers: workflow_action_numeric_jsonb_roundtrip_preserves_digest_and_replay.

Final targeted suite:7PASS,0FAIL,0IGNORE (3pure+4real isolatedDB), stock2MiB; no skip variable.
Exact command: `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_`.
Full final log:/private/tmp/workflow-04.1-tests-final.log. First test log had3DB sandbox
PermissionDenied failures, then escalated run6PASS; final7PASS includes the numeric correction.
Initial offlinecheck caught missing Value import, corrected before passing check/tests.

Independent reviewer inspected actual new/untracked files and original criteria. One P2:
JSONB normalizes numeric spelling, invalidating hashes. Corrected recursive numeric canonicalization
before hashing/storage, including schemas; new realDB regression above. Correction review PASS
for foundation scope, no remaining concrete findings; original production integration still pending.
ReviewerUUID01a0f1ba-49f5-7f83-b45c-e92cce5f918f Astra/medium fresh83548/25840032.33%
10:15:14Z usage/runtime sources (parent observed85306/25840033.01%10:15:25Z token_count.info).

Final exact sequential gate script:/private/tmp/workflow-04.1-gates.sh.
It runs fmt+both whitespace checks, locked offlinealltargets check+strictClippy, SQLxprepare+
prepare--check alltargets and graftbuild. Logs:/private/tmp/workflow-04.1-{fmt,whitespace,
check-final,clippy,sqlx-prepare,sqlx-check,graft}.log. At10:16Z fmt/whitespace/offlinecheck/
strictClippy PASS; SQLx and graph subsequently PASS, gate script exit0/session17651 completed.
SQLx metadata produced no changes (new queries are runtime SQL and are covered by realDB tests).
Fresh schema/migration inspection and isolated per-test migrations PASS. Final staged+unstaged
whitespace rechecks PASS after brief edits. No phase04 combined/full-suite gate claimed yet.
Schema inspection log:/private/tmp/workflow-04.1-schema.log; migrationinfo:/private/tmp/workflow-04.1-migrations.log.

## Resources and handoff

Historical retained directory and log were genuinely ABSENT (ls + normal/escalated pg_ctl ENOENT).
Authorized fresh initialization, not a reset: `initdb -D /private/tmp/workflow-admission-pg-e3aa -E UTF8 --locale=C`.
Created database:`createdb -h 127.0.0.1 -p 55439 workflow_admission`.
Started:`pg_ctl -D /private/tmp/workflow-admission-pg-e3aa -l /private/tmp/workflow-admission-postgres.log -o '-h 127.0.0.1 -p 55439 -k /private/tmp -c max_connections=200' -w start`.
Applied full fresh migration chain including20260930090000; NEW migration now immutable.
Task-created cluster/database currently RUNNING and retained for successor; all test-owned
databases use own_database fixture cleanup. No production/development DB used or reset.
DB TCP checks required sandbox escalation; escalated same-target commands succeeded.

Worker latest125568/25840048.59%10:17:54Z token_count.info usage/capacity sources.
Final stop sample130068/25840050.34%10:18:37Z usage/runtime sources; threshold reached,
retirement only save+stop. Reviewer confirmed completed/quiescent via interrupt_agent.
Reviewer idle afterPASS. All builds/tests/SQLx processes completed; exec17651 exit0.
Root owns acceptance/queue; no04.2 started. Root PROGRESS/RESUME changes preserved. No staging,
unstaging, commit/reset/deploy. Observed graft savings estimate2,828,149tokens (mostly orientation);
an estimate versus whole-file reading, not measured runtime token consumption.
