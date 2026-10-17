### Architecture decision 04.10-D2 — revision of D1 after 04.10-R001, awaiting B

Workflow `implement-opus-reviewed-2.1`; author A; batch 04.10 MCP. D2 supersedes D1
(`logs/04.10/decision-D1.md`, SHA256 `3f10553022b5d631876b8174d57d0ea270ff3c7d150b25d5812fd776e285035e`,
kept unchanged) and resolves 04.10-F001…F003 and the adopted optional findings
F005…F007 (F004 declined, see TASK resolution list). Requirements are unchanged: phase 04
"MCP tool calls over HTTP", "Action contract", "Execution and uncertainty" 1–7 and the MCP
and shared "Acceptance" bullets; EXPANSION row 04.10; BRIEF-04.1…04.9 obligations (04.4 no
replay from MCP metadata or session ids, 04.5 `isError` classification). Accepted
04.1–04.9 protocols are reused unchanged unless stated. No backward compatibility: reset
the database instead of migrating data. Items 1, 3, 4, 5 and 8 are D1 text unchanged
except where marked "D2".

1. **Resource owner and fixed endpoint (schema).** The existing company MCP catalog
   (`company_mcp_connections`, `company_mcp_credentials`, `company_mcp_tool_grants`) is the
   sole owner of workflow resource kind `mcp`; no parallel connection table. `endpoint_url`
   (with `company_id`, `id`, `transport`) becomes immutable through a BEFORE UPDATE trigger;
   `update_mcp_connection` rejects an endpoint change ("create a new connection") instead of
   rebinding. Auth mode stays mutable with its credential deletion and `credential_revision`
   bump; soft delete still disables. A new endpoint is a new resource id, so a frozen action
   can never silently reach another service. The UI/API edit path and the rebind test change.

2. **Approved operation contract (schema).** Additive `workflow_mcp_operations(company_id,
   connection_id, name, snapshot jsonb, approval_required, revision bigint > 0)`, PK
   (company, connection, name), FK (company, connection) → `company_mcp_connections`, FK
   (company, connection, name) → `company_mcp_tool_grants` ON DELETE CASCADE
   (`replace_grants` becomes a set difference so unchanged grants are not deleted and
   reinserted). CHECKs mirror `workflow_http_operations` (snapshot company = row company,
   `contract.name` = name, `contract.connection` null, recovery `reconcile`, snapshot
   ≤ 64 KiB). Admin-only `McpOperations::{register, revoke}` (`authority::authorize_company`,
   then the connection row FOR UPDATE): both require the expected catalog revision (the
   reviewed discovery/grant state) without bumping it, so legacy harness declarations stay
   valid. **D2 (F007):** both also CAS on the operation itself — register takes
   `expected: Option<i64>` (None = must not exist; Some(r) = replace only at revision r, then
   r+1), revoke requires the current operation revision — so stale writers cannot silently
   replace or revoke another approval. A snapshot is accepted only when: the name is both a
   `TypeName` and an `McpToolName`; the tool is currently granted; its input schema equals the
   reviewed discovered `inputSchema` and its output schema equals the reviewed discovered
   `outputSchema` (or `{"type":"object"}` when none is declared); both compile in the workflow
   dialect; `policy_revision > 0`; recovery `Reconcile`. MCP annotations, discovery metadata
   and session ids never set capability, effect, recovery or replay; MCP registers no
   `ProviderReplayContract`. `McpDiscoveredTool` gains `output_schema: Option<Value>`
   (≤ 64 KiB, compiled). Refresh keeps a grant only when both schemas are unchanged (refresh
   never broadens authority; a dropped grant cascades its operation). Soft delete also deletes
   the connection's operations.

3. **Publication, binding, admission.** `McpPublicationDirectory` captures explicit
   {connection, slot, tool} selections only from live registered operations of an enabled,
   credential-ready connection (`contract.connection = Some(slot)`). `supported_publication`
   routes each tool by its slot's declared kind: `mcp` → `mcp::registered_on`; `http` or no
   slot → the existing HTTP owner; other owners stay closed. `check_readiness` checks the exact
   bound MCP connection with `mcp::policy_on` (not deleted, enabled, grant present, discovery
   schemas equal, snapshot equal except the slot). `inspect_on` reports MCP
   `supported_contracts` = registered operation names (≤ 256); readiness rules unchanged.

4. **Action identity and ingress.** `mcp.call` steps and direct agent MCP tools
   (`McpToolCall{slot, tool, arguments, call: ModelToolCallId}`) build the same
   `ActionRequest` (run/execution scope, `operation_key` = tool, target
   `Connection{bound connection id, "mcp"}`, contract = the bundle's frozen snapshot for
   (tool, slot), frozen step `arguments` or caller arguments, HTTP-equivalent context). Steps
   use `prepare_step`, direct tools `prepare_tool` (durable model-call mapping; a changed
   argument set needs a new model-call id and creates a new invocation); both dispatch via
   `ActionService::dispatch_remote` with `PostgresActionDispatch<McpActionAuthority, NoLocal>`.
   Slots resolve only through the run's saved binding. Agent tool exposure stays 06 and
   worker wiring 06/08 (seam `adapters::workflow_mcp::McpRuntime`, like `HttpRuntime`).

