# Rig agents: configuration and operations

Rig is the only **in-process** agent runtime. It shares the server's privileges and is not a
sandbox. The application owns credentials, authorization, durable tasks, memory, review and
delivery. The system-prompt generator makes one tool-free Rig completion (`RigTextClassifier`)
with the credential it resolved. That call writes no model usage row, so usage analytics do not
count it.

## Configure an agent

In Agents, open an agent's settings (or create an agent) and save. Company and library forms use
the same fields. Model connections, skills and the response format are separate settings. There
is no runtime selector and no tool catalogue: the legacy native tools, built-in tools, agent MCP
selections and executable skill tool steps were removed in phase 06C. Workflow tools an agent may
use are named by its `workflow_tools` and dispatched by workflow actions, not by this runtime.

For `POST /api/companies/{company}/agents`, this is a minimal request. The company must already
have a model connection and an enabled model; credentials are not part of this body.

```json
{
  "name": "Assistant",
  "slug": "assistant",
  "config_json": { "version": 1 }
}
```

Rig V1 accepts only `version` and optional `max_turns` (integer 1–16, default 8). For example,
`{"version":1,"max_turns":4}` allows at most four model requests across the run, including repairs
and continuations. Unknown fields fail validation. Advanced JSON cannot configure credentials,
providers, URLs, tools or a response contract. Payloads naming the retired `harness_kind`,
`granted_tool_ids` or `native_tool_policy` fields are rejected.

API update omission preserves stored fields; an explicit `config_json: {"version":1}` resets
advanced settings. Library installation preserves its definition's config and response contract.

## Verified compatibility

The following is application support verified with local, request-checked provider fixtures using
the real SDK parsers and tool bridge. It is not a live authentication test or a promise that every
upstream model works. Choose only a model enabled in the company's connection; pilot the particular
model and capabilities before production use.

| Company provider key | Rig transport | Verified fixture coverage |
| --- | --- | --- |
| `google` | Gemini GenerateContent | Text, tool calls/results, continuation, usage, structured validation/repair |
| `openai` | Explicit Chat Completions | Text, tool calls/results, continuation, usage, structured validation/repair |
| `anthropic` | Messages | Text, tool calls/results, continuation, usage, structured validation/repair |
| `groq` | Groq OpenAI-compatible completion client | Text, tool calls/results, continuation, usage, structured validation/repair |
| `xai` | Native xAI Responses | Text, tool calls/results, continuation with distinct call/item IDs, usage, structured validation/repair |

| Capability | Rig behavior |
| --- | --- |
| Tools | Only `read_resource`, for attached skills; no built-in, native or MCP tools |
| Skills | Scoped catalog plus `read_resource`; the model follows the loaded prompt steps |
| Context and memory | Application-composed history/memory and refreshed time, timezone, identity and recipient role |
| Structured final responses | Optional shared JSON Schema contract, enforced by the application |
| Host access / delegation | No shell, filesystem, HTTP, provider-hosted tools, or Rig-managed sub-agent runtime |
| Streaming / multipart completions | Unsupported |

Skills are prompt steps, not deterministic recipes: loading a resource does not execute them.
Supported references use `user_input`, `context` and earlier `steps[index].result`, with dotted
fields and numeric indices. Filters, expressions, control blocks,
forward references and unknown roots fail preflight. Missing runtime values require clarification.
Limits are 16 attached skills, 32 steps/skill, 8,000 characters/prompt, a 32 KiB catalog and a
16,384-character rendered resource. Resume revalidates access and uses saved resource/results.

## Structured final answers

Select JSON Schema in the response-format setting and enter the schema, or submit this API patch
to `PUT /api/companies/{company}/agents/{agent}`. `response_contract` is a sibling of `config_json`.

```json
{
  "response_contract": {
    "version": 1,
    "format": "json_schema",
    "schema": {
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "type": "object",
      "properties": {
        "status": { "type": "string", "enum": ["done", "needs_input"] },
        "summary": { "type": "string", "maxLength": 500 }
      },
      "required": ["status", "summary"],
      "additionalProperties": false
    }
  }
}
```

