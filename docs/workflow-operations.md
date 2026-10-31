# Workflow operations and fresh cutover

This release uses one workflow engine and Rig agent runtime. It requires a fresh database;
there is no business-data migration, compatibility decoder, or old-worker coexistence mode.
Local verification is release evidence, not evidence that a particular deployment has passed its pilot.

## Observe and locate work

`GET /metrics` returns JSON. The `gauges` object contains deployment-wide durable workflow
observations, without company/run/action identifiers or payload labels. Every process samples the
same database: do not sum these gauges across replicas. Scrape through the deployment's access
controls. These gauges do not grant permission to inspect a run.

The sampler runs every 30 seconds with a four-second application deadline, a three-second database
operation budget, a two-second statement timeout and a 250 ms lock timeout. It uses one read-only,
repeatable-read transaction. Failure sets `workflow_metrics_collection_ok` to zero and retains the
last successful values and `workflow_metrics_last_success_timestamp_seconds`; missing gauges before
first success mean unavailable. Alert on failure or stale timestamps before interpreting zero backlog.
A large database can exceed the query budget; collection failure is explicit, never a partial count.

| Gauges (all prefixed `workflow_`) | Meaning and response |
| --- | --- |
| `ready_jobs`, `oldest_ready_seconds` | Due pending workflow jobs and maximum age since due time. Sustained age calls for checking capacity, fairness, poison jobs and provider availability. |
| `active_runs`, `waiting_runs` | Queued/running and parked runs, respectively. Terminal runs are excluded. |
| `expired_leases`, `retries_last24h` | Processing jobs past lease expiry; workflow attempts numbered above one started in the last 24 hours. Inspect recovery and repeated failures. |
| `completed_steps_last24h`, `step_duration_p95_seconds_last24h` | Completed executions and p95 activation-to-completion elapsed time, including parked time. Zero latency with zero observations means no sample. |
| `open_decisions`, `oldest_decision_seconds` | Decisions whose wait is still open and maximum age since creation. |
| `expired_run_deadlines`, `expired_wait_deadlines` | Live runs/open waits past deadline. A persistent count indicates maintenance delay or failure. |
| `reconciliation_runs`, `unknown_actions` | Parked reconciliation runs; action states needing reconciliation or applied without a result. An in-flight possible dispatch is not counted as known uncertainty. |
| `unknown_deliveries`, `delivered_last24h`, `dead_deliveries_last24h` | Delivery outcome unknown; accepted deliveries by delivered time; currently dead-lettered deliveries updated in the last 24 hours. Includes non-workflow mail handled by the shared delivery worker. Acceptance is not proof the recipient read the message. |
| `model_reserved_tokens`, `model_charged_tokens_last24h` | Outstanding input/output reservations; actual usage where known, otherwise conservative reserved usage for calls requested in the last 24 hours. These can overlap. |
| `model_reserved_estimated_usd`, `model_charged_estimated_usd_last24h`, `unpriced_model_calls` | Those amounts at current company/provider/model prices; unpriced calls in the same live/24-hour scope. Missing prices are excluded from the monetary sums, so a zero estimate is not free usage. Historical price changes affect estimates. This is not a bill or an enforced monetary budget. |

Use the company Workflows/Runs page to filter failures/waits and open the exact run, execution,
action and decision. The authenticated read API is
`GET /api/companies/{company_id}/workflow-runs/{run_id}`; list runs under the same prefix at
`/workflow-runs`, newest first, paged with `after` and `limit`. The listing filters by:

- `binding_id`;
- `state`;
- `agent_id`: runs of versions that use that agent of the company, including branches a run never
  took. Another company's agent is not found.
- `lineage`: `top_level` leaves out child runs. Without it, child runs are listed with their
  parent named.

Decision links use `/ui/workflow-decisions/{decision_id}`. Visibility is checked
against current company/channel membership. Keep identifiers in restricted incident records and
links, not metric labels. Retain receipts and audit events; never repair status with SQL updates.

## Safe operator actions

The API paths below are relative to `/api/companies/{company_id}`. Use the UI where possible;
control commands use the revision just read and a stable `command_key`. Re-read after a conflict.

