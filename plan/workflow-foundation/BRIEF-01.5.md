# 01.5 authorization outside prompts

Original phase01 item5/README govern. Astra `/root/astra_01_5`, session `01a0de17-e043-7f52-8ef2-c0ff3079122a`. Preserve verified01.1–01.4. No blocker. Implement actual lifecycle authorization, not future action/runtime/public route/binding/publication work. No compatibility constructor/bypass.

## Required contracts and behavior

Add application/workflow/authorization.rs: checked authenticated user ID, explicit current operation capability Admit/Cancel, required authorization interface with concrete implementation using existing inner-layer readers. Load current identity with PrincipalAccessPersistence::access_context_for_user(company,user); no supplied membership/grants/prompts/scope token as proof. Reuse CompanyMembership::manages_company_operations: owner/admin current management operations allowed, member/outsider denied. This bounded policy follows existing automation management; later message/schedule/child authorities must be explicit, not permissive System now.

RelatedAssociation is company-only/channel/thread-in-channel using distinct typed IDs. Containing command supplies CompanyId. Independently load ChannelPersistence::get_by_id and ThreadPersistence::get_thread_by_id, check returned IDs/channel company/thread channel association; reuse Channel::viewer_access. Admin does not bypass allowlist, owner exception remains. Thread has no company column; verified channel association establishes tenant. Store association in PreparedAdmission/RunHead, include in idempotency equivalence. Actor is auth context not automatically dedup identity; every replay reauthorizes. Cancellation uses stored head association, never request replacement.

WorkflowService requires authorizer: company operation permission before protected definition lookup or run inspection; then scope/integrity checks and related visibility before mutation. Preserve atomic admission, CAS cancellation and all existing outcomes. No actorless overload. Raw persistence ports remain trusted internal contracts, not authenticated APIs.

Nonempty unresolved ResourceRequirement declarations must reject admission with controlled BadRequest until phase02 resolves company resources and phases04/06 authorize effects/tools. They are names/kinds/contracts, not actual grants. No executor currently exists; do not add invented action service or allow-all authorizer. Update docs to state lifecycle auth implemented, resource/execution auth deferred. Registered step names/structural validation/bounded input do not grant tools.

Missing/foreign/inaccessible channel/thread and denied company operation -> non-disclosing NotFound. Reader infrastructure errors propagate unchanged with ?. Existing mismatched version/run envelopes remain Internal errors. Preserve CancelResult::NotFound/CAS/terminal semantics for authorized callers. No database outage converted to absence/decision.

Public docs: authenticated actor provenance, manager policy, independent visibility, scoped IDs, prompt-independent grants, replay reauth, association dedup, errors. Preflight not durable protection from revocation races; future persistence/effect commit must recheck current authority transactionally where required. No new locking protocol now.

## Repository references and scope

workflow/service.rs:15–77 constructor/admit/cancel; contracts.rs:55–93,:159–188 requests/head; ports.rs:14–62. Narrow PrincipalAccessPersistence application/use_cases/participant/mod.rs:68–83 (SQL adapter exists participant.rs:526–554, do not import). Membership decisions domain/entities/company_member.rs:76–87; channel viewer policy channel.rs:250–265. Existing application channel read pattern use_cases/channel.rs:453–486; thread lookup use_cases/thread/mod.rs:209, independent association checks:1276–1400. Company management access use_cases/company.rs:340–364. Thread domain thread.rs:8–18. WorkflowService indexed callers empty but exhaustive grep shows only export/service/test factory tests.rs:319–323. No production wiring migration. Unresolved resources domain/workflow/definition.rs:26–30.

Graft/callers before existing edits. Add contracts+authorizer, integrate service+immutable association, update store/dedup/factory/fixtures, add sibling authorization tests, public docs, final checks/graft. Reuse narrow abstractions; no SQL/cache/dependency/legacy caller/commit/deploy/reset changes. Parent owns records.

## Acceptance

Owner/admin admit/cancel company-only; member/outsider denied. Admin team allowed, ungranted allowlist denied, granted admin allowed, owner exception. Foreign/missing/wrong-ID channel; missing/wrong-ID/wrong-channel thread reject before writes. Company/run access cannot bypass related visibility. Cancel uses stored association. All principal/channel/thread errors propagate without writes. Revocation before replay denies; changed association same key conflicts. Unresolved resource declarations fail closed. Preserve prior atomic admission/replay and claim/cancel controlled/barrier races.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

Baseline domain26/application13. No service/DB/temp cleanup or new concurrency protocol. Later public manual-start policy and trigger authorities must be implemented by their owning phases, not inferred as complete from this internal management boundary.

## Review 1 corrections

Behavior passed inspection, parent final-tree46/46 at2MiB. Required fixes:
1. Public docs authorization.rs:74–108: concrete owner/admin Admit/Cancel, nondisclosing NotFound, unchanged reader errors. Unscoped reader results untrusted until IDs/company/visibility/thread relationship verified. Document service nonempty-resource rejection and no tool/execution authority. domain/workflow/definition.rs:18,26–30 distinguish lifecycle auth implemented vs resource/execution deferred. contracts.rs:153–155 Conflict includes changed related association.
2. tests/authorization_cases.rs610lines; functions82–168,212–308,311–422,479–576 exceed~80. Split behavioral sibling modules and small shared fixtures/helpers preserving ALL assertions. No behavioral feature change/new framework.
Rerun fmt/diff/workflow2MiB/offlinealltargets, then same Astra review. Previous reviewer test invocation interrupted waiting build lock, not evidence; parent confirmed46.
