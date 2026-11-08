# 03 — Use grants, connection bindings, and host authority

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-vault-storage-and-management.md). Define reusable account grants for existing
HTTP/MCP connections and the host authority handoff. Static credentials and managed credentials
remain undispatchable through the new gateway until 05–06 are integrated; this stage proves
grant persistence and authorization without exposing a public token resolver.

## Batch 03A — Connection model and owner consent

Extend HTTP and MCP authentication contracts with an explicit credential source:
none, existing connection-owned bearer, or an integration use-grant reference. These are supported
choices, not fallback reads. Database checks require exactly the fields appropriate to the selected
source. An integration reference includes the account, opaque use-grant identity, and expected
authorization/grant versions. No fallback to another source is permitted when resolution fails.

Add module-owned integration_use_grants with an internal FK to the account and owner identity.
A grant records the trusted client namespace, opaque host binding, app, exact destination policy,
permitted execution purpose, expected account authorization revision, grant revision, and status.
Use the existing company connection as the host binding; store only its opaque integration
reference/version in Openbot. HTTP and MCP connections may reference the same account through
separate grants. Do not copy the credential or create an unrelated ACL engine.

Enforce 01's per-account grant cap and grant creation atomically. Compete same-account grant
creators at the cap and return a bounded typed rejection without inserting excess grants.

Include the destination identity and workflow-only restriction in the binding's versioned
authorization. Rebinding, changing apps/capabilities, or changing an MCP destination invalidates
the grant until the owner confirms again. Existing HTTP origins
remain immutable. Use existing company-scoped connection relationships and constraints.

A company administrator may propose a binding but cannot approve someone else's account. An owner
must confirm the named company, connection, destination, and breadth of registered-operation use.
Do not implicitly grant access by showing an entry in an autocomplete or by accepting its UUID.
Owner unbinding is available even when they no longer administer the company.

## Batch 03B — Live host authority and gateway handoff

Split the paths around http_for_action and mcp_use so host authorization/preparation can produce
nonsecret integration invocations without returning a token to workflow handlers. Existing
connection-owned authentication remains an explicit supported source. Integration-backed
execution in 06 delegates token loading/injection and transport execution to LocalIntegrationGateway.
Put application ports in the consuming inner layer; do not call providers or import SQLx from
application services.

Openbot creates an opaque authority reference bound to the exact persisted deterministic action,
request digest, actor/company/connection, and grant/account versions. Integrations validates its
grant and invokes a required DispatchAuthority adapter immediately before business dispatch.
The local adapter reuses the existing live run/action/approval/membership/policy checks. An opaque
ID, run ID, signed blob, or cached approval is not sufficient by itself. Configuration discovery
uses a distinct owner-authorized purpose and cannot mint business dispatch authority.

The final dispatch gate combines current account/grant checks with the final host check. Its
authorization decision is the linearization point: revocation committed before that decision
prevents dispatch; an already authorized call may proceed and complete. The local adapter uses
short account/grant locks to serialize module revocation with this gate and a consistent lock
order with host authority/binding mutations. Release these locks before business network I/O.
Pause competing dispatchers before this point in tests. Do not claim that revocation retracts
already authorized effects. No host/company/action transaction lock is held across credential
refresh or discovery. A future remote implementation needs the explicit protocol in 07.

Enforce the workflow-only rule both during publication and at the common dispatch boundary.
Audit direct generic HTTP tools, direct MCP tools, imported/published workflow-tool registration,
nested model tool calls, sample execution, catalog discovery, and delayed/resumed runs.
A run ID or authenticated actor alone does not establish that the caller is a deterministic
HTTP/MCP workflow step.

Owner-initiated MCP catalog discovery may use its approved destination and current owner session
through a separate typed configuration context. It must not open a generic raw-secret endpoint.
Samples use synthetic credential metadata and scripted transports; even discovery and token
resolution must not reach the production vault or provider.

For static accounts, the private injector receives a short-lived SecretString plus nonsecret
account/grant authorization identity. A static bearer app accepts only a nonempty string payload
satisfying the existing transport's limits. A registered API-key app fixes the header name and
allowed destination; reject conflicting/duplicate headers and control characters. Do not permit
arbitrary browser/model-provided header names. Structured values require a typed adapter.
Reject incompatible values at binding and again at execution. Check app capabilities, destination,
owner membership, source kind, connection status, and current action policy before resolving and
again at the final dispatch guard after waits such as DNS, catalog discovery, approval, or worker
reclaim. Membership and action-policy decisions are supplied by the host authority adapter.

Prevent a company admin from using a personal token as a generic exfiltration credential.
The owner approves the exact destination; the AppDefinition also controls allowed origins/path
families and injection. Google/Gmail's concrete restrictions appear in 06. A generic static app
can use an explicitly owner-approved endpoint under existing network restrictions, without
turning managed OAuth accounts into credentials for arbitrary HTTP/MCP targets.

## Batch 03C — Revocation and existing action semantics

Replacing a secret, revoking a binding, removing membership, or disabling a connection blocks
later dispatches. Frozen action/approval identity includes the nonsecret binding authorization
identity, app, and grant revision so an old approval cannot authorize an account or destination
substitution. Account authorization and grant revocation are checked inside integrations, while
membership and business/action policy remain host decisions.

Do not include vault payload revision in the action digest: cache updates and key rewrapping
must not invalidate reviewed business arguments. Use the authorization revision from 01.

Preserve current approval suspension, operation registration, replay disposition, cancellation,
and uncertain-effect handling. A secret reference is authorization to use credentials, not
permission to replay a business operation or bypass its existing approval policy.

## Focused checks and acceptance

Add vault_binding_ and vault_dispatch_authority_ families with real persistence. Cover two users,
two companies, same entry referenced by both HTTP and MCP, missing consent, foreign IDs,
destination substitution, direct tools, nested model tools, membership removal, revoked grants,
stale approvals, namespace substitution, changed app/injection, denied host callbacks, and samples.
Compete owner unbind/replacement against a dispatcher paused before the final authority check;
the disallowed dispatch must make zero remote calls. Reject forged authority references and
cross-connection reuse of a valid reference. Include API-key header conflict/destination tests.

Batch checks: 03A uses vault_binding_; 03B uses vault_dispatch_authority_ and affected HTTP/MCP
authorization tests; 03C uses both new families and workflow_action_approval_. Run boundary scripts
after changing dispatch wiring, and apply 02's SQL checks to changed queries.

~~~sh
cargo test --locked --offline --lib vault_binding_
cargo test --locked --offline --lib vault_dispatch_authority_
cargo test --locked --offline --lib workflow_http_
cargo test --locked --offline --lib workflow_mcp_
cargo test --locked --offline --lib workflow_action_approval_
python3 scripts/workflow-sample-boundary.py
scripts/transport-boundary-check.sh
~~~

List new filters first, record counts/results, and perform the SQL/migration/cache checks from 02
for changed queries. Acceptance: reusable grants have one enforcement home, host authority is
checked through a required port, and direct tools, samples, forged references, and unapproved
destinations cannot acquire dispatch authority. Working business execution is completed in 06.
