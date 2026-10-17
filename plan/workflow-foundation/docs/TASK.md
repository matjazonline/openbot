# 04.8 continuation under bb-implement-astra-opus — task record

Root: thread `thr_8m3bvxma9d` (claude-code, Opus 5.5; effort not authoritatively
confirmed, so root is a thin coordinator and fills no role). Started 2026-10-15.

## Original requirements

- [Phase 04](../04-actions-http-and-delivery.md) execution item 7 and its acceptance
  section (competing dispatchers, stale workers, duplicate callbacks).
- [EXPANSION](../EXPANSION.md) point 04.8.
- Accepted architecture: [BRIEF-04.8](../BRIEF-04.8.md), SHA256
  `3f76f3181278bfff7744a47e6701add420fe22d0090f6145a6a0eac89885cd04`
  (independent PASS `/tmp/workflow-foundation-20261013-04.8/reviewer/ARCHITECTURE.md`).
- Prior state: [handoff](../HANDOFF-20261014-04.8.md),
  [continuation](../CONTINUATION-20261013-04.8.md), author handoff
  `/tmp/workflow-foundation-20261013-04.8/author-impl/AUTHOR-HANDOFF.md`.
- Repository rules: `/home/mh/dev/openbot/AGENTS.md` and `src/**/AGENTS.md`.

Not authorized: 04.9, migration squash, commits, deployment, production data.

## Baseline and preserved state

- Group A source was committed by the user in `67d319b`; current HEAD
  `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`, worktree clean apart from
  untracked dotfiles/IDE dirs (preserve; not task content).
- Group A artifact = HEAD content of the seven files in reviewed manifest
  `source-manifest.json` SHA256 `9148f4c1…5b57`; all seven re-hashed OK
  2026-10-15 ([log](logs/groupA-source-reconcile.txt)). Base for group A diff: `5b3d7dc`.
- Prebuilt test binary `target/debug/deps/mail_agents-43f563ba6b6c24ac` present
  (recorded SHA256 `f14fb5cd…7e`; D must re-verify before reuse).

## Batch order

1. **Group A verification** (code already PASS by prior independent reviewer):
   dispatch, receipt operational-error/refused-return and affected
   action/authority/control/reconciliation filters at stock 2 MiB; SQLx
   prepare/check; fmt/diff/offline check; strict Clippy. Root acceptance after.
2. **Group B** boundary matrix (BRIEF-04.8 §B) — S guidance, D expansion/impl,
   S whole-batch review. Starts only after group A acceptance.
3. **Group C** final combined gates (BRIEF-04.8 §C), S integration review.

## Workers

| Role | Thread | Provider/model/effort | Assignment |
| --- | --- | --- | --- |
| S (retired 82% ctx, after R-009) | `thr_xku2jgixtv` | codex / gpt-6-astra / high | stopped |
| D (retired 64% ctx, after B2r1) | `thr_6iqmr9bdad` | claude-code / claude-opus-5-5 / high | stopped |
| S2 (retired 83% ctx, after R-012) | `thr_4pqv29gbbd` | codex / gpt-6-astra / high | stopped |
| S3 | `thr_4e8he92jaz` | codex / gpt-6-astra / high | C1 review + group C integration review |
| D2 | `thr_2n9nrfy6ra` | claude-code / claude-opus-5-5 / high | B3, group C, C1 |

## Resources

- D owns builds, tests, and the task PostgreSQL fixture
  `workflow-foundation-048-20261013` (ID `f4e50996…dc40`, 127.0.0.1:55448,
  system id `7696100747480141867`). Docker requires an escalated
  `newgrp docker <<'EOF' … EOF` shell. Never touch other databases/containers.
- S is read-only on project files and fixtures; it may run non-mutating reads.

## Status / next action

- Group A: ACCEPTED 2026-10-15 on manifest `64ce8860…` (see REVIEW.md).
- Group B: B1 ACCEPTED (`ef2e0ecd…`), B2 ACCEPTED (`1b109cc6…`); B3 ACCEPTED (`c3379ee3…`).
- Group C: ACCEPTED 2026-10-16 (manifest `5f818107…`, R-017). **04.8 complete.**
- Next: 04.9 only under a new user instruction. Workers stopped; fixture container
  `workflow-foundation-048-20261013` stopped with data retained.

## Decisions log

- 2026-10-15 user: strict Clippy on Rust 1.99 — add crate-level
  `allow(clippy::double_must_use)` with a reason comment (async-trait expansion)
  and rename the 5 `fetch_update` calls to `try_update`. Assigned D-002.
