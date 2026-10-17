### Architecture decision 04.10-D3 — amendment of D2 after 04.10-R002, awaiting B

Workflow `implement-opus-reviewed-2.1`; author A; batch 04.10 MCP. D3 = D2
(`logs/04.10/decision-D2.md`, SHA256 `84d532eec3476569a878bca7188ed914ecde7f8ff9f09e4b462679e55330ded5`,
unchanged) with the replacements and additions below; every D2 item, row and invariant not
named here stands as written. D1 is unchanged. Scope per root: the 04.9 HTTP handler
alignment (F008) is in 04.10. Resolves 04.10-F008, F009, F010 and adopts F011.

**A. Replaces D2 item 2, operation CAS sentence (F010).** `workflow_mcp_operations` carries
`version uuid NOT NULL` instead of `revision bigint`. Every successful register writes a
fresh random UUID v4 version; nothing reuses or derives it. Register takes
`expected: Option<Uuid>`: None = create-only (fails if the row exists); Some(v) = replace
only while the stored version is v. Revoke requires the stored version. Because a deleted
row's version is never reissued (revoke, grant-FK cascade from grant removal or refresh
drop, soft delete), a stale writer holding a version from before a delete and re-register
can neither replace nor revoke the new approval: no ABA. The expected catalog revision
check and the no-bump rule for the catalog revision are unchanged. New failure mode: a
writer that lost a CAS must reload; there is no merge.

**B. Replaces D2 item 6, permit bullet (F009).** Session permits are acquired in a fixed
order: the per-endpoint permit first, then the global permit, under one shared deadline of
≤ 5 s measured from the start of acquisition. A caller waiting for a busy endpoint holds no
global permit, so one hot endpoint can occupy at most its 4 per-endpoint permits' worth of
global capacity, and callers to other endpoints are not starved by its queue. Both waits
fail immediately once the client is shut down (semaphores closed). Timeout yields
`mcp.unavailable` (D2 row 2). No limit is raised; the 32 global / 4 per-endpoint bounds,
the endpoint-keyed map and the Weak-entry cleanup stay. New failure mode: global
exhaustion by eight saturated endpoints still times out callers after ≤ 5 s (bounded,
classified, pre-marker).

**C. Replaces the first paragraph of D2 item 10 (F008, F011).** Rule for every
action-owned step kind (`mcp.call`, `http.request`, `tool.call`): the handler always
supplies `RetrySafety::SafeToRetry`; `workflow_action_retry_safe` is the only source of
`EffectOutcomeUnknown` (NULL without a marker keeps Safe; false forces Reconcile plus a
reconciliation fact; true keeps Safe). One shared pure mapping
`action_step_failure(family, outcome)` in the application action layer, used by both the
MCP and the HTTP handlers, produces code and class:
- preparation error (before `dispatch_remote`): NotFound/BadRequest/InvalidCredentials →
  `<family>.rejected` Terminal; any other error → `<family>.unresolved` Retryable;
- `dispatch_remote` error (reservation refusal, ambiguity, provider or post-entry error) →
  `<family>.unresolved` Retryable — the predicate decides: no marker → retry, and the next
  attempt's preparation re-checks authority and classifies a revocation as `rejected`;
  marker without receipt → Reconcile;
- ApprovalRequired → `<family>.approval_required` Terminal (interim until 05);
- PossibleDispatchExists → `<family>.unresolved` Retryable; Interrupted →
  `<family>.interrupted` Retryable;
- MCP only: eager transport `mcp.unavailable` Retryable, drift `mcp.contract_changed`
  Terminal, committed `isError = true` → `mcp.tool_error` Terminal.
`family` is `mcp` or `http`. D2 row 4 changes accordingly: a reservation refusal before the
marker is `mcp.unresolved` Retryable → RetryAt (one extra attempt; FinalError if attempts
are exhausted), and the next preparation refuses as `mcp.rejected`. All other D2 rows stand.
The `marker_owned_effect_kind` classifier and its use in `recovery::report` and
`pending_recovery::unknown_effect` stand.

Handler-composition invariant (F011): the classifier is truthful only while every
`WorkflowHandler` that supports one of those kinds performs effects solely through
ActionService's marker-first remote/local protocol (no I/O-bearing effect before a
committed marker). Today the only such handlers are `HttpRuntime` and the new
`McpRuntime`. The invariant is documented on the classifier and on `WorkflowHandler`,
and recorded in BRIEF-04.10 as a 06/08 replacement obligation (any new tool bridge for
those kinds must use ActionService or change the classifier first). No runtime assertion
is added: there is no handler registry to check at composition time.

**D. New D2 item 11 — 04.9 HTTP handler alignment (F008).** `HttpActions::execute`
(`application/workflow/http_runtime.rs`) replaces its single Terminal/Unknown
`http.unresolved` mapping with `action_step_failure("http", …)`:

| HTTP case | Code / class (Safe) | Effective decision |
| --- | --- | --- |
| `http_step_request` refusal (actor, revoked operation/connection, policy mismatch), `HttpRequest::parse`, `prepare_step` validation | `http.rejected` / Terminal | NULL → FinalError, no wait, no fact |
| ApprovalRequired | `http.approval_required` / Terminal | NULL → FinalError (interim until 05) |
| `dispatch_remote` error before marker | `http.unresolved` / Retryable | NULL → RetryAt; the next preparation rejects |
| PossibleDispatchExists, post-entry error, Interrupted after entry | `http.unresolved` / `http.interrupted` Retryable | false → Reconcile + fact |
| Committed receipt | step output (unchanged) | success route |
| Crash or lease loss before / after marker | `report` default Safe via classifier | RetryAt / Reconcile + fact |

`HttpRuntime::call_tool` (direct tools) is unchanged: it already returns the observation or
the error. Accepted 04.9 effect, receipt and authority behaviour is otherwise unchanged.

**E. Invariant addition.** I9: no action-owned step leaves a run in
`waiting`/`reconciliation` without at least one unresolved remote marker of that
execution (so every such wait is settleable by the 04.7 evidence command).

**F. Verification additions (to the D2 plan and batch gate).**
- HTTP (F008), DB-backed through the real handler and retirement, each asserting run
  `state`/`waiting_reason`, attempt failure code/class/`workflow_retry_safety`, marker,
  receipt and reconciliation-fact counts, and zero network: pre-marker operation
  revocation → `http.rejected`, run failed (or final_error route), `safe`, no marker, no
  fact; protected operation → `http.approval_required`, same state; crash/lease expiry
  before the marker → `workflow.lease_expired` retryable `safe`, run running with the job
  rescheduled; post-entry unknown → Reconcile with one fact. Unit test of
  `action_step_failure` for both families.
- Permits (F009): adapter test with one endpoint saturated (4 held) and queued waiters
  while a second endpoint connects within its bound; waiters on the saturated endpoint time
  out as unavailable while holding no global permit (global available count asserted);
  shutdown fails waiters immediately.
- Operation CAS (F010): added to D2 matrix item 7 — stale version after revoke and
  re-register refused for replace and for revoke; stale version after a grant-cascade
  delete and re-register refused; two concurrent registers from one version → exactly one
  winner.
- Gate: unchanged from D2 (`workflow_http`, recovery and pending-recovery suites were
  already included).