5. **Current authority, lock order, transactions.** Unchanged from D1:
   `McpActionAuthority::lock_current` runs inside the existing reserve/enter transactions
   after `prelock_on` and the fence locks, takes the connection and operation rows FOR SHARE,
   verifies grant and discovery match. All catalog writers take `companies` first and never
   run locks; credential rotation keeps its ciphertext CAS. Revocation linearizes against
   dispatch at the company row. No network I/O inside SQL transactions.

6. **Transport phases and the possible-dispatch boundary.** Discovery and the frozen-contract
   comparison run before `tools/call`, in the same session.
   - The handler reads the read-only hint `ActionEffectStates::effect_state(scope, subject)`
     (existing `workflow_action_effect_states` view; never authority, replay grant or effect
     truth; reservation decides).
   - Hint `prepared` and current policy unprotected → **eager open** before reservation:
     `mcp_for_action` (own short transaction: run actor authority, live run,
     connection/grant/operation, credential envelope) precedes any network I/O; then the
     guarded connect (existing `EndpointPolicy`), initialize, bounded paginated `tools/list`,
     exact comparison (tool present; input/output schemas canonically equal; descriptions and
     annotations ignored). Failures leave no marker (outcomes in item 10). The open session is
     handed to a per-dispatch `RemoteAction` for exactly this invocation.
   - Any other hint, or protected current policy → **lazy** provider: reservation returns the
     receipt, PossibleDispatchExists or ApprovalRequired without `tools/call`; only an
     evidence-permitted re-reservation (04.7 proven not-applied) opens a session inside
     `invoke`, after the marker, with the same comparison.
   - Inside `invoke`: `mcp_for_action` again immediately before `tools/call`, requiring an
     unchanged `credential_revision`; revocation or rotation aborts before sending. Then one
     `tools/call`; the session is moved into the call.
   - **D2 (F006):** session permits are acquired with a bounded wait (≤ 5 s each for the
     global and per-endpoint semaphore; immediate failure once the client is shut down)
     instead of an immediate try-acquire, for both workflow and legacy callers. Permit timeout
     is `mcp.unavailable` (item 10). No limit is raised; the 32/4 bounds stay.
   Competing claimants may each open a session, but only the reservation winner reaches
   `tools/call`; losers close unused sessions. No reconnect repeats `tools/call`
   (`reinit_on_expired_session = false`, no GET stream, no automatic retry).

7. **Result envelope.** The adapter returns typed `McpCallResult{content, structured_content,
   is_error}` from rmcp's `CallToolResult` (unknown fields and `_meta` dropped; absent
   `isError` = false); the active bearer token is redacted by one shared helper moved out of
   `workflow_http` (discovery keeps rejecting secret echoes). The envelope is
   `{content, structuredContent?, isError}`; for `isError = true` structuredContent is
   omitted. Shared `validate_action_result` validates `mcp`-target results against the
   registry MCP envelope schema (one function shared with the compiler contract):
   bounded content blocks, structuredContent against the frozen output schema when present,
   canonical size ≤ 64 KiB. Resource links and embedded resources are inert, never fetched.
   Classification of each outcome is item 10.

8. **Cancellation and cleanup.** Unchanged from D1: dropping the provider future drops the
   proxy session; the tracked worker sends `notifications/cancelled` for an in-flight call and
   awaits a bounded close (DELETE when a session id exists), joined at shutdown. Cancellation
   proves no rollback; late receipts follow 04.8.

9. **Bounds — no limit raised.** As D1 (≤ 64 connections/company, ≤ 100 tools, ≤ 1 MiB per
   discovery/response, ≤ 101 pages, cursor ≤ 4096 B, session id ≤ 1024 B, schemas and
   arguments ≤ 64 KiB, 32 global / 4 per-endpoint sessions, DNS/connect 5 s, request 30 s,
   read 10 s, initialize/discovery/call 30 s each) plus ≤ 256 operations per connection,
   snapshot ≤ 64 KiB, envelope ≤ 64 KiB, and the new ≤ 5 s permit wait.