- 2026-10-15 root: `workflow_action_` full-filter failures under load (inherited,
  reproduced on base) routed to S as S-002 for diagnosis/correction criterion.
- 2026-10-15 user: fix the 5 Linux timestamp-precision test failures within 04.8
  (test-only unless S finds a production precision bug). Assigned S-012 → D.
- 2026-10-15 user: timing-sensitive gates — rerun when host load is lower
  (user's other project bizi44 was running cargo test, load ~19 on 8 cores), or
  change verification so it does not depend on timing. S-012 assesses the latter.
- 2026-10-15 user: include F-006 production fix (wait park returns persisted
  deadline) in 04.8. Batch C1 = DEC-013 precision fixes + F-006 + DEC-014 test
  stabilization; full-suite/budget reruns on a quieter host. Assigned D-011.
- 2026-10-15 21:01 user committed `cfef3d8` "workflow-foundation 4.8" (accepted
  B3 content + records). Manifests keep comparing worktree content against
  `8533a79`; C1 freeze records HEAD `cfef3d8` as well.
- 2026-10-16 root: F-009 (inherited stop-task test expectation defect, test-only)
  fixed within 04.8 as it blocks the required full-suite gate (same policy as the
  user's precision-fix decision). S3 accepts a clean stack-budget full-suite run as
  full-suite evidence. Assigned D-014.

---

## 04.9 continuation — 2026-10-17 — implement-astra-reviewed-2.1

Original instruction: use `implement-astra-reviewed-2.1` and continue
`plan/workflow-foundation/RESUME.md`. Root interprets the new continuation as
authorization for the next point, **04.9 only**. Original requirements remain
`04-actions-http-and-delivery.md` (HTTP and registered tools; relevant shared
action acceptance) and `EXPANSION.md` point04.9 and its cross-point reconciliation.
04.8 history above is preserved. No commits, migration squash, deployment,
production resets,04.10 MCP implementation,04.11 messaging, or06/08 wholesale
legacy replacement are authorized.

Baseline: `2729f2caaf58830bea8291bdcc6f9882a3ad03ee`. Preserve pre-existing modified
`AGENTS.md` and untracked `.agents/skills/implement-astra-reviewed-2/` and
`.agents/skills/implement-astra-reviewed-2.1/`. Author `/root/author` requested
`gpt-6-astra` / medium with independent context; root model not authoritatively
exposed. Reviewer pending. Context threshold controls/occupancy unavailable;
host defaults and durable checkpoints used (75% target cannot be enforced).

### Scope and cohesive batch

One HTTP/tools batch: company-owned fixed-origin HTTP resources, current
encrypted credentials and policy/revocation, guarded HTTP adapter, trusted
registered-tool routing through the existing action service, readiness and
step/direct-tool entry integration, hostile transport and persistence tests.
Existing `ActionService` receipt-before-return, possible-dispatch ledger,
current authority checks and cancellation supervision remain authoritative.

### Architecture decision revision 04.9-D2 — proposed, awaiting B

D1 retained at `logs/04.9/review-D1.txt`. D2 resolves04.9-F001..003;
reviewer `/root/reviewer`, requested `gpt-6-astra` / medium, independent context.

1. Additive company HTTP connection/credential/operation-policy schema. Connection
   identity and origin immutable; origin replacement creates a new identity, so
   an already frozen action cannot silently target another service. Enabled state,
   current credentials and registered policies remain revocable. Tenant participates
   in credential/policy FKs. Revisioned management is company-admin authorized;
   dispatch retains existing current owner/admin plus frozen capability/resource
   authorization. Ordinary membership is insufficient at management, publication,
   binding, admission, action preparation, dispatch and replay boundaries.
   Credential envelopes use a separate tenant+connection-bound context and join the
   existing credential inventory/rotation path. No credentials enter frozen actions.
2. Trusted operation registration supplies bounded `ToolSnapshot` contracts for a
   connection. Registrations permit only remote/reconcile operations initially:
   HTTP verbs and headers never establish replay safety. Policy changes cannot
   broaden an old snapshot. Binding aliases are not grants: selected resource ID
   must match the saved run binding; current contract comparisons retain all schema,
   capability and recovery fields. Unknown registrations fail closed.
3. Implement production `SqlActionAuthority` using the existing company/actor
   locks before run/attempt locks (`action_authority::prelock_on` then
   `authorize_prelocked_on`); lock the connection and its policy only after those
   owners and before action intent. Never acquire company/actor locks beneath a
   connection or run lock. Management locks company/actor then connection and
   never a run; publication/binding/admission retain company/actor-first order.
   Credential writes/deletes and provider credential reads lock their parent
   connection first; envelope rotation retains its existing ciphertext-CAS update
   and never acquires a connection/run lock beneath a credential row lock.
   Recheck
   current connection/credential/policy at provider use. Remote network I/O never
   runs in the SQL authorization transaction. Revocation after entry cannot retract
   an already started remote effect; ambiguous outcomes retain existing reconciliation.
4. HTTP transport is private behind `RemoteAction`; new step and direct-tool
   ingress use the same `ActionService`, including its durable model-call mapping.
   Registered remote tools route only to a trusted registered adapter/operation;
   arbitrary supplied provider metadata cannot register a tool or claim safe replay.
   Existing legacy harness paths remain at their original06/08 replacement gates.
5. Requests use fixed origin + absolute path only; reject alternate authorities,
   schemes, fragments, userinfo, backslashes and credential/host/hop-by-hop header
   overrides. HTTPS required by default; explicitly configured operator-only exact
   IP-literal origins may permit local/private test or deployment destinations.
   Resolve once per operation, reject any prohibited answer, pin vetted addresses
   into a fresh proxy-free HTTP client, disable redirects and automatic retries.
   Preserve TLS hostname validation. No pooled client can reuse stale DNS authority.
6. Bound DNS/connect/total/read duration, input headers/body, response bytes and
   retained envelope. No raw request/response/error logging. Retain only safe response
   headers and redact active credentials from bounded body content before receipts
   or context. Redirect/error/timeout/oversize failures never prove non-application.
   Existing supervisor owns cancellation by dropping the actual provider future.
7. Replace `authority::supported_publication`'s blanket tools denial with bounded
   database-authoritative validation only for registered HTTP-owned operation
   snapshots. Publication validates the company's live registration and immutable
   schema/capability/recovery facts; no caller-supplied snapshot becomes a grant.
   Resource-slot alias substitution may change only `contract.connection`, after
   verifying a declared HTTP slot; all other contract/policy fields must match.
   Binding/admission additionally check the exact bound connection's current
   registration, rather than accepting another connection's matching tool name.
   Dispatch repeats that exact resource+operation check under the existing locks.
   Traverse child bundles with bounded existing publication depth/count limits;
   each child's company and registrations must pass. Agents, skills, profiles,
   MCP and other unimplemented resource/tool owners remain denied. Existing
   publication idempotency replay remains its established immutable-result contract;
   current use authorization remains mandatory on binding/admission/dispatch.
   Exercise production publish → bind → admit → dispatch and negative forged,
   changed, foreign, revoked, unsupported and ordinary-member paths without
   fixture-only bundle replacement.

Failure matrix: foreign tenant/actor, missing or revoked connection/credential/
policy, changed contract, protected action, forged URL/header, unsafe DNS or rebinding,
redirect, timeout, oversized/chunked body, secret echo, competing dispatchers,
restart with receipt and unknown write. All require fail-closed behavior or retained
effect truth as appropriate; no new queue or lease owner is introduced.

### Verification and resources

Focused checks: HTTP domain/transport hostile fixtures; HTTP resource/credential
roundtrip and revocation/concurrent claimant DB tests; compiler/readiness and action
integration filters; credential rotation tests. Final full gate: formatting,
offline all-target compilation, additive fresh migration, SQLx prepare/check,
strict all-target Clippy, full database-backed library suite at stock stack via
`scripts/stack-budget.sh`, applicable workflow lease-budget gate, and `graft build`.
Exact commands/results and logs recorded when run; unavailable checks remain unverified.

Only task-created disposable PostgreSQL resources may be mutated. Historical
04.8 container `workflow-foundation-048-20261013` is stopped/retained; preserve it.
Fixture identity/ports to be recorded before creation. Serialize fixture writes/tests.

Freeze excludes only these workflow records (not the whole docs directory, which
contains retained deliverables): `plan/workflow-foundation/docs/TASK.md`,
`plan/workflow-foundation/docs/REVIEW.md`,
`plan/workflow-foundation/docs/agent-review-notes.md`,
`plan/workflow-foundation/docs/agent-review-findings.md`,
`plan/workflow-foundation/docs/logs/04.9`.
Snapshot digest pending implementation. No project edits while B reviews a freeze.

Fixture created with root authorization: `workflow-foundation-049-20261017`,
container `257e9aab04959a3c3e34a2f55cc16cd509df1899488a523f9f3781fa04e21768`,
`127.0.0.1:55449`, database `workflow049_test`, PostgreSQL system identifier
`7697618204589658155`, image `postgres:18.6-bookworm`. New task-only disposable
password `workflow049`. `newgrp docker` is required for socket access. Baseline
`DATABASE_URL=postgres://postgres:workflow049@127.0.0.1:55449/workflow049_test cargo sqlx migrate run`
PASS (`logs/04.9/baseline-migrations.log`); pg_stat_statements configured.

Status: root accepted architecture04.9-D2 on reviewer04.9-R002; findings
04.9-F001..003 resolved at decision level. Author `/root/author` owns implementation;
reviewer `/root/reviewer` remains independent. Graft caller evidence is retained in
`docs/logs/04.9/wf049-callers-*.txt`.

### 04.9 implementation checkpoint (in progress)

Added application HTTP models/ports/runtime (`src/application/workflow/http*.rs`),
guarded provider and production composition (`src/adapters/workflow_http/`),
additive connection/credential/operation migration, HTTP persistence authority,
authoring directory and runtime request builder (`workflow/http*.rs`), credential
rotation coverage, scoped publication/binding/admission support. Both new tool
and step entrances use ActionService. HTTP transport reuses existing MCP guarded
client policy with fresh vetted DNS pinning and rechecks secrets/policy after DNS.
No existing legacy harness or production worker startup was replaced (06/08).

Checks so far: baseline offline all-target check PASS (`baseline-check.log`);
baseline migration PASS; HTTP additive migration PASS (`http-migration.log`);
`SQLX_OFFLINE=true cargo check --locked --offline --lib` PASS (`check-lib2.log`).
Earlier all-target attempts were stopped after initial import errors to avoid
compiling stale test source; `check-lib.log` records wrong WorkflowStepKind imports,
corrected to polling before successful library check. Current focused check:
`TEST_DATABASE_URL=postgres://postgres:workflow049@127.0.0.1:55449/workflow049_test SQLX_OFFLINE=true cargo test --locked --offline --lib workflow_http_`
(`http-tests-first.log`, running at checkpoint). Nothing is implementation-accepted.

Remaining: finish focused tests and source audit (hostile DNS, real publication/
binding/admission negatives and foreign tenant/member, management races/rotation),
required SQLx preparation and full final gates, delivery documentation/RESUME,
graph refresh, frozen digest, independent whole-batch review and root acceptance.
No current substantive artifact digest. Preserve all pre-existing work.

User steering: root takes authorship at author's55% context; authoritative occupancy
is unavailable. User subsequently chose to keep this author until its batch is ready.
At complete frozen batch checkpoint A stops project writes and root takes authorship
for review corrections; B remains independent. Root's model is still not exposed.

Checkpoint update: `http-tests-second.log` PASS 11/11 focused tests. Expanded
HTTP suite (foreign/member/forged publication, read deadline, unknown write and
refined typed request/seam checks) is compiling with source held stable; current
log `http-tests-current.log`. Formatting, diff whitespace, transport boundary
and boundary regression gates PASS. `graft build` PASS (`graft-build.log`).
Local network-isolation wrapper is explicitly unavailable: its Linux implementation
mutates the shared user host OUTPUT firewall and assumes PostgreSQL port5432,
whereas the authorized disposable fixture uses55449. Root agreed to preserve
shared host firewall state; existing CI wrapper coverage remains a review item.
No full-verification claim is made for that wrapper.

### 04.9 USER STOP — authoritative current checkpoint, 2026-10-17

User requested stop at next secure checkpoint. Author complied without beginning
any new tests or adding the newly identified missing HTTP credential-rotation row
test. Current expanded focused compilation was safely terminated (cargo354655,
rustc354694, shell status143; both confirmed absent). Its source is unverified.
Earlier `http-tests-second.log`11PASS does not verify subsequent additions.
Prepared `final-gates.sh` has NOT been executed. Required full gates remain pending.

Task-only PostgreSQL container `workflow-foundation-049-20261017` is stopped with
data retained; Docker `exited`, volume
`c5fed217cc0982b517310205a52114f8da66989402f78477f9bf8c9fcf6ce9fc`.
No author-owned build/test/graph processes remain. No unrelated resource was changed.
Architecture D2/R002 remains accepted only at decision level. Implementation has
not undergone B code review or root acceptance. No authorship takeover occurs now.
New instruction is required before resuming work.

Full handoff: `plan/workflow-foundation/HANDOFF-04.9-USER-STOP-20261017.md`;
newest RESUME entry is04.9 USER STOP, prior04.8 acceptance retained.
Snapshot script ran after handoff/RESUME were written, using the five exact workflow
record exclusions listed above; preserved AGENTS/skills and all deliverables are
included. Base `2729f2caaf58830bea8291bdcc6f9882a3ad03ee`; digest
`e64f7016555ec87c656b2198e309ecb707ccc47e1915f6c093667500d3154cd9`;
manifest `docs/logs/04.9/user-stop-snapshot.txt`.
This is a stop checkpoint, not a code-review-ready freeze.
Retained graft tool estimate tally: approximately4,297,415 tokens saved.

### 04.9 resumed — 2026-10-17

User authorized continuation of RESUME under implement-astra-reviewed-2.1 and
confirmed root is Astra/high. Root now owns authorship; a fresh independent
Astra/medium reviewer will review the whole frozen batch. Historical worker handles
are inactive. Context occupancy and compaction controls are unavailable; use host
defaults with durable records. Starting HEAD remains
`2729f2caaf58830bea8291bdcc6f9882a3ad03ee`; preserve all handoff WIP and unrelated
AGENTS/skill files. Scope remains 04.9 only; D2/R002 gate remains accepted.

Verified retained fixture container ID matches handoff and restarted it. Root owns
serialized fixture checks. First check: expanded `workflow_http_` tests, log
`logs/04.9/resumed-http-tests.log`. Next add populated HTTP rotation/inventory/CAS
coverage, run related checks, then the recorded final full gate and independent
whole-batch review. No commits, deployment, migration squash or host firewall writes.

Coverage additions: `credential_rotation_http_tests.rs` verifies populated inventory,
old-key rewrap, tenant/connection/type binding, idempotence and stale ciphertext CAS.
`workflow/http_authority_tests.rs` exercises exact selected-connection contract
validation during binding and revoked bound operation during admission with another
matching live registration. Existing HTTP tests cover actual publish/bind/admit/
dispatch, competing dispatchers/revisions, receipt replay/direct tool mapping,
unknown write, foreign credential FK/member authority, forged policy, revocation,
protection, origin/header injection, DNS pinning, redirect, timeout, bounded output,
secret redaction and cancellation. These complement existing shared authority tests.

Final serial gate command is `bash docs/logs/04.9/resumed-gates.sh` (path relative
to this plan); exact commands are retained there and outcomes append to
`logs/04.9/resumed-gate-status.log`. It starts only after the initial resumed
HTTP check exits, then rebuilds for the additions. It includes HTTP/related focused
checks, migrations, SQLx preparation/check, offline all-target compilation, strict
Clippy, full all-target tests, stock-stack and lease-budget gates, fmt/diff and graph.
Static transport and its regression script PASS in `resumed-transport-boundary*.log`.
The local host-firewall network wrapper remains unavailable as already recorded;
no full network-isolation claim will be made. No resource limit was raised.

Initial resumed source: focused HTTP 15/15 PASS (`resumed-http-tests.log`, build
8m37s, tests12.71s). Subsequent expanded focused build includes four added tests.
Author output audit then found JSON scalar secret echoes escaped the string-only
redactor; `redact_value` now redacts number/bool/null values containing the active
secret, with a real-transport scalar-echo regression. This final addition postdates
the expanded focused compile and must pass in the final suite; no earlier focused
result verifies it. No gated decision changes.

Continuation verification: expanded HTTP19/19 and related MCP/credential25/25 PASS;
latest scalar redaction transport regression1/1 PASS by running the built test
binary with its exact test name (`resumed-scalar-redaction.log`). Migration,
SQLx prepare/check and offline all-target compilation PASS; `.sqlx` unchanged.
Credential rotation CLI script PASS (`resumed-credential-cli.log`). Strict Clippy
found one `collapsible_if` in `authority::supported_publication`; corrected with
the same predicate. Original failure retained in `resumed-clippy.log`. Remaining
gates resumed via `logs/04.9/final-gates-after-clippy.sh`, with corrected Clippy and
a final offline check in addition to the outstanding full/budget/static/graph gates.

The test-target Clippy pass then identified two `let_and_return` fixture bindings;
both corrected (`resumed-clippy-corrected.log` retains failures). Strict Clippy PASS
in `resumed-clippy-final.log`. Named `ActionCounts` replaced three-field count tuples
in two HTTP tests per source instructions; the shared runtime query now names its
columns. Final metadata/check/Clippy commands are retained in
`logs/04.9/final-metadata-checks.sh` and must run after the current full/budget gates
to verify the final test helper source. Full-suite command is running through
`logs/04.9/final-gates-after-test-clippy.sh`; no implementation freeze or acceptance
yet. Provisional pre-review digest is historical once source changes; only a new
post-verification digest can be submitted to B.
