# Vault07 independent reviewer findings

## VG-P07-A1-R1 — 2026-11-08 architecture disposition

**No actionable architecture findings.** Approved immutable VG-P07-D1/r1 SHA `241766fb8efd2af70cbdc5c8d8a5e7dcb23dcd4db63e7db3c1e52b4937049484`; C006/r1 may activate. No VG-P07-A1-F IDs allocated. Exact source remains accepted06 `d9d84e7e35bc0a1d603f6f57bf2ac6ba5a9c95a133611bba00b432fe31533b71`, HEAD `ace526fd8259ec53c5199ffa7555a1a99dcba58e`, coordination22. Requested reviewer Astra/high; actual runtime settings not exposed. See [notes](agent-review-notes.md#vg-p07-a1-r1) for coverage, frame-overflow clarification and implementation verification obligations. No implementation or runtime acceptance is implied.


## VG-P07-B1-F1 — Control transport does not bind request/cancellation identity to trusted context

**Classification:** actionable defect; medium; verification / requirements. **Status: OPEN.**

2026-11-08; implement-astra-reviewed-2-2; phase07 whole batch B1, code review VG-P07-B1-R1. Reviewer B, requested Astra/high; actual runtime model/effort not exposed. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`, HEAD/review base `ace526fd8259ec53c5199ffa7555a1a99dcba58e`, artifact `8ce396f8ef9b731cb1f33846c82a30de0e8d9852623618e8fec9218375267cc4`. [Task](TASK.md), [round notes](agent-review-notes.md#vg-p07-b1-r1), [review](REVIEW.md).

**Evidence and impact.** `src/application/integrations/wire/control_transport.rs:7–34` decodes the request and checks trusted owner, equal deadline, current deadline and the supplied local token's cancellation state. It does not compare `request.identity` or `request.cancellation` to trusted expected identities. Its signature has no expected identity or context factory from which to obtain them. It simply returns the decoded identity in the response. `wire/mod.rs:40–57` checks matching invocation identities only for execution commands; it provides no such check for management/connect/grant commands. Consequently another valid request/cancellation ID can be substituted while retaining the same owner/deadline and an unrelated uncancelled local token, and the control gateway still runs. This defeats the exact context association required by accepted D1/r1, paragraph 2. Execution/discovery harnesses perform stronger original-request matching; the control path does not establish the same conformance property.

The negative cases in `connect_tests/control_wire.rs:5–52` cover owner, deadline, expiry, an already-cancelled passed token, and version. None substitutes cancellation or request identity. `workflow/vault_tests/native_control.rs:30–61` creates independent wire IDs and a new local token without associating them. The actual persisted grant consent checks remain meaningful and passed; this finding does not allege forged owner capability, a deployed remote endpoint, or a production authorization exploit. It is an acceptance-blocking gap in the promised local serialization/context proof. [Static source/caller evidence](logs/reviewer-control-context-audit.json); no executed private-function attack probe is claimed.

**Required correction and verification.** Bind control envelopes to trusted expected request/cancellation identities and the corresponding local owner/deadline/token before calling the gateway. Add real control-path negative cases that substitute each identity or cross-associate same-owner/same-deadline contexts and prove zero management/connect/grant work, while preserving positive current-context cases. Do not synthesize authority or cancellation from decoded payload fields.

**Instruction lesson.** D1 already specified this invariant; the implementation and negative-case matrix omitted the association for one command family. Candidate guidance for protocol conformance reviews: test substitution of each opaque identity against a distinct trusted local context, not only cancellation of the token already passed. This is an implementation/proof defect, not a newly contradicted architecture decision.

**Resolution history:** 2026-11-08 opened against the exact B1 artifact above. No correction reviewed.

## VG-P07-B1-F2 — Credential serialization gate loses taint through imported aliases

**Classification:** actionable defect; medium; verification / security boundary. **Status: OPEN.**

2026-11-08; implement-astra-reviewed-2-2; phase07 whole batch B1, code review VG-P07-B1-R1. Reviewer B, requested Astra/high; actual runtime model/effort not exposed. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`, HEAD/review base `ace526fd8259ec53c5199ffa7555a1a99dcba58e`, artifact `8ce396f8ef9b731cb1f33846c82a30de0e8d9852623618e8fec9218375267cc4`. [Task](TASK.md), [round notes](agent-review-notes.md#vg-p07-b1-r1), [review](REVIEW.md).

**Evidence and impact.** `scripts/integration-boundary.py:110–132` discards `as Alias` while collecting import paths; `:134–166` builds credential taint from declaration identifiers without importing that alias relationship. A public DTO using `use secrecy::{SecretString as PrivateSecret, ExposeSecret};`, a `Leaked { value: PrivateSecret }` struct with a manual `serde::Serialize` implementation exposing that value, and `Reply::Leak(Leaked)` produces **no violations**. This is a real compiling Rust mutation, not an unresolved fixture type: [source](logs/reviewer-boundary-alias-source.rs), [checker result](logs/reviewer-boundary-alias.json), [rustc receipt](logs/reviewer-boundary-alias-compile.json), [compiler log](logs/reviewer-boundary-alias-compile.log). The checker SHA is recorded with the reproduction. The temporary fixture module tree and emitted metadata were removed; no project source changed.

The existing 15 mutations and current tree check pass, but fail to cover this spelling of the forbidden type. CI and original07A require a regression guard preventing serializable private credentials/results; `storage_tests.rs:345–362` also delegates its custody assertion to this checker. The accepted frame types do not currently contain this leak; the defect is that a routine aliased import bypasses the promised gate and a future leak would remain green. The result projection check is bypassed as well as the manual-serialization check.

**Required correction and verification.** Preserve relevant import/re-export alias relationships in the boundary analysis (or use an equally reliable type-aware check). Retain a compiling negative mutation for an aliased credential wrapper with manual serialization and reachability from `Reply`; verify the actual production checker rejects it. Keep the narrow command-only secret submission exception and valid DTOs working; do not broaden the allowlist.

**Instruction lesson.** Original07A and D1 require a substantive boundary check; passing a small fixture set is not proof that names resolve correctly. Candidate guidance for source-analysis gates: include compiling alias/re-export mutations for each type-based safety rule. Root cause beyond the observed missing alias propagation is unknown. No agent instruction edits are authorized by this finding.

**Resolution history:** 2026-11-08 opened against the exact B1 artifact above. No correction reviewed.


## VG-P07-B1-R2 — correction dispositions, 2026-11-08

Independent B rechecked the combined six-path correction under implement-astra-reviewed-2-2. Requested Astra/high; actual runtime settings not exposed. Worktree `/Users/mac02/.codex/worktrees/1133/openbot`, HEAD/base `ace526fd8259ec53c5199ffa7555a1a99dcba58e`, artifact **`ad2cae3d8ce497444a91a39bf14fb21f8ecdc27ed547a842a97dbee4cbf9cd48`**, coordination23, unchanged D1/r1. [Round coverage/evidence](agent-review-notes.md#vg-p07-b1-r2), [review](REVIEW.md).

- **VG-P07-B1-F1 — FIXED.** `wire/control_transport.rs:10–72` requires original LocalContext, preserves trusted owner/work/token/consent and binds both opaque identities plus exact frame digest before gateway entry. Reviewed all callers and management/connect/cross-context/grant regressions. Independent23 service-contract tests pass, including genuine positive consent/revoke and cancelled/expired cases. No decoded authority or replacement cancellation introduced.
- **VG-P07-B1-F2 — FIXED.** `scripts/integration-boundary.py:126–201` propagates import/re-export and serialization-trait aliases while retaining scoped DTO declarations and command-only exceptions. Independent original compiling mutation and helper-module alias-chain probes now report both expected violations;22 mutations/current source gate pass. [Direct reproduction](logs/reviewer-B1-R2-alias-reproduction.json). This closes the demonstrated alias bypass, not a claim of arbitrary Rust serializer/data-flow analysis.

Both original findings and evidence remain above. No open actionable findings remain in reviewed phase07 scope. Verdict **PASS** for the exact artifact identified here.
