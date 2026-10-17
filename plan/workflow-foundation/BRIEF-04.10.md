# 04.10 — MCP workflow actions

No backward compatibility is required; reset the database when schema or payload
changes cannot apply cleanly. Do not add backfills or compatibility paths.

Status and verification live in [TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md).
The effective architecture is decision04.10-D2 amended by D3 in docs/logs/04.10.

The existing company MCP catalog owns connections, encrypted credentials, reviewed
discovery and grants. Endpoints are immutable; use a new connection for a new
endpoint. Workflow operation approvals reference the existing grants and disappear
when a grant is removed. Registration/revocation requires current administrator
authority, the expected catalog revision and a UUID operation version, preventing
stale updates across delete/recreate. Discovery metadata never grants capability,
idempotency or safe replay.

Publication freezes approved input/output contracts. Binding, admission, preparation
and dispatch check the exact company connection and current authority. The MCP
runtime provides both `mcp.call` and direct `call_tool` over the shared ActionService.
Argument changes require a new model-call identity; committed receipts are replayed
without another call. Production worker/tool exposure remains a phase06/08 obligation.

Initial connection and discovery occur before the possible-dispatch marker. Live
schema drift fails without calling a tool. A receipt, unresolved marker or protected
operation is resolved by the action protocol without opening a session. A retry
authorized by verified not-applied evidence opens and checks discovery after its
new marker. Current credentials and authority are checked again immediately before
the call. MCP installs no production evidence verifier or replay contract; protocol
session IDs, tool annotations and operator assertions cannot authorize a repeat.

The adapter uses the existing guarded HTTP transport for JSON and SSE. Session
admission takes an endpoint permit before a global permit under a shared five-second
deadline (four sessions per endpoint,32 total). Busy endpoint waiters retain no
global permit. Shutdown closes admission and joins supervised session workers.
Cancellation attempts protocol notification and session cleanup; rmcp serializes
transport sends, so notification can wait behind the bounded in-flight POST.
Cancellation never establishes remote rollback.

Results retain bounded content, optional structuredContent and isError (false when
absent). Active credentials and sensitive fields are redacted; resource links remain
inert. Structured output uses the frozen schema. A tool error commits diagnostic
content and fails the step without creating a new invocation; invalid output or a
lost response after dispatch remains unknown. An absent structuredContent is allowed
even when an output schema exists; downstream references must supply defaults.

HTTP and MCP action handlers now share failure classification. Pre-marker rejection
and interim approval refusal terminate safely; transient pre-marker errors retry.
Only the database action-effect predicate promotes a failure to unknown. An
action-owned reconciliation wait requires an unresolved remote marker or an evidence
conflict. Preparation checks connection readiness before attempting dispatch.

Phase06/08 obligation (04.10-F011): every handler advertising `mcp.call`,
`http.request` or `tool.call` must perform effects exclusively through ActionService's
marker-first local/remote protocol. The shared recovery classifier assumes this;
any new tool bridge must preserve the invariant or change the classifier first.

Verification includes real publish/bind/admit/claim execution, JSON/SSE, typed refs,
direct-tool parity, competing dispatch, operation CAS, revocation, credential rotation,
cross-company refusal, output errors, lost responses, stale workers and cleanup.
The not-applied retry test injects a test-host verifier through the real reconciliation
service; it does not install a production verifier. All results and limitations are
recorded in the plan-local evidence, with the full phase04 gate after04.11.
