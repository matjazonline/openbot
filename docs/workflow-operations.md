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

`/workflow-runs/board` reads the same runs as a board: one column per state, top-level runs only,
each card with its `children` counted by `total`, `open` and `failed`. Open columns hold every open
run, oldest first. Finished columns hold runs that finished inside `window`, most recently
finished first: `day`, `week` (the default) or `month` (30 days). Each run carries its
`finished_at`. A column shows at most 50 cards; its `total` counts the runs you may see, and
`cut` says it showed only the first 50. It takes `binding_id` and `agent_id` as the listing does,
and names the `window` and `cutoff` it read.

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
| Agent publication or revoked capability | Inspect the saved draft, active revision and exact binding/assignment in [Agents](agent-authoring.md). Publish agent changes independently of workflow versions. To restore a revoked capability, select and publish a fresh incarnation; existing runs keep their captured manifest and still require current authority. Never retarget an old saved call to today's replacement. |
| Disabled/revoked connection | Restore access only if the resource and frozen operation remain authorized. Configuration changes do not rewrite an admitted run or authorize a stale approval. Otherwise cancel and publish/start appropriate fresh work. |
| Expired wait | Allow deadline maintenance to settle it. Human, timer/event, child, message receipt and reconciliation waits have durable deadline/recovery paths. Inspect persistent overdue gauges and worker logs; do not extend deadlines by hand. Budget waits retain the run deadline. |

Worker capacity is `WORKFLOW_WORKER_SLOTS` (1–16, default 2) per process. Startup requires
`DATABASE_MAX_CONNECTIONS >= 12 + 2 × WORKFLOW_WORKER_SLOTS` (default pool 20): twelve connections
cover shared workers/listener/headroom including the final-reply publisher and metrics sampler; two per workflow slot cover
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

## Channel responses and human questions

The channel's **Automatic response** card chooses one active message binding and a version1
subject/body projection from its final business result. Other bindings observe. Selection requires
current company administration, channel visibility, run-as visibility and resource readiness.
`GET` and `PUT /api/companies/{company_id}/channels/{channel_id}/response` use the same checks.
PUT carries `expected_revision` (null for the first save), `responder` (null to clear), and optional
`approval_policy`. A responder contains `binding_id`, `expected_binding_state_revision`, and
`contract`, for example:

```json
{"format_version":1,"subject":{"literal":"Response"},"body":{"ref":"/input/body"}}
```

References select required string fields of the declared final output. The optional final approval
policy is `{"reviewers":[{"kind":"user","id":"CURRENT_ADMIN_UUID"}],"review_timeout_seconds":300}`.
Reviewers must currently be company admins who can see the channel. Re-read after a revision
conflict; a stale form keeps its typed values and offers an explicit reload. Clearing selection
persists a new revision. Replaying a native create command never restores a cleared responder.
Fresh native assistant channels select only their new active binding in the provisioning transaction.
Native templates return typed `{body}` output, with no explicit final send. Editable business review
publishes the approved/edited body; rejection closes review and finishes without reply. This differs
from rejecting protected final publication, which refuses that final effect.

`channel.no_reply` (**Finish without reply**) returns its explicit business value and must route
success to `$end`. It suppresses only this run's automatic final reply. Business schemas still apply;
null or empty business output is valid when its schema permits it. Earlier explicit messages remain.
A child cannot suppress its parent's response. Native custom steps cannot route directly to `$end`
unless they use this terminal type. Normal AfterReply work returns the saved reply afterward;
composition refuses paths exceeding the unchanged activation budget.

`message.ask` atomically posts a prompt and registers a human wait. Only a correlated reply from the
expected source author resumes it; provider acceptance does not resolve the human wait. Formal
review remains private and separate. The authoring reference includes five runnable response examples.
Samples accept optional sample-only response mapping/policy, include both in their frozen identity,
and preview automatic, unmapped, suppressed or waiting outcomes. They create no live run, delivery
or reviewer notification. Sample results cannot authorize production publication.

Run and conversation response panels link the exact original message even outside the current
message window. At most 50 response roots are displayed; truncation is explicit. Execution success,
human/reviewer waiting, canonical publication and provider delivery remain separate facts:

| Display | Meaning |
| --- | --- |
| Processing | Business work is active. |
| Waiting for your response / reviewer | Work is parked; an accepted prompt does not imply a human answer. |
| Work completed; reply pending | Business success is retained while publication or delivery proceeds. |
| Completed; reply posted / sent | Local publication or provider acceptance, never a read receipt. |
| Work completed; reply needs attention | Business result remains completed; inspect the linked delivery. |
| Completed without reply | Trusted terminal suppression; prior explicit messages remain. |
| Failed / cancelled | Earlier message history remains authoritative. |

Safe queue retries reuse the same canonical message, delivery and accepted parts. Do not retry a
completed business run to repair delivery. The delivery page provides inspection/reload, not a
manual blind resend command. Unknown provider outcomes remain quarantined; inspect saved provider
identity/evidence. Existing cancellation fences unsent work and never recalls accepted messages.
Final publication survives a valid successful run's historical deadline but still requires current
authority, policy, knowledge release and a genuine publication claim. Review payloads and proposal
identities are absent from public conversation outcomes; restricted source/run content hides the
corresponding response metadata.

Structured final-response logs contain company/run/execution/intent/proposal/message/delivery IDs
and bounded status codes. Staged logs precede transaction commit; re-read durable state to establish
commit. `workflow_channel_reply_publications_total` uses only bounded outcome labels: published,
failed, retry, lease_lost, error. Existing delivery unknown/dead/retry and overdue-work monitoring
continues to apply. Never add message text, recipient lists or approval tokens to logs or labels.
These changes require a fresh database using the sole initialization migration; no old rows are
rewritten and there is no compatibility rollout.

