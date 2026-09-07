# Orchestration for a tenant-owned execution backend

Status: implementation plan, 2026-09-07. The types, routes, tables, limits and UI below are
proposed contracts, not implemented features or configuration keys the server currently reads.
This document prepares the abstractions for implementation; it does not enable remote execution.

## Product decision and responsibility

Companies can supply custom tool scripts and select/configure an execution provider from the
platform's supported providers. Initially the only option is **Fly.io**. Execution happens in
the company's cloud account, with cloud charges paid by the company. A company may leave remote
execution unconfigured; existing platform tools and skills continue to work.

The company controls its script, dependencies, network access, external credentials and cloud
infrastructure. Arbitrary scripts and outbound networking are permitted in that environment.
The platform does not build an egress allowlist, audit scripts, promise sandbox safety, or require
approval for each network request. Platform-owned credentials and cross-company authority never
enter the execution environment. Tenant-controlled results remain untrusted application input.

The platform owns company authorization, protection of stored cloud credentials, bounded RPCs
and results, job accounting, cancellation, recovery and cleanup of resources it created. Resource
limits bound our orchestration and requested cloud capacity; they are not a hard guarantee about
the tenant's total cloud bill or independently created resources.

The LLM loop, provider keys, existing native tools and existing approvals remain in the server.
Only a custom tool invocation goes to the selected backend. Do not add a Fly variant to
`HarnessKind`, change `ai-agents` into a cloud-specific harness, or allow its host `command` tool.

This supersedes `plan/opencode_microvm_sandbox_plan.md` for custom tools. That older document
describes a different, whole-agent execution design and is not this feature's implementation guide.

## Company settings

Add **Company settings → Tool execution** using the existing company management authorization.
The page shows a provider selector populated by the server registry. Its initial choices are
unconfigured and Fly.io; GCP is not a disabled placeholder or an accepted wire value yet.

For Fly.io collect a dedicated existing app name, preferred region, and an app-scoped deploy
token. The tenant creates the app in its own organization. We create Machines inside that app;
we do not need permission to create organizations or apps. Explain that scripts execute in their
account with its configured networking and charges. App secrets belong to that tenant and may be
used by their tools; never store our orchestration token as a secret on the execution app.

