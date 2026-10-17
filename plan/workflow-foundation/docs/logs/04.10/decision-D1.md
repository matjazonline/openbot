### Architecture decision 04.10-D1 — proposed, awaiting B

Workflow `implement-opus-reviewed-2.1`; author A; batch 04.10 MCP. One record for every
costly-to-unwind decision in the batch. Requirements: phase 04 "MCP tool calls over HTTP",
shared "Action contract" and "Execution and uncertainty" items 1–7, MCP "Acceptance"
bullets; EXPANSION row 04.10; BRIEF-04.1…04.9 obligations (04.4: no replay from MCP
metadata/session ids; 04.5: `isError` classification is 04.10). Accepted 04.1–04.9
intent/dispatch/receipt/uncertainty/evidence/cancellation protocols are reused unchanged
unless stated. No backward compatibility: reset the database instead of migrating data.

1. **Resource owner and fixed endpoint (schema).** The existing company MCP catalog
   (`company_mcp_connections`, `company_mcp_credentials`, `company_mcp_tool_grants`) is the
   sole owner of workflow resource kind `mcp`; no parallel workflow MCP connection table.
   `endpoint_url` (with `company_id`, `id`, `transport`) becomes immutable through a new
   BEFORE UPDATE trigger, and `update_mcp_connection` rejects an endpoint change ("create a
   new connection") instead of today's rebind. Auth mode stays mutable and keeps its
   credential deletion and `credential_revision` bump; soft delete still disables. A new
   endpoint is a new resource id, so a frozen action (target = connection id) can never
   silently reach another service. The UI/API edit path and the existing rebind test change.

2. **Approved operation contract (schema).** Additive `workflow_mcp_operations(company_id,
   connection_id, name, snapshot jsonb, approval_required)`, PK (company, connection, name),
   FK (company, connection) → `company_mcp_connections` and FK (company, connection, name) →
   `company_mcp_tool_grants` ON DELETE CASCADE (`replace_grants` becomes a set difference so
   unchanged grants are not deleted and reinserted). CHECKs mirror `workflow_http_operations`:
   snapshot company = row company, `contract.name` = name, `contract.connection` null,
   `policy.recovery` = `reconcile`, snapshot ≤ 64 KiB. Admin-only `McpOperations`
   register/revoke (`authority::authorize_company`, CAS on the expected catalog revision
   without bumping it, so legacy harness declarations are not invalidated; same-name
   registration is last-writer-wins under the company lock) accepts a trusted `ToolSnapshot`
   only when: the name is both a `TypeName` and an `McpToolName`; the tool is currently
   granted; its input schema equals the reviewed discovered `inputSchema` and its output
   schema equals the reviewed discovered `outputSchema` (or `{"type":"object"}` when none
   is declared); both compile in the workflow schema dialect; `policy_revision > 0`;
   recovery = `Reconcile`. MCP annotations, discovery metadata and session ids never set
   capability, effect, recovery or replay; MCP registers no `ProviderReplayContract`
   (`replay_contract() = None`). `McpDiscoveredTool` gains `output_schema: Option<Value>`
   (≤ 64 KiB, compiled). Refresh keeps a grant only when both schemas are unchanged
   (refresh never broadens authority; a dropped grant cascades its workflow operation).
   Soft delete also deletes the connection's workflow operations.

3. **Publication, binding, admission.** `McpPublicationDirectory` captures explicit
   {connection, slot, tool} selections only from live registered operations of an enabled,
   credential-ready connection, setting `contract.connection = Some(slot)`.
   `supported_publication` routes each tool by its slot's declared kind: `mcp` →
   `mcp::registered_on` (a live company registration with the identical snapshot);
   `http` or no slot → the existing HTTP owner; other owners stay closed. Binding readiness
   and admission (`check_readiness`) check the exact bound MCP connection with
   `mcp::policy_on` (not deleted, enabled, grant present, discovery schemas still equal,
   snapshot equal except the slot). `inspect_on` reports MCP `supported_contracts` as the
   currently registered operation names (≤ 256) instead of the empty set; readiness rules
   are unchanged (disabled or no grants = Revoked; bearer without secret = Unavailable).
   A resource requirement `contract` label therefore names a registered tool, as for HTTP.

4. **Action identity and ingress.** `mcp.call` steps and direct agent MCP tools
   (`McpToolCall{slot: ResourceName, tool: TypeName, arguments, call: ModelToolCallId}`)
   build the same `ActionRequest`: run/execution scope, `operation_key` = tool name, target
   `Connection{resource: bound connection id, resource_kind: "mcp"}`, contract = the
   bundle's frozen snapshot for (tool, slot), arguments = the frozen step input `arguments`
   or the caller arguments, context as for HTTP (run actor, bundle capability ceiling).
   Steps use `prepare_step`; direct tools use `prepare_tool` with the durable model-call
   mapping; both dispatch through `ActionService::dispatch_remote` with
   `PostgresActionDispatch<McpActionAuthority, NoLocal>`. The slot resolves only through
   the run's saved binding resources; inputs cannot supply endpoints, headers or credentials.
   Agent tool exposure/selection stays 06; worker-startup wiring stays 06/08 (composition
   seam `adapters::workflow_mcp::McpRuntime`, like `HttpRuntime`).

5. **Current authority, lock order, transactions.** `McpActionAuthority::lock_current` runs
   inside the existing reserve/enter transactions after `prelock_on` (company FOR NO KEY
   UPDATE, member/principal FOR SHARE) and the run/job/attempt fence locks. It locks the
   connection row and operation row FOR SHARE, verifies grant and discovery match, and
   returns `ResourceStatus` + `CurrentActionPolicy` for the existing `validate_resource` /
   `validate_current_authority`. Every MCP catalog mutation already serializes on
   `companies` FOR UPDATE before touching connections; registration takes the company
   (authorize_company) then the connection FOR UPDATE; neither takes run locks. Credential
   rotation keeps its envelope ciphertext CAS without connection or run locks. Disable,
   delete, grant removal, refresh drop, operation revoke and membership/role changes
   therefore linearize against dispatch at the company row. No network I/O runs inside
   any SQL transaction.

6. **Transport phases and the possible-dispatch boundary.** Live discovery and the
   frozen-contract comparison run before `tools/call`, in the same session.
   - The handler reads a new read-only hint, `ActionEffectStates::effect_state(scope,
     subject)`, from the existing `workflow_action_effect_states` view. It is never
     authority, a replay grant or effect truth; reservation remains the decision.
   - Hint `prepared` (no marker, no receipt) and current policy unprotected: **eager
     open** before reservation. `mcp_for_action` (own short transaction: run actor
     company authority, live run, connection/grant/operation, credential envelope)
     precedes any network I/O; then the guarded connect (existing `EndpointPolicy`: HTTPS
     unless an exact operator IP-literal exception, vetted DNS pinned into a fresh
     proxy-free client, no redirects or retries), initialize, bounded paginated
     `tools/list`, exact comparison (tool present; input and output schemas canonically
     equal to the frozen ones; descriptions and annotations ignored). A failure here
     leaves no marker and is a clean step failure: `mcp.unavailable` (Retryable,
     SafeToRetry: DNS, connect, initialize, discovery, session capacity) or
     `mcp.contract_changed` (Terminal: missing tool or schema drift). The open session is
     handed to a per-dispatch `RemoteAction` for exactly this invocation.
   - Any other hint (committed, possible_dispatch, needs_reconciliation) or a protected
     current policy: **lazy** provider. Reservation returns the committed receipt,
     PossibleDispatchExists or ApprovalRequired without MCP I/O; only an evidence-permitted
     re-reservation (04.7 proven not-applied) opens a session inside `invoke`, after the
     marker, with the same comparison (failure there stays conservatively unknown).
   - Inside `invoke` (after first marker and entry): `mcp_for_action` again immediately
     before `tools/call`, also requiring an unchanged `credential_revision`; revocation or
     rotation since open aborts before sending. Then exactly one `tools/call`. The session
     is moved into the call, so a second invoke cannot reuse it.
   Competing claimants may each open a session, but only the reservation winner reaches
   `tools/call`; others close without calling. Reconnect never repeats `tools/call`
   (`reinit_on_expired_session = false`, no GET stream, no automatic retry).

7. **Result envelope and classification.** The adapter returns a typed
   `McpCallResult{content, structured_content, is_error}` from rmcp's `CallToolResult`
   (unknown remote fields and `_meta` dropped; absent `isError` = false). Occurrences of the
   active bearer token are redacted (strings, keys, scalars, credential-named keys) by one
   shared redaction helper moved out of `workflow_http`; discovery keeps rejecting secret
   echoes. The application envelope is `{content, structuredContent?, isError}`; for
   `isError = true` structuredContent is omitted (an error is not the declared result).
   Shared `validate_action_result` validates `mcp`-target results against the registry's
   MCP envelope schema (extracted into one function used by both the compiler contract and
   runtime): content blocks bounded, structuredContent validated against the frozen output
   schema when present, canonical size ≤ 64 KiB. Resource links and embedded resources are
   inert data and are never fetched. Outcomes:
   - valid `isError = false` → receipt → step output at `steps.<id>.output`;
   - valid `isError = true` → receipt committed (bounded diagnostics) → step failure
     `mcp.tool_error` (Terminal, EffectOutcomeUnknown); a retry returns the same receipt
     and never calls; the direct-tool observation is classified as a tool error;
   - invalid or oversized output, JSON-RPC error, transport loss, timeout, session expiry
     (404), lease loss or cancellation after entry → no receipt → existing 04.6/04.7
     unknown-effect parking and reconciliation. Tool errors and output failures never
     prove non-application.

8. **Cancellation and cleanup.** Dropping the provider future (cancel, deadline, lease
   loss, worker drop of the handler future) drops the proxy session; the existing tracked
   worker sends `notifications/cancelled` for an in-flight call and awaits a bounded
   session close (DELETE when a session id exists), and client shutdown joins it.
   Cancellation does not prove rollback; late receipts follow 04.8.

9. **Bounds — no limit raised.** Existing: ≤ 64 connections per company, ≤ 100 discovered
   tools, ≤ 1 MiB per discovery and per response, ≤ 101 pages, cursor ≤ 4096 B, session id
   ≤ 1024 B, schemas ≤ 64 KiB, arguments ≤ 64 KiB, 32 global / 4 per-endpoint sessions
   (try-acquire), DNS and connect 5 s, request 30 s, read 10 s, initialize/discovery/call
   30 s each. New: ≤ 256 operations per connection, operation snapshot ≤ 64 KiB, committed
   envelope ≤ 64 KiB (existing action bound).

**Invariants.** I1 no `tools/call` without a committed possible-dispatch marker and entry
plus a current-authority check after entry. I2 at most one `tools/call` per invocation
absent proven not-applied evidence. I3 committed receipts return without MCP I/O. I4 a
connection id's endpoint never changes. I5 no credential in bundles, actions, receipts,
logs or errors. I6 MCP metadata never grants capability or replay. I7 cross-company ids
fail closed at publication, binding, admission, preparation, dispatch and use.

**Failure modes and accepted limitations.** Drift, revocation or credential rotation
detected inside `invoke` (after the eager check) parks conservatively without sending.
Eager open costs one session per first dispatch; a stale hint is resolved by reservation
(worst case a closed, unused session). The legacy harness MCP journal (agent runs) keeps
its own path until 06/08 but shares the adapter changes (typed result, call-result
redaction instead of rejection, output schema in discovery, stricter refresh).

**Affected contracts and files (planned).** Domain `entities/mcp.rs`; application
`services/mcp_client.rs`, `services/mcp_runtime.rs` (legacy adaptation), `use_cases/mcp.rs`,
`workflow/mcp.rs` + `workflow/mcp_runtime.rs`, `workflow/actions` (envelope validation,
effect-state port), `workflow/registry/{mcp,actions}.rs`; adapters `mcp/{mod,http}.rs`, new
`workflow_mcp/`, shared redaction helper, `workflow_http` (uses it), `persistence/mcp.rs`,
`persistence/workflow/{mcp*.rs, resources.rs, authority.rs, mod.rs}`, MCP UI edit route;
additive migration `20261017110000_workflow_mcp_operations.sql`; `BRIEF-04.10.md` and the
`NO_BACKWARD_COMPATIBILITY_NOTES` entry. Callers are traced with graft before edits.

**Verification plan.** Focused DB-backed tests on the task fixture:
- Adapter: JSON and SSE initialize/discovery (with `outputSchema`)/call; typed result;
  redaction; `isError`; resource links never fetched; session-expiry 404 without
  re-initialize; dropped-call cancellation and cleanup.
- Workflow: publish → bind → admit → claim → `mcp.call` with typed argument refs and a
  downstream `structuredContent` ref; direct tool and step share one receipt through the
  model-call mapping; `isError` receipt and terminal failure; output-schema violation and
  oversize → unknown without a second entry; drift and missing tool → clean pre-marker
  failure with no `tools/call`.
- Authority: revoked grant/operation, disabled/deleted connection, foreign company,
  non-admin and membership loss fail closed; endpoint trigger and update rejection;
  encrypted credentials and rotation inventory; rotation between open and call aborts.
- Races and recovery: concurrent dispatchers on one invocation and revoker vs dispatcher
  → exactly one `tools/call`; crash/timeout/lost response after dispatch → restart sees
  PossibleDispatchExists with zero new calls; restart after receipt → committed with zero
  MCP requests; cancellation sends `notifications/cancelled` and closes the session;
  protected operation stays undispatched.
- Gates: fresh migrations + schema, SQLx prepare/check, offline all-target check, strict
  Clippy, fmt, `git diff --check`, transport-boundary scripts, focused + affected suites
  (mcp, workflow_mcp, workflow_http, credential_rotation, workflow action/receipt/
  uncertainty filters, MCP UI routes). The full all-target, stock-stack and lease gates
  run at the 04.11 combined gate unless shared-infrastructure impact requires them earlier;
  the shared `validate_action_result` change makes the action/receipt suites mandatory now.