| Situation | Procedure |
| --- | --- |
| Failed run | Inspect its failed step and receipts. `POST /workflow-runs/{run_id}/retry` accepts only a failed top-level run whose failed step is safe to repeat. It resumes frozen work, not edited configuration. Do not force a rejected retry by clearing attempts, reservations or receipts. |
| Human review | Read the exact proposal, revision and artifact digest; submit a declared choice, feedback and structured data. Comments do not advance work. Late or stale submissions cannot settle a replacement decision. |
| Reviewer unavailable | There is no live reassignment endpoint. Assess completed/unknown effects, cancel the old run, publish corrected reviewers, configure the new version, and explicitly start replacement work when safe. The old decision and history remain closed records. Never silently transplant approval to new work. |
| Cancel | `POST /workflow-runs/{run_id}/cancel` fences further workflow progress and supersedes unsent owned work. Inspect any in-flight or already accepted effects; cancellation is not compensation. |
| Unknown HTTP/MCP write | Retain invocation, dispatch, connection and provider evidence. The reconciliation endpoint records an annotation in production; no trusted provider verifier is installed, so it cannot prove absence or resume dispatch. Resolve the external business result and arrange safe fresh work only after evidence supports it. See [reconciliation safety](workflow-reconciliation-safety.md). |
| Unknown delivery | Inspect the provider using the saved outbound identity and its audit trail. Do not resend an uncertain part. Already successful destinations remain completed if siblings fail. A run retry cannot override uncertainty. |
| Disabled/revoked connection | Restore access only if the resource and frozen operation remain authorized. Configuration changes do not rewrite an admitted run or authorize a stale approval. Otherwise cancel and publish/start appropriate fresh work. |
| Expired wait | Allow deadline maintenance to settle it. Human, timer/event, child, message receipt and reconciliation waits have durable deadline/recovery paths. Inspect persistent overdue gauges and worker logs; do not extend deadlines by hand. Budget waits retain the run deadline. |

Worker capacity is `WORKFLOW_WORKER_SLOTS` (1–16, default 2) per process. Startup requires
`DATABASE_MAX_CONNECTIONS >= 9 + 2 × WORKFLOW_WORKER_SLOTS` (default pool 20): nine connections
cover shared workers/listener/headroom including the metrics sampler, two per workflow slot cover
work and renewal. Tenant admission limits and fair polling are additional controls; increasing
slots does not remove them. Mixed-load tests exercise full tenants, unsupported work, poison,
waiting and hung work, quiet tenants and sustained refill. Inspect queue age before increasing
capacity, and retain the lease and stock-stack CI budget alarms.

## Deliberate fresh cutover

1. Name the application, database, environment, release digest, operators and controlled recipients.
   Obtain explicit authority for that target and any reset. Retain any required export securely
   outside this release's migration scope. Record external effects that must survive the reset.
2. Close ingress and configuration access at the routing/deployment layer; deactivate admission
   bindings and stop schedule materialization. Stop all old web/worker/delivery processes and join
   or time out their shutdown under supervision. Account for in-flight effects with providers.
   Old and new dispatchers must never send concurrently from the same inbox.
3. Provision the fresh database and deploy the same release everywhere. Apply the squashed init
   migration; verify readiness. Retain the non-root image and proxy port mapping. Configure actual
   keys from [deployment settings](deploy.md), including database, encryption, domain and transport.
   Startup validates local configuration, not remote credentials, provider readiness or pilot results.