10. **D2 (F001, F002): outcome table and effective recovery.** Rule: for action-owned step
    kinds the handler always supplies `RetrySafety::SafeToRetry`; the shared DB predicate
    `workflow_action_retry_safe` is the only source of `EffectOutcomeUnknown`. It returns
    NULL when the execution has no marker (handler label kept), true when every marker has a
    receipt / supported replay / available not-applied evidence, false otherwise (forces
    Reconcile and a reconciliation fact). This is truthful because `tools/call` is only sent
    after a committed marker and entry, so no marker ⇒ no MCP effect. Contract change
    (affects accepted 03/04.6 no-marker defaults): one pure classifier
    `marker_owned_effect_kind(kind)` = `mcp.call | http.request | tool.call` (kinds whose
    only effect path is the marker-first action protocol) is consulted by both
    `recovery::report` (crash/lease/interrupt defaults) and `pending_recovery::unknown_effect`
    (exhausted pending work), giving them SafeToRetry instead of Unknown when no marker
    exists. The 04.9 HTTP handler's own Unknown labels are not changed in 04.10 (follow-up
    recorded for root). Effective decisions follow `RecoveryPolicy::decide`: Unknown →
    Reconcile; else deadline passed → FailRun; Terminal or attempts exhausted → FinalError
    (frozen final_error route, else failed run); Retryable → RetryAt with backoff.

    | # | Situation (step / direct tool) | Durable state after | Step code / class (handler safety Safe) | Effective decision | Direct-tool result |
    | --- | --- | --- | --- | --- | --- |
    | 1 | Eager or preparation refusal: revoked/disabled/deleted connection, removed grant or operation, missing bearer secret, foreign company, non-admin or lost membership, invalid arguments | no marker from this attempt | `mcp.rejected` / Terminal | predicate NULL → FinalError; attempt `workflow_retry_safety='safe'`, no reconciliation fact | `Err` (NotFound/BadRequest), no network |
    | 2 | Eager transport failure: DNS, connect, initialize, discovery, permit wait timeout | no marker | `mcp.unavailable` / Retryable | RetryAt until attempts exhausted, then FinalError; retry re-runs discovery and dispatches once | `Err` (unavailable) |
    | 3 | Eager drift: tool absent or schema differs | no marker, no `tools/call` | `mcp.contract_changed` / Terminal | FinalError | `Err` |
    | 4 | Reservation refusal before marker (revocation between eager check and reserve) | no marker | `mcp.rejected` / Terminal | NULL → FinalError (false → Reconcile only if an older unresolved marker exists) | `Err` |
    | 5 | Fence/lease conflict or infrastructure error before marker; hint read error | no marker from this attempt | `mcp.unresolved` / Retryable | NULL → RetryAt (stale fence: retirement refused, the owner's lease expiry rules) | `Err` |
    | 6 | ApprovalRequired (frozen or current protection) — interim until 05 | no marker, nothing sent | `mcp.approval_required` / Terminal | NULL → FinalError; no wait and no reconciliation fact until 05 adds approval waits | `Ok(ApprovalRequired)` |
    | 7 | PossibleDispatchExists (earlier unresolved marker) | marker, no receipt | `mcp.unresolved` / Retryable | false → Reconcile + fact (settleable by 04.7 evidence) | `Ok(PossibleDispatchExists)` |
    | 8 | Committed, `isError = false` | receipt | success → `steps.<id>.output` | success route | `Ok(Committed)`, classified success |
    | 9 | Committed, `isError = true` | receipt (bounded diagnostics) | `mcp.tool_error` / Terminal | predicate true → FinalError (final_error route or failed run), `workflow_retry_safety='safe'`, no reconciliation fact; an operator retry reopens the same execution and returns the same receipt with zero MCP requests; a tool error never proves not-applied, so no new invocation or call is made | `Ok(Committed)`, classified tool error |
    | 10 | Post-entry failure: lost response, JSON-RPC error, invalid or oversized output, session expiry 404, adapter call timeout, revocation/rotation or (lazy) drift detected inside `invoke` | marker, no receipt | `mcp.unresolved` / Retryable | false → Reconcile + fact; no second call | `Err` |
    | 11 | Interrupted (cancel, lease deadline, ownership loss) | none / marker / receipt | `mcp.interrupted` / Retryable | before marker: NULL → RetryAt; after entry without receipt: false → Reconcile; after receipt: true → RetryAt → retry returns the receipt without a call | `Ok(Interrupted)` |
    | 12 | Worker crash or lease expiry (no handler result; `report` default via classifier) | as reached | `workflow.lease_expired` / `workflow.interrupted` / Retryable | before marker (including eager open): NULL + Safe default → RetryAt, retry eager-opens and dispatches once; after marker without receipt: Reconcile; after receipt: RetryAt → receipt returned | n/a |

    No row produces a reconciliation wait without an unresolved marker, so every wait is
    settleable by the 04.7 evidence command; rows 1–6 cannot have sent `tools/call`.

**Invariants.** I1 no `tools/call` without a committed marker and entry plus a current
authority check after entry. I2 at most one `tools/call` per invocation absent proven
not-applied evidence. I3 **D2 (F005):** committed receipts return without `tools/call`, and
without any MCP request when the effect-state hint is current (a stale `prepared` hint may
cost one initialize/`tools/list`). I4 a connection id's endpoint never changes. I5 no
credential in bundles, actions, receipts, logs or errors. I6 MCP metadata never grants
capability or replay. I7 cross-company ids fail closed at publication, binding, admission,
preparation, dispatch and use. I8 Unknown effect safety for MCP comes only from the shared
predicate, never from a handler label.

**Failure modes and accepted limitations.** Drift, revocation or credential rotation detected
inside `invoke` parks (row 10) without sending. Eager open costs one session per first
dispatch. A declared output schema whose tool omits structuredContent commits a receipt;
downstream refs must supply defaults (F004 declined). The legacy harness MCP journal keeps its
path until 06/08 but shares the adapter changes (typed result, call-result redaction, output
schema in discovery, stricter refresh, bounded permit wait).

**Affected contracts and files (planned).** D1 list plus: `persistence/workflow/recovery.rs`
(`report`) and `pending_recovery.rs` (`unknown_effect`) via the shared classifier;
`adapters/mcp/mod.rs` permit wait. Callers traced with `graft callers --depth all`
(`validate_action_result`, `report`, `unknown_effect`, `inspect_on`, `check_readiness`,
`supported_publication`, `replace_grants`, `update_mcp_connection`, `McpSession::call`,
`McpClient::connect`) before editing.

**Verification plan (D2, F003).** DB-backed tests on the task fixture; each states run
`state`/`waiting_reason`, attempt failure code/class/`workflow_retry_safety`, marker,
receipt and reconciliation-fact counts, and the fixture's `tools/call`/request counts.
- Adapter: JSON and SSE initialize/discovery (with `outputSchema`)/call; typed result;
  redaction; `isError`; resource links never fetched; session-expiry 404 without
  re-initialize; dropped-call cancellation and cleanup; bounded permit wait then
  `mcp.unavailable`, immediate failure after shutdown.
- Workflow path: publish → bind → admit → claim → `mcp.call` with typed argument refs and a
  downstream `structuredContent` ref (JSON and SSE); every table row 1–12 for steps, with
  rows 1, 6, 9 and 10 also for the direct tool; direct tool and step share one receipt.
- F003 matrix:
  1. Lazy post-marker path: after an unknown, record 04.7 proven not-applied through the
     accepted evidence command; retry makes exactly one new `tools/call`; a drift variant
     detected inside `invoke` makes no call and parks again.
  2. Stale workers: lease loss during eager open → no marker, Safe retry, one call; lease
     loss during `tools/call` → `notifications/cancelled` and close, Reconcile, the new
     owner makes no second call.
  3. Workflow-level session expiry: 404 on `tools/call` → Reconcile + fact, one
     `tools/call`, no second initialize.
  4. Pre-marker `mcp.unavailable` (initialize failure and permit timeout): no marker,
     attempt `safe`/retryable, run RetryAt, the retry dispatches once.
  5. Direct-tool parity: revocation, protected operation and `isError` through
     `call_tool` match the step outcomes.
  6. Argument change: a new model-call id with changed arguments creates a new
     invocation; reusing a model-call id with changed arguments is refused without a new
     invocation or call.
  7. Competing catalog writers: concurrent register vs grant removal, vs refresh drop, vs
     soft delete, and two registrations from one stale revision → never an operation without
     its grant or with stale discovery; exactly one CAS winner.
  8. Cross-tenant FK: transaction-scoped inserts of a `workflow_mcp_operations` row naming
     another company's connection or grant are rejected.
  Plus: concurrent dispatchers on one invocation and revoker vs dispatcher → exactly one
  `tools/call`; crash/timeout/lost response after dispatch → restart sees
  PossibleDispatchExists with zero new calls; restart after receipt → committed with zero
  MCP requests; endpoint trigger and update rejection; credential encryption, rotation
  inventory and rotation between open and call; classifier unit test.
- Batch gate (04.10): fresh migrations + `\d` schema; SQLx prepare/check; offline all-target
  check; strict Clippy; fmt; `git diff --check`; transport-boundary script and regression;
  focused new suites (`workflow_mcp`, adapter `mcp`); affected suites of changed functions'
  callers: workflow binding/admission/publication/authority/resources tests, action
  dispatch/receipt/reconciliation (`workflow_action`, `action_`), recovery, pending
  recovery and uncertainty tests, `workflow_http`, credential rotation, persistence `mcp_*`,
  legacy `services::mcp_runtime` and rig MCP harness tests, MCP UI/API route tests; and a
  scoped stock-2 MiB run (`scripts/stack-budget.sh --offline -- workflow_action workflow_mcp
  mcp`). Full all-target, full stock-stack and lease-budget gates stay at the 04.11 combined
  gate.
