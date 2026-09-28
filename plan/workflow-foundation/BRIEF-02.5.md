#02.5 — Binding revisions and resource readiness

Original phase02 Publication item3; shared EXPANSION.md contracts apply. Depends on
verified02.4. Application-owned immutable configuration captures one PublishedBundle Arc,
positive binding revision, company, parameters and exact declared slot-to-resource IDs.
No credentials or live provider clients in configuration. Constructor checks bundle
ownership, schema/size-valid parameters, exact slots before accepting owned values.

Activation readiness reads current resource facts through a required narrow application
port, scoped by company and authenticated actor; validates returned identity/ownership,
current authorization/revocation, kind, required contract and readiness. Provider adapters
are responsible for deriving truthful supported contracts/readiness; directory errors
propagate. This check neither grants management authorization nor commits activation:
02.7 lifecycle commands own management checks/CAS,03 persistence owns atomic admissions,
04 use-time effects recheck current secrets/revocation. No boolean cached ready flag.

Types: domain WorkflowBindingId, BindingRevision, RuntimeResourceId; application binding
module ConfiguredBinding/BindingConfiguration, ResourceDirectory/ResourceStatus. Revisions
are independent immutable values; replacing the selected revision cannot mutate earlier
configuration. No local mutable head or duplicate persistence owner is introduced.

Tests: schema/type/null/size failures; missing/extra slots; foreign bundle; wrong/missing/
foreign/revoked/incompatible/unready/unauthorized resources; directory errors; readiness
rechecked each call; prior revisions unchanged. Competing revision CAS tests belong to
owning02.7/03 adapter, since02.5 introduces no write/concurrency protocol. Checks: workflow
suite2MiB, offlinealltargets/check/Clippy, fmt/diff, graph, independent Astra review.

## Implemented; independent review pending

Changed `application/workflow/binding/{mod,resources,tests}.rs`, module export,
domain ID declarations/exports and language docs. Self-audit: private immutable
configuration, validated parameters before storage, exact slot cardinality/membership,
current returned identity/company/auth/revocation before readiness, bounded contracts,
propagated lookup failures; no credentials/state owner/concurrency protocol added.

PASS:139 workflow tests at2MiB (`SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test
--locked --offline --lib workflow`); locked offline all-target check and Clippy with
-D warnings; fmt/diff and graft. Logs `/private/tmp/workflow-02-5-{tests-final,
check-final,clippy,fmt,diff,graft}.log`. Initial failed compile was corrected import
of AppError/AppResult; no failed final gates. No SQL/DB/services/commits.

Independent review **PASS for02.5 library configuration/readiness scope**, no findings.
Reviewer `/root/review_02_4_resume`, verified UUID
`01a0e448-7a4f-77d1-9a4d-2bfe7acbaeb7`, Astra/medium, final27.42%
70,841/258,400 at2026-09-27T19:31:14.604Z (token_count.info.last_token_usage/
model_context_window). Original item3's production activation, atomic revision
selection and competing-revision DB tests remain required at02.7/03 integration.
No outstanding checks for this library scope.