## Agent history and retention

`agent.run` automatically stores its exact supplied context and accepted logical JSON output. A
trusted message thread shares one stream for each actual agent. Manual/scheduled work uses its
authenticated private session; an explicit authenticated session reference can reuse it. Business
JSON cannot select a conversation. Each child branch has its own private history, even when it
uses the same agent as its parent. Classifiers and agent decisions do not create logical exchanges.
Human questions, tool approval, delivery continuation and safe retries resume the same exchange.
A genuinely new task receives a new ordered exchange.

The run step's **Agent history** disclosure lists at most 16 metadata entries per page and can
inspect one bounded payload at a time. It uses current owner/admin Inspect permission, association
visibility and current access level, including for private and child runs. An otherwise authorized
admin can inspect a private run; ordinary membership or customer thread access alone is insufficient.
A lower authorized viewer sees only restricted order/state/timing. Each cursor, source and detail
request authorizes again. Completed accepted pairs use their immutable exchange classification;
in-flight input also observes the current source run classification. Source links separately check
the source's current permission and level. Retirement of an agent does not erase retained history.

The authenticated API is
`GET /api/companies/{company_id}/workflow-runs/{run_id}/steps/{execution_id}/history`.
`limit` is 1..16 (default 16); pass its opaque `next` value as `after` to read older entries.
`exchange={exchange_id}` inspects one exchange of that derived stream and takes no page parameters.
The run/execution anchor must be genuine. A sample ID, another stream's cursor or an unrelated
source handle grants no access. Reads neither dispatch work nor call a model.

**Produced** means the agent accepted a logical output, including JSON `null`. Workflow, channel
response and delivery status appear separately. A later workflow failure, cancellation or permanent
delivery failure does not erase Produced. Queued, waiting for an agent turn, active, waiting for
human input, and failed/cancelled before output are distinct states.

The next call selects only completed retained pairs within its current ceiling: at most 16 pairs
and 64 KiB of logical seed, with mandatory current input/output reservation taking priority in the
request budget. The frozen view shows the exact selected pairs, eligible-budget omissions and
request budget. Hidden or expired pairs are not presented as eligible omissions. Preparation stays
frozen through retry/resume: it is never silently refreshed. If an old selected source expires,
a retained owning preparation still shows its exact copied seed and labels the old source expired.
If the owning payload expires, the inspector says so and does not reconstruct it. It never exposes
a raw checkpoint, provider continuation or dispatch credential configuration.

These initial history limits are code-owned, with no new configuration keys:

| Resource | Limit |
| --- | --- |
| Retained payload records, including queued/active | 128 per conversation; 2048 per company |
| Recent terminal payload target | 64 per conversation; eligible payloads also expire after 90 days |
| Company pressure | Above 1536 retained payloads, expire oldest eligible copies toward 1536; sparse streams have no guaranteed minimum |
| Metadata, including expired rows | 4096 per conversation; 32768 per company |
| Conversations | 2048 per company |
| Pending work | Existing 32 per conversation; 256 per company |
| Individual stored input/output, frozen preparation, provenance | 1 MiB/1 MiB, 2 MiB, 256 KiB |

The existing supervised storage worker runs every 60 seconds, uses at most 4 company candidates
per discovery page and a 256-record cleanup budget split between payload expiry and unused scopes.
Each operation has the existing 6-second application budget and native statement/lock bounds. Its
cursors advance past protected/busy/error prefixes and wrap. A terminal history copy can expire
after its own execution/job has retired even when later workflow steps are still running. Active
turns, live human/tool continuations, failed exchanges or failed own jobs before their original source deadline (including produced output
needed for same-execution retry), unresolved
effects and required delivery/audit pins stay protected. At a hard bound, preparation backs off
without creating another history record/checkpoint or entering the provider. Existing activation
replay reuses its stored slot. There is no history reset control.

Expiry clears only history's output/preparation copies. Immutable digests, original identity,
classification, timing and Produced proof remain. `expired_count` includes deleted old metadata;
`expired_through_order` is the contiguous expired prefix rather than the largest hole. Source
workflow/checkpoint/audit copies remain under the existing90-day whole-root policy and its effect,
usage and external-rerun pins. They are not silently rewritten to satisfy history-copy limits.
Only safely expired metadata cascades with eligible source retirement. Empty scopes are pruned
only without live scoped work or native references; a later recreated stream has a new window ID.

Use the existing cancel, safe retry and reconciliation controls when a turn is stuck. Resolve
unknown effects/delivery through their existing owners; never erase fencing or issue SQL repairs
to make space. Safe retry keeps its logical exchange and recorded effects. A new task is separate
work. A separate workflow-conversation product and replacement-conversation controls remain future
support, not an available recovery action.

Structured `agent_history.*` events contain identifiers, counts, states and numeric levels.
`durability=staged` describes work inside an uncommitted transaction, including terminal output
and turn release; it is not evidence of commit. Public preparation/checkpoint verification, committed cancellation and
expiry/pruning report verified outcomes; an unresolved checkpoint acknowledgment is explicitly
unknown. Correlate the exact source execution/checkpoint with its durable state before recovery.
Fixed counters include `agent_history_payloads_expired_total`, `agent_history_payloads_skipped_total`
and `agent_history_scopes_pruned_total`; these in-process counts complement durable state and are
not a multi-instance audit ledger. Bodies, credentials and approval tokens are excluded.