A valid answer is `{"status":"done","summary":"The request is complete."}`. Bounded local
references are also supported; this alternative contract accepts the same answer:

```json
{
  "response_contract": {
    "version": 1,
    "format": "json_schema",
    "schema": {
      "$defs": { "status": { "type": "string", "enum": ["done", "needs_input"] } },
      "type": "object",
      "properties": {
        "status": { "$ref": "#/$defs/status" },
        "summary": { "type": "string", "maxLength": 500 }
      },
      "required": ["status", "summary"],
      "additionalProperties": false
    }
  }
}
```

Draft 2020-12 is used when `$schema` is omitted; other dialect declarations are rejected. Schemas
are meta-validated and limited to 65,536 serialized bytes, 4,096 visited nodes and depth 32 including
reference traversal. The envelope is limited to 65,792 input bytes. External/file references and
recursive schemas are rejected; regular expressions use a non-backtracking engine with a 64 KiB
compiled-pattern limit. The JSON examples above are read by the application's parser tests in CI.

Create omission or null means ordinary text. Update omission preserves the contract; explicit
`response_contract: null`, or choosing ordinary text in the form, clears it. A run already
prepared with a different contract fails closed rather than delivering under the new one.
Structured delivery requires
one answering agent, since concatenating constrained answers would change the payload.

The final candidate must be exactly one JSON value. The application validates it **after
sanitization and before completed-answer persistence**, then checks the frozen publication/delivery
body again. Final response bodies are capped at 64 KiB; checkpoint and serialized-output limits
also apply. Successful JSON delivery omits the product footer. The saved contract is immutable for
review drafts: human edits must satisfy that original contract, cannot replace/remove it, and do
not trigger model repairs. Provider-native structured generation cannot replace these checks.

An invalid answer can use at most two additional repair calls within the original model, token and
deadline budgets. Repair calls expose no tools. Exhaustion is terminal `invalid_output`, with no
completed answer or answer delivery; restarting a worker does not reset repair counters. Without a
contract, ordinary text remains the default.

## Execution and limits

Production inbound, scheduled and simulated work uses the task-backed execution ledger. The full
assistant tool batch (`read_resource` calls) is saved before execution; calls execute sequentially
with their original arguments and IDs. Completed results are replayed after restart, and final
output is consumed with the reply/review/outbox transaction.

| Resource | Enforced ceiling |
| --- | --- |
| Model attempts | Default 8; `max_turns` 1–16, shared by continuations, skills and repairs |
| Tool attempts | 64/run; concurrency 1; individual deadline 30 seconds or remaining run time |
| Input reservation | 131,072/request; 1,048,576/run |
| Output reservation | 16,384/request; 262,144/run |
| Arguments / durable tool result | 64 KiB each; model-facing result also capped at 16,384 characters |
| Serialized checkpoint | 2 MiB including conservative pretty-JSON measurement |
| Execution claims | 64/run |
| Active execution | At most 300 seconds/claim, 3,600 seconds/run; caller can shorten |
| Provider HTTP | 10-second connect, 120-second request, 4 MiB completion body; outer run deadline still applies |