Provide save, test connection, replace credential, enable, disable, and disconnect actions.
Credentials are write-only: reads show credential presence, connection status and last check,
never the value. The connection test checks app identity/access without running tenant code.
It must not claim that read access proves Machine creation/deletion privileges or that an opaque
token has no additional permissions. An explicit test invocation verifies the lifecycle and reports
any cleanup failure. Fly recommends app-scoped deploy tokens for third-party automation, but notes
that deploy tokens also permit some organization networking operations. [Fly token documentation](https://fly.io/docs/security/tokens/)

V1 permits one selected active connection per company. Model connections separately from the
company so later versions can retain multiple connections without adding provider columns to
`companies`. Save configuration changes as immutable revisions. Jobs snapshot the selected revision;
changing the provider/app affects new jobs only. A retired revision remains available for cleanup.
Disable blocks new work and requests cancellation of existing work. Disconnect drains owned
resources before removing credentials; revoked credentials produce visible cleanup-blocked state
and instructions identifying the remaining resources, not a false successful disconnect.

## Provider-neutral model

Domain entities belong in `src/domain/entities/execution.rs`. Use validated newtypes for IDs,
provider resource names, region names and artifact digests; no positional tuples of strings.

| Concept | Contract |
| --- | --- |
| `ExecutionProviderKind` | Closed serialized enum: `FlyIo = "fly_io"` initially. No default fallback on unknown values. |
| `ExecutionProviderDescriptor` | Kind, display name, config version, supported runtimes/resource profiles, availability. Derived from registered adapters. |
| `ExecutionConnection` | Connection ID, company ID, selected revision ID, status and timestamps; contains no plaintext credentials. |
| `ExecutionConnectionRevision` | Immutable typed config, provider kind and credential reference. Persisted so old jobs can be reconciled. |
| `ExecutionProviderConfig` | Tagged, versioned enum with `FlyIo(FlyExecutionConfigV1)` initially; reject unknown fields and mismatched kinds. |
| `FlyExecutionConfigV1` | Validated app name and preferred region. Credentials live separately. No tenant-supplied Machines API base URL. |
| `ExecutionCredentialRef` | Company, connection and credential version identity. Resolve only through a secret-bearing port. |
| `ToolRuntime` | Closed runtime identifier. V1 supports `Python` and `JavaScript`; each maps to an operator-published, versioned runner rather than an executable name supplied by a tenant. |
| `ToolProgram` | Immutable database source or private bundle reference, source digest, runtime, dependency manifest, fixed entrypoint convention and runner-protocol version. |
| `ExecutionRequest` | Company, invocation and attempt IDs, connection revision, artifact, resource profile, deadline and job-scoped bootstrap credentials. |
| `ExecutionHandle` | Provider kind, connection revision, attempt ID, opaque resource/operation reference. No Fly Machine fields in orchestration. |
| `ExecutionObservation` | Pending, running, exited with exit code, or absent; normalized from provider states. |
| `ExecutionFailureKind` | InvalidConfiguration, ReauthorizationRequired, PermissionDenied, RateLimited, CapacityUnavailable, Unavailable, ProtocolViolation. |

Configuration is non-secret typed data. Secret variants are separate, never serialized in ordinary
responses or included in `Debug`/tracing. A future provider can use workload identity or refreshable
credentials; the shared API must not assume every credential is an API-key string.

Proposed non-secret configuration example:

```json
{
  "provider": "fly_io",
  "version": 1,
  "app_name": "acme-mail-tools",
  "region": "fra"
}
```

Separate resource profiles from provider machine types. V1 offers a small bounded set, starting
with `small` (1 vCPU, 512 MiB). Adapters explicitly map supported profiles and reject unsupported
ones. Do not silently change regions, sizes, providers or accounts. Resource capabilities reported
to the UI and validated before submission come from the same descriptor.

## Application ports and registry

Introduce `src/application/execution/` with `ports.rs`, `registry.rs`, `service.rs` and lifecycle
workers. Domain and application types must not import Fly, Google, HTTP or database SDK types.
The provider adapter owns API authentication, wire formats, error normalization and pagination.

The following signatures specify the intended interface; supporting types are defined above and
should be made concrete during implementation:

```rust
#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    fn descriptor(&self) -> &ExecutionProviderDescriptor;

    async fn validate_connection(
        &self,
        context: &BackendContext,
    ) -> Result<ConnectionCheck, BackendError>;

    async fn ensure_submitted(
        &self,
        context: &BackendContext,
        request: &ExecutionRequest,
    ) -> Result<SubmissionOutcome, BackendError>;

    async fn inspect(
        &self,
        context: &BackendContext,
        handle: &ExecutionHandle,
    ) -> Result<ExecutionObservation, BackendError>;

    async fn cancel(
        &self,
        context: &BackendContext,
        handle: &ExecutionHandle,
    ) -> Result<CancellationProgress, BackendError>;

    async fn cleanup(
        &self,
        context: &BackendContext,
        handle: &ExecutionHandle,
    ) -> Result<CleanupProgress, BackendError>;

    async fn reconcile_submission(
        &self,
        context: &BackendContext,
        identity: &SubmissionIdentity,
    ) -> Result<SubmissionResolution, BackendError>;
}
```

`BackendContext` is resolved by the application from trusted company/connection records and a
credential resolver. It carries the matching typed configuration and temporary secret-bearing auth,
not model arguments. Validate provider/config/handle correspondence at every adapter entry.

`SubmissionOutcome` distinguishes accepted handles from uncertain submission. `SubmissionResolution`
distinguishes found resources, proven absence, and still-unknown outcomes; do not collapse these to
`Option`. Cancellation/cleanup progress distinguish requested from confirmed complete. Providers
whose APIs return asynchronous operation IDs must be representable without blocking a worker until
completion. No correctness method has a silently successful default.

`ensure_submitted` uses a stable submission identity for one attempt. A timeout never authorizes
blind resubmission. The adapter reconciles using provider idempotency facilities or deterministic
resource naming and ownership metadata. If it cannot prove absence or find the resource, leave the
attempt uncertain and retry reconciliation with backoff; do not run duplicate customer side effects.

Keep separate cohesive ports for `ExecutionConnectionPersistence`, `ExecutionCredentialStore`,
`ToolArtifactStore` and `ExecutionLedger`. The ledger exposes transactional operations for admission,
claim/renew, result acceptance, cancellation and cleanup transitions, all with execution fences.
No default implementations, generic unscoped get-by-ID operations, or raw SQL in services.

`ExecutionBackendRegistry` follows `HarnessRegistry`: reject duplicate/mismatched registration,
require an exact kind, and enumerate registered descriptors deterministically. Register Fly.io in
`src/infra/setup.rs` only when its adapter is available. A company with an unsupported stored provider
gets an explicit unavailable state; there is no fallback to our account or local execution.

## Persistence and credentials

Add additive migrations for:

- `company_execution_connections`, immutable `execution_connection_revisions`, and a company default
  selection with a composite foreign key proving that the selected connection belongs to it.
- `execution_credentials`, using the existing envelope-encryption mechanism with a new execution
  context bound to company, connection, provider and credential identity. Extend rotation/inventory
  commands to include these records. Do not pretend Fly is an email/chat transport.
- `company_tools` and immutable `company_tool_versions`, including schemas, artifact reference,
  runtime and timeout. A version may store one bounded UTF-8 Python or JavaScript source document
  directly in PostgreSQL. Store larger or multi-file bundles through a private storage port, not in
  prompt YAML.
- `tool_invocations` for logical calls and `tool_execution_attempts` for remote attempts. Include
  connection revision, tool version, normalized input digest, resource handle, deadline, ownership
  generation, result state, retry counters, next-attempt time and cleanup state.
- A durable bootstrap/result credential record storing token hashes and binding each capability to
  company, invocation, attempt, purpose and expiry.

Use composite tenant foreign keys for all company-owned associations. Restrict deletion of a
connection, tool version or credential needed by active jobs/cleanup. Index due work, ownership
expiry, uncertain submissions and cleanup independently. Bound retained inputs/results with a
retention worker; diagnostic history contains identifiers, counts and categories rather than tokens
or full bodies. Prevent a single provider app from being accidentally shared by two companies via
an external-resource uniqueness policy; allow successive revisions of the same company connection.

## Custom tools and skills

Generalize `NativeToolDeclaration` into an owned `ToolDeclaration` so names/descriptions can come
from tenant records. Preserve source identity (`Builtin`, `Native`, `CompanyCustom`) and reserved
platform names. Resolve model-visible aliases to immutable tool versions inside the trusted run.

Compose the existing `HarnessToolHost` with a `CompanyToolHost`. The latter calls the execution
application service, which resolves the company connection and backend registry. Provider selection
does not belong in tool arguments, skill steps or `ai-agents` runtime YAML.

Keep the static built-in allowlist intact. Introduce a resolved company catalogue that joins it with
authorized custom tool versions. Split structural skill validation from tenant-aware reference
resolution, updating agent writes, skill writes, library copies and compiler grant filtering together.
A global library skill cannot hold a reference to a particular tenant's tool or credential.

Retain the existing effective-grant behavior: attaching a skill includes the tools its steps require.
Show those requirements in the existing picker and apply the same company-management permission as
direct tool grants. Published tool/skill dependency snapshots are immutable; changing their contents
requires publishing/selecting a new version. Do not create an additional approval ceremony for
tenant scripts or their networking. Existing native-tool approval requirements remain authoritative.

Both explicit skill steps and model calls dispatch through the same company/version authorization
and ledger. Execution target and company identity always come from the server's run context.

### Python and JavaScript program contract

V1 supports dynamically saved single-file programs in both runtimes. Editing source publishes a new
immutable `company_tool_version`; it never mutates the program referenced by a running or historical
invocation. Persist the runtime identifier, bounded source, SHA-256 digest, dependency lock manifest,
input/output schemas and runner-protocol version together. Recompute and verify the digest before
delivery. Use a database constraint and application validation to reject invalid runtime/source
combinations and oversized source or manifests.

The runner writes database source as data into a fresh workspace, then launches a fixed argument
array without a shell:

```text
Python:     [platform Python interpreter, "/workspace/tool.py"]
JavaScript: [platform Node.js executable, "/workspace/tool.mjs"]
```

The actual interpreter paths and versions belong to the signed runner image and provider descriptor;
tenant input cannot replace them or add command-line flags. Both programs read exactly one bounded
JSON value from stdin and write exactly one JSON value to stdout. Exit status, bounded stderr and
malformed/oversized output are protocol outcomes, not model replies. The runner does not evaluate
source through `sh -c`, interpolate source/arguments into commands, or put source in environment
variables or provider Machine configuration.

Source-only programs may use their runtime's bundled standard library. Third-party dependencies use
a bounded, exact-version lock manifest stored with the tool version and are resolved inside the
tenant execution environment. Dependency installation never occurs on the mail server. A later
optimization may turn the source plus lock manifest into a content-addressed private bundle/cache;
the immutable `ToolProgram` contract and digest remain unchanged. Do not reuse a writable dependency
cache across companies.

## Invocation and recovery protocol

1. Resolve the granted tool version and selected enabled connection. Validate input against a bounded
   schema (disable remote schema-reference fetching), runtime/profile support and enforced limits.
2. Atomically admit a logical invocation under global/company capacity and task ownership fences.
   Persist the request digest and immutable execution snapshot before contacting the provider.
3. Claim a submission attempt with a lease. Create its stable submission identity and job credentials.
   Submit via the backend adapter. If the response is lost, reconcile that same identity.
4. A versioned runner fetches that attempt's artifact and JSON input over our fixed HTTPS job API.
   Credentials allow access only to this attempt. Bootstrap redemption is atomic; delivery retries
   have defined idempotent behavior, so a lost HTTP response does not strand the job.
5. The runner invokes the tenant script with JSON on stdin, collects bounded JSON output, and sends
   it to the result endpoint. Separate diagnostic stderr from the result. Never interpolate model
   arguments into a host shell command. Script execution and dependency installation happen solely
   in the tenant environment; package builds also never execute on the mail server.
6. Accept a result transactionally only for the matching live attempt and current execution fence.
   Repeating the identical submission returns the original acknowledgement; a conflicting or stale
   result is rejected. A job token proves which job replied, not that tenant code reported the truth.
7. Record the tool result durably and return it as untrusted tool data to the waiting harness.
   Provider failure, script failure, timeout and cancellation remain distinct typed outcomes.
8. Schedule cleanup independently of result delivery and verify resource removal. Successful output
   must not erase an outstanding cleanup obligation.

V1 uses short tool calls awaited by the existing harness within its agent deadline. Submission,
monitoring and cleanup are supervised durable workers, and waiting for a result is cancellable.
Do not invent transparent resumption of arbitrary `ai-agents` internal state. If an agent run is
retried, stable persisted logical call identities are required for result reuse; a regenerated model
call ID or input hash alone is insufficient to distinguish an intentional repeat from a retry.
Until replay identity is proven, an uncertain side-effecting call is surfaced for resolution rather
than automatically re-executed. Exactly-once effects at arbitrary third-party APIs are not promised;
expose a stable logical operation ID to tenant scripts for their own idempotency.

Task cancellation/lease loss invalidates job capabilities and durably requests remote cancellation.
The new owner must not launch a replacement for uncertain old work. Network partitions can prevent
immediate termination; mark it pending, reconcile remotely, and prevent stale server-side commits.
A tenant can modify its own Machine or continue external work: our cancellation contract covers
provider operations and our authority, not a guarantee that already-issued external effects cease.

Use an external sweeper and durable attempt identities for crash recovery, not Rust `Drop` as a
promise of asynchronous deletion. Reconcile attempts whose creation returned no resource ID. Only
delete resources whose connection, deterministic identity and ownership metadata match our ledger.
Never sweep every Machine in the tenant app indiscriminately.

Proposed initial admission limits: 60-second default/300-second maximum tool duration (also bounded
by remaining agent time), 256 KiB input, 256 KiB output, 64 KiB retained stderr, 10 MiB artifact,
2 active attempts per company and 16 globally. Enforce byte limits while streaming before parsing,
and runtime/disk limits in the runner. Schema size/depth, extracted artifact size/file count, tool
count, HTTP timeouts, polling, retry budgets and retention also receive explicit tested bounds.
Reserve global/company slots transactionally across server instances; cleanup-pending or uncertain
resources continue to count until their termination is confirmed. Threshold changes must retain CI
tests that fail when the bounds are approached again.

## First adapter: Fly.io

Implement `src/adapters/execution/fly_io/` with a reusable bounded HTTP client, typed request/response
DTOs, status mapping, submission reconciliation and cleanup. Use the fixed public Machines API
origin; credentials go only to that origin, with cross-origin redirects refused. Fly documents the
public endpoint and per-action rate limits; account for these in polling/submission backoff rather
than treating a small HTTP timeout as permission to retry creation. [Machines API access](https://fly.io/docs/machines/api/working-with-machines-api/)

Use operator-published Python and JavaScript runner images pinned by digest. Database-backed source
is fetched through the job API; private bundles use the same immutable program identity. Runtime
variants package the interpreter and protocol wrapper, while tenant dependencies are installed or
loaded only inside the tenant environment.
No Fly token is passed to a Machine. The Machine gets only runner configuration and job-scoped
credentials; tenant app secrets and networking are tenant-controlled.

Create a fresh Machine per attempt with deterministic identity/metadata, a supported fixed resource
profile, no platform-requested public services or persistent volumes, and restart disabled. The
runner exits after one job. Verify the current API's naming, create/launch, stop/delete and restart
semantics in adapter integration tests. Do not assume a universal Fly hard TTL or an idempotency
header exists. Provider auto-destroy, if used, is an optimization with cleanup reconciliation as the
authority. [Fly Machine lifecycle API](https://fly.io/docs/machines/api/machines-resource/)

When token access expires or is revoked, stop submitting, mark reauthorization required, retain
resource identities and show cleanup status. Rotating a token for the same app can restore cleanup;
changing the app must create a new revision without redirecting old cleanup to the new app.

## Adding Google Cloud Platform later

Add a concrete service adapter once chosen, for example a jobs service; do not assume Google has
the same Machine semantics as Fly. Before shipping it:

1. Add its `ExecutionProviderKind` and typed config variant with project/location/service-specific
   identifiers. Extend credential handling for its actual authentication method.
2. Implement the same backend port, including asynchronous operation handles, cancellation and
   discovery of uncertain submissions. Reuse the runner's artifact/input/result protocol.
3. Map supported runtime/resource profiles explicitly. Validate capability differences through the
   provider descriptor rather than provider conditionals in the agent or skill layers.
4. Extend persistence constraints, descriptor-driven company settings forms and adapter setup.
5. Pass the shared lifecycle contract tests and provider-specific integration tests before adding
   the provider to the available registry. Unsupported capabilities must fail visibly.

Company settings and adapters are the expected places for provider-specific changes. Tool version,
skill, harness, invocation ledger, callback and application orchestration contracts stay shared.

## Implementation phases and acceptance gates

1. **Domain and ports:** add validated types, provider configuration, descriptors and registry.
   Test unknown providers/config fields, kind/config mismatch, invalid limits and duplicate registry
   entries. Keep Fly as the only initial supported kind; do not advertise it as usable before wired.
2. **Company configuration:** migrations, credentials, rotation support, management use cases and
   settings routes/UI. Test cross-company reads/writes and selection, ciphertext context substitution,
   credential redaction, revision switching, disable/disconnect and reauthorization behavior.
3. **Tools and skills:** immutable artifact/version storage, publication and validation, company
   catalogue, declarations and host composition. Test reserved-name collision, cross-company tool
   references, library copy behavior and grants for both skill and model calls.
4. **Ledger and protocol:** transactional admission/claim/result/cleanup operations, runner job API,
   supervised workers and cancellation integration. Use competing database claimants to prove slot
   limits, one submission owner, replay-safe result acceptance and stale-generation rejection.
5. **Fly adapter and runner:** verify full create/run/result/delete behavior in a dedicated test app.
   Inject response loss after creation, delayed results, duplicate callbacks, rate limits, auth loss,
   script crashes, process trees, oversized outputs and failed cleanup. Repeat against a fake backend
   with delayed asynchronous operations to prove the abstraction is not secretly Fly-specific.
6. **Rollout and operations:** add execution status and cleanup visibility; document tenant setup and
   responsibility. Gate enablement on successful tests and expose only actually registered providers.
   Track queue age, active resources, uncertainty, cleanup debt and credential failures by company.

Each queue needs a poison-batch test proving unchanged failures are not immediately reclaimed. Test
server death between durable admission and submission, after submission but before saving its handle,
and after result commit but before cleanup. Connection switches must preserve old-job cleanup.

Run formatting, offline locked compilation, migrations and database-backed tests in CI, regenerate
`.sqlx` after every query change, and retain `scripts/stack-budget.sh` when wiring the tool-await
boundary. Box external async seams as needed rather than raising stack limits. Provider integration
tests use a dedicated explicitly configured test account; ordinary tests never spend tenant funds.

## Deliberate exclusions from v1

Whole-agent cloud execution, provider fallback, platform-paid execution, arbitrary user-supplied
provider URLs, GCP implementation, automatic app creation, persistent workspaces, warm Machine reuse,
script security review and platform-managed egress filtering are outside this plan's initial scope.
An independently deployed tenant controller that keeps cloud credentials entirely outside our server
can later implement the same lifecycle contract; it is not required for the direct Fly integration.