4. Recreate company membership, channels, model connections, agents, approved HTTP/MCP resources
   and credentials. Install validated workflow templates, publish and configure bindings while
   keeping them **inactive**. Keep schedules and external ingress closed. No legacy runtime flag
   or alternate dispatcher is needed. Memory and classifier-selected capability profiles are out
   of this release; see [exclusions](https://github.com/matjazonline/openbot/blob/19bab48c28c76a929f20a19f46e44bfb75c24cae/plan/workflow-foundation/NOT_PLANNED.md).
5. Run isolated samples first: confirm no customer deliveries or external writes. In a restricted
   pilot, deliberately activate only the controlled binding and exercise manual, email and schedule
   starts. Verify classification/context, comments and revision feedback, the exact accepted reply
   and threading, two independent same-thread runs, and restart recovery. Exercise a controlled
   unknown effect without resending it. Verify configured model/MCP credentials at the endpoint.
6. Record run/decision/action IDs, provider counts and receipts, metric freshness, release digest
   and pass/fail results without secrets. Open selected production bindings and ingress only after
   the required checks and pilot pass. Keep unselected bindings inactive. Observe queue age,
   waits, retries, failures and reservations during the first live window.
7. Emergency stop: close ingress, deactivate bindings, pause schedules and stop dispatchers;
   cancel scoped runs where appropriate, then inventory unknown/in-flight effects. Deactivation
   alone stops new admission, not already admitted work. Accepted provider effects cannot be undone
   by shutdown, a database reset or cancellation. Prefer fix-forward; never put an old binary
   against a schema/checkpoint contract it cannot enforce.

Current ownership: ingress creates durable message intents; bindings admit immutable runs;
`WorkflowEngine` owns polling and maintenance; executions own outputs/routes; action invocations
own remote uncertainty; human decisions own review; the shared delivery worker owns transport
attempts. Obsolete channel-owned agent dispatch and the alternative runtime were removed in phase08.
The old [replacement inventory](https://github.com/matjazonline/openbot/blob/19bab48c28c76a929f20a19f46e44bfb75c24cae/plan/workflow-foundation/REPLACEMENT-MAP.md) records that baseline,
not supported APIs. [Release evidence](workflow-verification.md) maps the acceptance scenarios.


## TypeSafe Jev evaluation

Company Settings manages TypeSafe connections separately from agent model providers.
Owners and company admins can create, rotate, disable and restrict exact allowed
model versions. The API is `/api/companies/{company_id}/typesafe-connections`,
with `PUT /{id}` for availability/models and `POST /{id}/rotate` for a replacement
key; updates carry `expected_revision`. Views expose readiness and revision only,
never stored credentials. Ready means the connection is enabled. The list does not decrypt the credential
or probe TypeSafe; current authorization and credential decryption are checked at
provider use. Reload after a revision conflict.

Choose a ready connection in the workflow Bindings tab. A published `jev.evaluate`
step fixes its exact `jev-MAJOR.MINOR.PATCH` model and typed questions; aliases are
rejected. Noul's `noul` value is already its probability, so it has no separate
confidence. Use explicit downstream rule thresholds; confidence is not correctness.
The catalogue shows exact answer paths and a compiling mixed example. The
`samples/jev-triage.json` automatic/human scenarios and `samples/jev-image.json`
experimental Box fixture run with supplied results and usage only.

Jev calls use `https://api.typesafe.ai/v1/systemone`, a 5-second connection timeout,
a deadline of at most 30 seconds and 64 KiB request/response limits. Each provider
entry reserves one model call and tokens regardless of question count. Unconfirmed
calls retain conservative usage; a crash after provider use but before durable
settlement may incur another charged call on retry. A committed validated result
is reused after recovery without another provider call. Usage and run-step views
show provider `typesafe`, pinned model and tokens with no agent association.

Disable a connection or remove its model to stop subsequent authorized provider
entry; no replacement connection is selected. An already authorized in-flight call
can finish. Rotation keeps the same connection ID and uses the new key at next use.
Current company, connection and image-file authority are checked at the final
provider boundary; loss of a lease cancels work and fences stale settlement.
Authentication/model/validation refusals are terminal; bounded transport or rate
failures follow the normal retry/deadline policy. Inspect failure codes and usage
without copying keys, provider bodies or image bytes into incident notes.

Images/Box remain experimental. Only private authorized immutable attachment
references are accepted; PNG/JPEG images must be still, at most 24 KiB, 280–1024
pixels per side, with at most four assets and the same aggregate 64 KiB wire cap.
Images require Jev 1.13/1.14 and Box 1.14. A missing storage configuration or revoked,
changed or oversized attachment produces an explicit failure, never an external URL
fetch. See the exact [image contract](workflow-language-v1.md#experimental-jev-images-and-box).