Reservations count serialized input bytes conservatively and charge output/active time upfront.
Unknown requests remain charged; retries, suspension and process restart cannot reset budgets.
Provider automatic retries are disabled. Cancellation stops local work but cannot undo a remote
effect already accepted. The [deployment guide](deploy.md#rig-rollout-on-a-fresh-database) covers
pilot evidence and recovery.

## Company HTTP MCP

In company settings, open **MCP servers**. Create a `streamable_http` connection with an endpoint,
enabled state and authentication type (`none` or `bearer`). Save a bearer token separately, then
use the connection test/refresh to initialize and discover tools. Review discovered schemas and
grant tool names explicitly. Workflow MCP actions call granted tools; agents no longer select
connections (removed in 06C). Definitions and encrypted secrets belong to the company and are
separate from model credentials.

Saving checks local configuration, not connectivity or authentication. Refresh tests both enabled
and disabled definitions; bearer connections require a token first. Discovery grants nothing.
Refresh revokes grants for removed/changed schemas; review and re-grant them before use. Execution
rechecks discovery fingerprints and definition and credential revisions before dispatch.
Endpoint/auth changes clear the token. Library export/copy does not transfer company connections or
secrets.

| Surface under `/api/companies/{company}` | Operation |
| --- | --- |
| `/mcp-connections` | GET catalog; POST definition |
| `/mcp-connections/{id}` | GET; PUT `{expected_revision, definition}`; DELETE `{expected_revision}` |
| `/mcp-connections/{id}/credential` | PUT `{expected_revision, token}`; null removes the token |
| `/mcp-connections/{id}/refresh` | POST `{expected_revision}` |

A definition has `slug`, `endpoint_url`, `transport: "streamable_http"`, `enabled`,
`auth: {"type":"none"}` or `{"type":"bearer"}`, and optional `tool_grants: [{"name":"remote_name"}]`.
Use the revision returned by the most recent read/write. GET exposes `auth.secret_set`, never the
token. All operations require company-management authorization; request bodies are capped at 128 KiB.

Public destinations must use HTTPS with no userinfo, query or fragment. DNS answers are validated
and pinned; redirects, environment proxies and automatic HTTP/session replay are disabled. Only
deployment-owned `MCP_INTERNAL_ENDPOINTS` can allow exact IP-literal URLs, including scheme, port
and path (for example `http://127.0.0.1:8080/mcp`). Startup rejects malformed or hostname exceptions.
Company settings cannot widen this policy. JSON and POST SSE responses are supported; stdio,
legacy SSE, OAuth, unsolicited GET streams, sampling, roots, elicitation, resources and prompts
are not enabled.

Limits: 64 live definitions/company, 32 granted tools/connection, 100 discovered tools/connection, 64 KiB/schema and arguments, 2,048 bytes/URL, 16 KiB/bearer token and 1 MiB wire
data/discovery or call. Discovery has a 30-second total deadline; initialization/call deadlines are
30 seconds, DNS/connect 5 seconds and idle reads 10 seconds. Each process allows 32 sessions total
and four per exact endpoint.

Remote intent is journaled before network dispatch. Completed receipts bypass the network;
uncertain effects become `indeterminate` and are never automatically replayed. There is no automated
reconciliation/reset interface or exactly-once remote-effect guarantee. See the recovery procedure
in the deployment guide before retrying an uncertain operation.

## Diagnostics and verification boundaries

Task details expose safe run diagnostics. Output metadata's `execution_diagnostics` records
`harness`, `provider`, `model`, `prompt_characters`, `response_characters`, `history_message_count`,
`model_calls`, `tool_call_count`, `tool_names`, `supported_tool_ids`, `token_usage_source`,
`unreported_usage_calls`, `unresolved_model_calls`, `structured_validation`, `repair_call_count`
and `terminal_invalid_output_reason`. Validation is `not_configured`, `pending`, `validated` or
`invalid`. The application adds elapsed duration.

The durable projection adds run/revision/state and wait IDs, reservation counts/limits,
`continuations`, `replayed_invocations` and `indeterminate_effect`. Terminal diagnostics remain
available without a reply. Usage is reported, estimated or mixed; missing counters and unresolved
requests use reserved estimates. Aggregate usage includes repair requests exactly once across
recovery; there is no separate repair-only billing total. These counts are not provider invoices.
Schemas, candidates, prompts, arguments/results, credentials and approval tokens are excluded.

Startup validates configuration, registry coverage and local transport construction; it does not
authenticate company keys or probe model availability. Agent writes/preflight validate typed
settings, response schemas and capabilities. CI uses local provider/MCP fixtures plus PostgreSQL
for durable approval, competing claims, review and delivery tests. Live model authentication and a
controlled MCP endpoint pilot are separate operator checks, not startup guarantees.

The [verification record](rig-verification.md) contains the release
matrix. See [adapter maintenance](../src/adapters/harness/rig/README.md) for exact dependency pins
and upgrade gates, and [rollout](deploy.md#rig-rollout-on-a-fresh-database) for the deployment checklist.
