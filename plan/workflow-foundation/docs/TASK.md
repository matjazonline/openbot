# 04.8 continuation under bb-implement-astra-opus — task record

## 04.9 crash recovery — 2026-10-17 current status

**04.9 VERIFIED AND ROOT-ACCEPTED — 2026-10-17.** R003 whole-batch code review
and R004 final documentation/evidence review PASS; F001–F003 fixed, no new findings
or required verification gaps. Root independently recomputed accepted digest
`1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b` at base
`e634a82837d50e7ed4aba4bb003f0a98ec294b10`, same five exact exclusions
(`logs/04.9/crash-accepted-snapshot.txt`). All gate commands/results are retained.
Task PG fixture stopped cleanly after checks; data retained at
`/private/tmp/workflow049-crash-20261017`. No author-owned checks remain running.
PostgreSQL18.6 remains installed; original external-volume database path preserved.
Other sessions' edits preserved. No source correction was needed in this recovery.
No commit, deployment, migration squash or later phase performed. Next phase
requires a new instruction. Earlier in-progress entries below are historical.

Original requirements: phase04 HTTP/registered-tool section, BRIEF-04.9 and
accepted architecture D2/R002. Scope remains 04.9 only. User confirms root is
Astra/high and author; independent Astra/medium B will be spawned at freeze.
Host context telemetry/compaction controls remain unavailable; use durable records.
Starting HEAD `e634a82837d50e7ed4aba4bb003f0a98ec294b10` contains all prior 04.9 WIP.
Preserve unrelated `.claude/helpers/graft-hooks.cjs` and `graft-statusline.cjs`
changes. Existing AGENTS/skill changes in HEAD are pre-existing, outside this batch.

Old Linux fixture and detailed ignored logs are unavailable. User requested latest
PostgreSQL18; refreshed Homebrew installed18.6 (formula18.6_1). Default data symlink
to missing `/Volumes/ssd1/postgresql/data18` preserved. Isolated PG18.6 fixture
`/private/tmp/workflow049-crash-20261017`, localhost55449, role `workflow049`,
database `workflow049_test`, system identifier `7697645681053396076`; root owns it.
No application database, host firewall, migration history or deployment changed.

Current checks: offline all-target compilation PASS (`logs/04.9/crash-offline.log`);
graph rebuilt. Focused HTTP and credential checks, fresh migration/schema, SQLx
prepare/check, offline compilation, strict Clippy, formatting, transport regression,
credential CLI, full all-target suite with macOS network isolation, stock-stack and
lease-budget gates are serialized in `logs/04.9/crash-gates.sh`; outcomes in
`crash-gate-status.log`. No limit raised. Then freeze using the five exact existing
exclusions, independent whole-batch code/integration review, corrections and acceptance.
No implementation acceptance yet. No new architecture decision proposed.

Recovery update: fresh migrations/schema and focused HTTP20/20 PASS. During this
run an unrelated session changed AGENTS.md, persistence/AGENTS.md, plan README
guidance and `src/main.rs` (startup warning); preserve these concurrent edits.
Observed path list: `logs/04.9/crash-concurrent-paths.txt`. New guidance removes
backward-compatibility work but does not remove constraints, fail-closed decoding,
lease fencing or DB tests; this continuation adds no compatibility rollout.
Snapshot helper `logs/04.9/crash-snapshot.sh` calls the unchanged skill snapshot
script with its five exact exclusions, providing a Python realpath/relpath wrapper
because macOS BSD realpath lacks the script's GNU flags. No skill files changed.

Review freeze: base `e634a82837d50e7ed4aba4bb003f0a98ec294b10`, digest
`7b68a7fcdb1c96d8a1e683feae8bc0008b945573c6f846f72cd30b39aa31ab1a`, exact
five exclusions unchanged. Manifest `logs/04.9/crash-review-snapshot.txt`.
Review the complete implementation delta from `2729f2caaf58830bea8291bdcc6f9882a3ad03ee`
to current source, including code already committed in the crash checkpoint;
`crash-review-source-files.txt` identifies its source/migration files. Concurrent
guidance/main warning edits are preserved outside 04.9 scope and covered by digest.
Author audit traced the HTTP/action entrances, publication/binding/admission checks,
provider revocation/secret reads, shared guarded client and credential inventory/CAS.
No new design change. Focused HTTP20/20 and credential12/12, fresh migration/schema,
SQLx prepare/check (cache unchanged), offline all-target check, strict Clippy,
fmt/diff, transport/regression and credential CLI PASS. Full all-target suite is
running under verified macOS egress isolation; stock-stack/lease checks follow.
Code is frozen for B; root alone owns the running fixture checks. Independent
review may inspect now; acceptance requires all outstanding check results.

B handle `/root/reviewer049`, requested `gpt-6-astra`/medium with independent
context, owns review notes/findings/REVIEW. Root remains author and owns fixture
checks. No project writes during this review; review records/logs alone may change.

R003 code/integration PASS received and persisted; no new findings. After B
finished, root corrected delivery status in RESUME and BRIEF-04.9 to link current
acceptance records. No source/migration change. Final artifact base unchanged,
digest `1ad9356a5a29195853d8a5be8e53e4e384465e3365db0ed456ea35f35c76f08b`,
manifest `logs/04.9/crash-final-snapshot.txt`, same five exclusions. Final B
follow-up needs only this documentation delta and outstanding gate evidence.

Full all-target gate PASS under macOS egress isolation: library2579PASS/0FAIL/
22existingignored (555.28s), binary5PASS/0FAIL (`crash-full-tests.log`). Stock
2048KiB full library suite started14:54:55Z; lease and final graph follow.

Stock-stack gate PASS15:04:31Z:2579PASS/0FAIL/22existingignored at2048KiB,
573.85s (`crash-stack.log`). Lease gate now running; no stack/lease limits raised.

All recovery gates finished PASS15:06:54Z (`crash-gate-status.log` and exact
commands in `crash-gates.sh`). Lease89/89PASS, highest measured work35.5% versus
50% limit, all required operations observed (`crash-lease.log`, `crash-lease.jsonl`).
Final graph rebuild PASS. Root rechecked final digest `1ad9356a…` unchanged and
diff whitespace PASS. Gate process exited0. No unavailable required gate: macOS
network-isolation wrapper ran successfully, replacing the historical Linux gap.
22 pre-existing ignored tests remain excluded as in the baseline/CI. B final
documentation-delta/evidence review pending; root acceptance not yet declared.

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

## 04.10 + 04.11 — 2026-10-17 — implement-opus-reviewed-2.1

### Scope and authorization

User instruction 2026-10-17: continue RESUME under implement-opus-reviewed-2.1 and
"Start 04.10 and 04.11". Not authorized: commits, deployment, migration squash,
production data, host firewall mutation, phases 05+ (05 human waits, 06 Rig/startup,
08 bypass removal, 09 UI remain out of scope; record seams only).

Original requirements: [Phase 04](../04-actions-http-and-delivery.md) sections
"MCP tool calls over HTTP", "Provider-neutral messaging" and "Acceptance";
[EXPANSION](../EXPANSION.md) rows 04.10 and 04.11 (04.11 includes the combined
phase04 gate); prior briefs BRIEF-04.1…04.9 for established contracts and the
04.10/04.11 obligations they defer (e.g. BRIEF-04.5 MCP `isError` classification).
Repository rules: `AGENTS.md` and applicable `src/**/AGENTS.md`.

Acceptance: each batch meets its phase-04 section and acceptance bullets with
focused tests (competing-claimant tests for changed concurrency), independent B
PASS on the frozen digest, and required checks recorded. 04.11 additionally
requires the full combined phase04 gate (migrations, SQLx prepare/check, offline
all-target, strict Clippy, fmt/diff, transport scripts, full all-target suite under
network guard, stock-stack, lease budget, graft build).

### Baseline and preserved state

Starting HEAD `1b3b82acfe8f53bb7c0d08cc138f5271704970af` (04.9 accepted/committed).
Preserve untracked `.claude/skills/implement-opus-reviewed-2.1/` (not task content).
Root added `env.CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` to globally ignored
`.claude/settings.local.json` at user request; it applies only to sessions started
afterwards, so this run uses default auto-compaction (no 75% compaction claim).

### Batches

1. **04.10 MCP** — `mcp.call` step, company MCP connections, direct agent MCP tools
   through ActionService, frozen contract vs discovery, JSON/SSE, inert resource
   links, isError classification, revocation/secrets, cancellation/session cleanup,
   MCP race/recovery matrix. Architecture gate expected (schema/credential/ownership).
2. **04.11 Messaging + combined phase04 gate** — starts only after 04.10 acceptance.

### Workers

Root: Opus 5.5, session effort `high` (not xhigh) → thin coordinator, fills no role.

| Role | Handle | Model/effort | Assignment |
| --- | --- | --- | --- |
| A | agent aaae928273e0006cd | opus / xhigh (requested; runtime-reported) | 04.10 author |
| B | agent adde0801387df89a3 | opus / xhigh (requested) | independent review (04.10-D1 gate first) |

### Resources

A owns builds, tests and a new task-only PostgreSQL fixture it creates and records
here (identity, port, data dir). The stopped 04.9 fixture
`/private/tmp/workflow049-crash-20261017` is not reused or deleted. B is read-only.

### Status / next action

- 2026-10-17: records established; spawning A for 04.10 investigation/gate.

### 04.10 author checkpoint — 2026-10-17 (A, implement-opus-reviewed-2.1)

Author A (opus / xhigh as requested by root) started 04.10. Actual HEAD at start is
`8ce1fd49b5a961da705fddf0b92cac3182da427b`; it differs from the recorded starting HEAD
`1b3b82acfe8f53bb7c0d08cc138f5271704970af` only by committing the
`.claude/skills/implement-opus-reviewed-2.1/` skill files (no source change). Preserve them.

Fixture (task-only, A-created): PostgreSQL 18.6 data dir `/private/tmp/workflow0410-20261017`,
listen `127.0.0.1:55450` (socket dir = data dir), superuser role `workflow0410` (trust,
localhost only), database `workflow0410_test`, system identifier `7697669855432439122`,
`max_connections=200`, `pg_stat_statements` preloaded. Baseline `cargo sqlx migrate run`
PASS, 54 migrations (`logs/04.10/baseline-migrations.log`). The stopped 04.9 fixture
`/private/tmp/workflow049-crash-20261017` is untouched. Gate script will be
`logs/04.10/gates.sh` with outcomes in `logs/04.10/gate-status.log`.

Investigation: existing MCP catalog/credentials/grants, guarded rmcp adapter (bounded JSON/SSE,
pinned DNS, no reinit), legacy harness MCP journal, 04.9 HTTP owner/runtime/authority,
shared remote dispatch (`supervise_remote`, reservation, receipts) and the registry's
`mcp.call` envelope contract. The batch changes schema, credential/resource ownership,
authorization and a cross-subsystem result contract, so it is gated by 04.10-D1 below
(exact copy in `logs/04.10/decision-D1.md`). No gated implementation before B's PASS.

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

Status: 04.10-D1 written and frozen for the architecture gate; A stops and reports the
decision path and SHA256 to root. Next: implement the whole batch after root relays B PASS.
- 2026-10-17: A delivered gate 04.10-D1 (`logs/04.10/decision-D1.md`, SHA256 `3f105530…035e`); HEAD now `8ce1fd4` (user committed skill files only). Fixture created by A: `/private/tmp/workflow0410-20261017`, 127.0.0.1:55450, role `workflow0410`, db `workflow0410_test`, sysid `7697669855432439122`. B spawned for D1 architecture gate; A paused awaiting PASS.
- 2026-10-17: B review 04.10-R001 on D1: CHANGES REQUIRED — actionable F001 (medium), F002 (low), F003 (medium); optional F004–F007. Relayed to A for one-round correction as 04.10-D2 (`logs/04.10/decision-D2.md`); recheck by same B by finding ID.

### 04.10-R001 response — D2 (A, 2026-10-17)

B round 04.10-R001 returned CHANGES REQUIRED on D1 (F001–F003 actionable, F004–F007
optional). D1 file is kept unchanged. Revised decision 04.10-D2 below; exact copy in
`logs/04.10/decision-D2.md`. Still no gated implementation.

Per-finding resolution:
- 04.10-F001 (medium) — RESOLVED IN D2 item 10: outcome table for every step/direct-tool
  result (eager refusal, eager transport, drift, reservation refusal before marker,
  fence/infrastructure/hint errors, ApprovalRequired interim until 05, PossibleDispatchExists,
  committed success/isError, post-entry failure, Interrupted, crash/lease before/after marker)
  with code, class, handler safety, effective decision and direct-tool result. Pre-marker
  refusals are clean Terminal/Safe failures (FinalError), never waits. Crash/lease loss
  before a marker no longer parks: shared classifier `marker_owned_effect_kind`
  (`mcp.call|http.request|tool.call`) gives `recovery::report` and
  `pending_recovery::unknown_effect` a Safe no-marker default (explicit contract change; the
  predicate still forces Reconcile whenever an unresolved marker exists). Tests assert run,
  attempt, marker, receipt and reconciliation-fact state, not only network counts.
- 04.10-F002 (low) — RESOLVED IN D2 item 10 rows 9 and I8: MCP handlers always supply
  SafeToRetry; the predicate is the only Unknown source. `mcp.tool_error` takes FinalError
  (final_error route or failed run) with effective `safe`, no reconciliation fact; operator
  retry returns the same receipt with zero MCP requests; tool errors never prove not-applied.
- 04.10-F003 (medium) — RESOLVED IN D2 verification plan: the eight missing matrix cases
  (lazy post-marker after 04.7 not-applied, stale workers in eager open and during
  `tools/call`, workflow-level session expiry, pre-marker unavailable retry, direct-tool
  parity, argument change, competing catalog writers, cross-tenant FK) with expected state;
  batch gate adds the affected caller suites (binding/admission/publication/resources,
  action dispatch/receipt/reconciliation, recovery/pending/uncertainty, HTTP, credential
  rotation, persistence MCP, legacy MCP runtime/rig harness, MCP routes) and a scoped stock
  2 MiB run `scripts/stack-budget.sh --offline -- workflow_action workflow_mcp mcp`. Full
  all-target/stack/lease stay at the 04.11 combined gate.
- 04.10-F004 (optional) — DECLINED: a declared-vs-absent output flag would change the shared
  `ToolContract`/registry facts for marginal benefit; the registry already types
  structuredContent as optional, so workflows supply defaults. Recorded as an accepted
  limitation in D2.
- 04.10-F005 (optional) — ADOPTED: I3 restated ("without `tools/call`, and without any MCP
  request when the hint is current"); sequential restart test keeps asserting zero requests.
- 04.10-F006 (optional) — ADOPTED: bounded ≤ 5 s permit wait (global and per-endpoint,
  immediate failure after shutdown) before `mcp.unavailable`; covered by tests; no limit raised.
- 04.10-F007 (optional) — ADOPTED: `workflow_mcp_operations.revision` with operation-level
  CAS on register (None = create-only) and revoke, while still not bumping the catalog
  revision used by legacy runs.

Follow-up recorded for root (out of 04.10 scope): the accepted 04.9 HTTP handler still labels
every non-committed outcome Terminal/Unknown (`http.unresolved`), so pre-marker HTTP
revocation parks without a marker; aligning it with the D2 rule is a separate change.

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

Status: 04.10-D2 frozen for B recheck of F001–F003 (and adopted F005–F007); A stops and
reports the D2 path and SHA256 to root. Fixture unchanged and running.
- 2026-10-17: A delivered 04.10-D2 (`logs/04.10/decision-D2.md`, SHA256 `84d532ee…0ded5`): F001–F003 addressed, F004 declined, F005–F007 adopted. A flagged 04.9 HTTP pre-marker `http.unresolved` parking as follow-up; B asked to judge whether it belongs in this batch. B rechecking as 04.10-R002; A paused.
- 2026-10-17: B 04.10-R002 on D2: CHANGES REQUIRED — F008 (medium, 04.9 HTTP pre-marker refusals park unreconcilably), F009/F010 (low, from adopted F006/F007); F011 optional. Shared recovery-default change judged sound. Root decision: HTTP handler alignment (F008) is in 04.10 scope. A writing 04.10-D3; recheck limited to F008–F010.

### 04.10-R002 response — D3 (A, 2026-10-17)

B round 04.10-R002: CHANGES REQUIRED on D2 (F001–F003, F005 resolved; F004 decline
accepted; classifier judged sound; new F008 medium, F009/F010 low, F011 optional). Root put
the 04.9 HTTP handler alignment in 04.10 scope. D1 and D2 files are unchanged. D3 amends
D2; exact copy below and in `logs/04.10/decision-D3.md`. Still no gated implementation.

Per-finding resolution:
- 04.10-F008 (medium) — RESOLVED IN D3 C/D: one shared `action_step_failure(family, outcome)`
  mapping (always Safe; predicate is the sole Unknown source) used by MCP and by
  `HttpActions::execute`. HTTP pre-marker refusals → `http.rejected` Terminal, protection →
  `http.approval_required` Terminal, dispatch errors/PossibleDispatchExists/Interrupted →
  Retryable with the predicate deciding. New invariant I9 (no reconciliation wait without an
  unresolved marker). DB tests assert run/attempt/marker/receipt/fact state for HTTP
  revocation, protection, crash before marker and post-entry unknown. D2 row 4 (MCP
  reservation refusal) aligned to the same rule (`mcp.unresolved` Retryable, next
  preparation rejects).
- 04.10-F009 (low) — RESOLVED IN D3 B: endpoint permit first, then global, one shared ≤ 5 s
  deadline; waiters on a busy endpoint hold no global permit; shutdown fails waiters
  immediately; adapter starvation test with a saturated endpoint and a second endpoint.
- 04.10-F010 (low) — RESOLVED IN D3 A: per-write random UUID `version` (never reissued
  across revoke, grant cascade or soft delete) for register/revoke CAS; competing stale-writer
  tests after revoke/re-register and cascade/re-register, plus one-winner concurrent register.
- 04.10-F011 (optional) — ADOPTED (documentation + obligation): invariant stated on the
  classifier and `WorkflowHandler`, and recorded in BRIEF-04.10 as a 06/08 obligation;
  runtime assertion declined because there is no handler registry to assert against.

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

Status: 04.10-D3 frozen for B recheck of F008–F010 and the HTTP alignment; A stops and
reports the D3 path and SHA256 to root. Fixture unchanged and running.
- 2026-10-17: A delivered 04.10-D3 amendment (`logs/04.10/decision-D3.md`, SHA256 `f89b0a89…7b32b`): F008 HTTP aligned via shared `action_step_failure`, F009 endpoint-first permits, F010 UUID version, F011 adopted as documentation. B rechecking as 04.10-R003.
- 2026-10-17: B 04.10-R003: **PASS architecture gate** for D3 `f89b0a89…7b32b`; effective gated decision = D2 as amended by D3. Root accepts the gate (decision level only). F012 (low, actionable) carried into implementation — must be fixed+tested before batch acceptance; F013 optional. A proceeding to implement whole 04.10 batch.

### 04.10 author USER STOP checkpoint — 2026-10-17 (A)

Gate: B PASS 04.10-R003 on D3 `f89b0a89…7b32b` (effective decision = D2 as amended by
D3); root accepted at decision level. Implementation was in progress when the user
requested a stop. **This is not a review-ready artifact; no freeze/digest was taken.**

Changed source (relative to HEAD `8ce1fd49b5a9…`):
- New: `migrations/20261017110000_workflow_mcp_operations.sql` (endpoint-immutability
  trigger; `workflow_mcp_operations` with grant-FK cascade and UUID `version`);
  `src/application/workflow/{mcp.rs,mcp_runtime.rs}` (McpOperation/ports/McpActions
  handler + direct `call_tool`, eager/lazy provider); `src/application/workflow/actions/
  {effect.rs,failure.rs}` (effect-state hint port; shared `action_step_failure`,
  `marker_owned_effect_kind`); `src/adapters/persistence/workflow/{mcp.rs,
  mcp_runtime.rs,mcp_publication.rs}` (status/policy/registered_on, McpActionAuthority,
  McpOperations CAS, McpActionUse, ActionEffectStates, preparation, publication
  directory); `src/adapters/workflow_mcp/mod.rs` (McpRuntime composition);
  `src/adapters/redaction.rs` (shared redaction); tests
  `src/adapters/persistence/workflow/{mcp_tests.rs,mcp_test_support.rs,
  action_outcome_support.rs}`.
- Modified: domain `entities/mcp.rs` (discovered `output_schema`); application
  `services/mcp_client.rs` (typed `McpCallResult`), `services/mcp_runtime.rs` (legacy
  adaptation), `use_cases/mcp.rs` (endpoint fixed, output-schema validation, stricter
  refresh), `workflow/{mod.rs,worker.rs,http_runtime.rs}`, `workflow/actions/{mod.rs,
  dispatch.rs}` (MCP envelope result schema), `workflow/registry/{mod.rs,mcp.rs,
  actions.rs}` (shared `tool_result` envelope); adapters `mcp/{mod.rs,worker.rs,tests.rs}`
  (output schema discovery, typed/redacted call result, endpoint-first bounded permits
  with shutdown), `mod.rs`, `workflow_http/mod.rs` (shared redaction, `NoLocal`
  pub(crate)), persistence `mcp.rs` (endpoint rejection, set-difference grants, soft
  delete removes approvals), `mcp_tests.rs`, workflow `{authority.rs,resources.rs,
  http.rs,http_runtime.rs,recovery.rs,pending_recovery.rs,mod.rs,tests.rs,
  binding_tests.rs,http_tests.rs}`.

Implemented (code written, NOT verified by tests): D2 items 1–10 and D3 A–D (UUID
operation CAS, endpoint-first permits, shared step-failure mapping for MCP and HTTP,
HTTP handler alignment), I9, F011 documentation on the classifier and `WorkflowHandler`,
F012 (HTTP preparation `ready_on`; MCP preparation `current_on` for eager and lazy
paths), F013a (accepted assertion at `http_tests.rs` remote-failure test replaced by the
effective retirement state: `http.unresolved`, attempt `unknown`, reconciliation wait,
one fact). F013b (I9 wording "…or an evidence conflict") adopted in intent but not yet
written into BRIEF/records.

Not done:
- Adapter-level new tests in `src/adapters/mcp/tests.rs` (outputSchema discovery,
  redaction of call results, permit starvation with a saturated endpoint and shutdown
  failing waiters) — not written.
- Lazy post-marker path test: DB-level "04.7 proven not-applied → one new call" is not
  reachable for MCP because no MCP evidence verifier is registered (04.7 not-applied
  requires a registered verifier attestation); planned substitute is an application-level
  test of `McpCall::invoke` with fakes (connect/discover/call once; drift → no call) —
  not written. Must be disclosed to B as a deviation from D2 matrix item 1.
- UI edit form still offers the endpoint field (server rejects changes); optional polish.
- `BRIEF-04.10.md`, its entry in `NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs`), the
  06/08 obligation for F011, F013b wording — not written.
- `logs/04.10/gates.sh` and status log — not written; SQLx prepare/check not run.

Checks run (logs in `logs/04.10/`):
- `cargo sqlx migrate run` for the new migration on the task fixture: PASS
  (`migrate-mcp.log`).
- `SQLX_OFFLINE=true cargo check --locked --offline --lib`: PASS with 2 unused-import
  warnings, since fixed (`check-lib-1.log`).
- `cargo check --all-targets`: first run FAIL on test literals (`check-all-1.log`, fixed);
  second run FAIL with 3 test compile errors (`check-all-2.log`): one shadowed `setup`
  binding in `http_tests.rs` and two temporaries in `tokio::join!` in `mcp_tests.rs`.
  Both were edited at the stop point but **the fix is unverified (no re-check run)**.
- No tests have been executed. Everything above is unverified.

Exact next steps on resume:
1. Restart fixture (`pg_ctl -D /private/tmp/workflow0410-20261017 -l
   /private/tmp/workflow0410-20261017/server.log start`).
2. `SQLX_OFFLINE=true CARGO_BUILD_JOBS=2 cargo check --locked --offline --all-targets`;
   fix remaining compile errors.
3. Run focused tests: `workflow_mcp_`, `workflow_http_`, `workflow_action_failure`,
   `workflow_action_marker_owned`, adapter `mcp_`, persistence `mcp` catalog tests
   (`TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test`);
   check the assumed default step attempt budget (retry rows expect `running`).
4. Write the missing adapter tests and the lazy-path application test; BRIEF-04.10 +
   `NO_BACKWARD_COMPATIBILITY_NOTES`; F013b wording.
5. Write `logs/04.10/gates.sh` (D2 batch gate incl. scoped stock 2 MiB run), run it,
   `cargo sqlx prepare`/`--check`, clippy, fmt, diff --check, transport scripts,
   `graft build`.
6. Audit the diff, freeze with snapshot.sh, record base+digest, report to root.

Fixture stopped cleanly with `pg_ctl -m fast stop` (log `logs/04.10/pg-stop.log`;
`pg_ctl status`: no server running); data retained at `/private/tmp/workflow0410-20261017`
(port 55450, system identifier `7697669855432439122`). No author-owned cargo/rustc/
postgres processes remain. The 04.9 fixture was never touched.

### 04.10 USER STOP — root checkpoint, 2026-10-17

User requested handoff and stop. Author stopped at checkpoint (entry above); fixture stopped
cleanly (pg_ctl status: no server), no author processes remain; reviewer idle, no review in
progress. Implementation unverified and not review-ready. Checkpoint identity only: base
`8ce1fd49b5a961da705fddf0b92cac3182da427b`, digest
`6d5b89b7a612505bf209e74b18a3483a5999d5920ecb905bca7f4a8979de0c30`
(`logs/04.10/user-stop-snapshot.txt`). Handoff: `../HANDOFF-04.10-USER-STOP-20261017.md`.
Next action: per handoff resume steps; a new instruction is required.


### 04.10/04.11 resumed — implement-astra-reviewed-2.1, 2026-10-17

User renewed continuation and confirmed root as Astra/high author in this session.
Root owns authorship, serialized fixture checks and acceptance; independent reviewer
will use Astra/medium with fresh context. Effective compaction threshold is unavailable;
continue with host defaults and durable records. No configuration changes.
Starting HEAD: `8ce1fd49b5a961da705fddf0b92cac3182da427b`. Preserve every existing
tracked/untracked change captured by the prior stop handoff; no unrelated edits identified.
Original requirements: `../04-actions-http-and-delivery.md`, `../EXPANSION.md`,
`../HANDOFF-04.10-USER-STOP-20261017.md`; accepted decisions D2 amended by D3.
Scope/order: finish and independently accept 04.10, then 04.11 messaging and combined
phase04 verification. No commits, deployment, migration squash or phases05+.
Records: this TASK, REVIEW.md, agent-review-notes.md, agent-review-findings.md,
logs/04.10 and later logs/04.11, all within this plan's docs directory.
Exact snapshot exclusion list: `plan/workflow-foundation/docs` (workflow records only).
Root restored task-only PostgreSQL18 fixture `/private/tmp/workflow0410-20261017`,
port55450, role workflow0410, database workflow0410_test; verified system identifier
7697669855432439122. Other fixtures unchanged. Offline all-target check PASS with
one unused test helper warning (`logs/04.10/resume-check-all.log`). Focused MCP pending.
The previous proposed lazy-path mock-only deviation is not adopted: investigate an
injected test-host evidence verifier through the real 04.7 reconciliation service,
retaining D2 matrix item1 without adding a production MCP verifier.
Final full all-target, migration/schema/SQLx/static/transport, stock-stack and lease
checks remain required at the combined04.11 gate. 04.10 uses its D2/D3 focused gates.

### 04.10 resume implementation checkpoint

New changes: adapter outputSchema/redaction/inert-link and bounded permit fairness,
timeout/shutdown tests; test-host verifier integration through the real reconciliation
service (no production verifier); competing registration versus grant removal, refresh
drop and delete. Split the >500-line MCP test module into core/recovery/catalog files.
Fixed inherited test expectations: terminal state is `succeeded`; rmcp's serialized
send queue needs the in-flight fixture POST released after local cancellation before
its cancellation notification can be observed. This preserves actual cancellation and
cleanup assertions and documents the transport limitation in BRIEF-04.10.
Author audit found two implementation defects: publication used `.is_ok()` on a
fallible authority read (now propagates errors); the effect-state hint omitted the
existing DB `not_applied` and `applied_without_result` states (now typed, both use lazy
reservation). The real evidence retry test exposed the latter; this implements D2's
"any other hint" rule without changing the gated protocol. A retry reuses the logical
marker and adds a remote entry, per `action_receipts::marker_on`.
Added BRIEF-04.10, startup note path, phase06/08 handler-composition obligation and I9's
evidence-conflict qualification. Full source diff audited; independent review pending.
Evidence retained: initial MCP17 tests15PASS/2FAIL; expanded MCP55 tests54PASS/1FAIL
(test table typo); workflow MCP19 tests18PASS/1FAIL (missing hint states); correction
checks ongoing. These are superseded failures, never acceptance evidence.
Fresh DB `workflow0410_fresh_test` on the same task server migrated successfully;
normalized pg_dump schema equals retained fixture (`schema-comparison.log`).
The reproducible per-batch gate is `logs/04.10/gates.sh`; it records status and output.

### 04.10 frozen for independent whole-batch review

Artifact identity (exact exclusion: `plan/workflow-foundation/docs`):

```text
base:   8ce1fd49b5a961da705fddf0b92cac3182da427b
digest: cb1b1dba9f6a803d7e35308917786c2863e89e73e7abddd4d2f722f2927a6210
```

No project edits until review returns. Source includes the added workflow permit-timeout
test, which was added after the focused binary compiled and is covered by the pending
stock run. `gate-focused.log`:504PASS/0FAIL, including the real not-applied lazy retry
and drift variants. Fresh migration/schema identity PASS. Stock, SQLx, offline/Clippy
and remaining static gates are RUNNING in `gates.sh`, root owns the shared fixture.
Reviewer may inspect source and logs now; no acceptance until every required gate passes
and B explicitly passes the current digest. Existing findings log whitespace was normalized
without changing any finding content. Original decisions and review history preserved.

Snapshot portability: macOS realpath lacks GNU `-m --relative-to`. Task-only
`/private/tmp/workflow0410-tools/realpath` implements exactly that path normalization
with Python `os.path.realpath`/`relpath`; use
`PATH=/private/tmp/workflow0410-tools:$PATH bash .agents/skills/implement-astra-reviewed-2.1/scripts/snapshot.sh plan/workflow-foundation/docs`.
Digest algorithm and skill files unchanged; shim is a disposable task resource.

Reviewer B handle `/root/reviewer`, requested/started gpt-6-astra medium with no
inherited history, assigned whole-batch04.10 reviewR004 of cb1b1dba…7a6210. Root
remains author; B owns review records only. Root keeps source frozen and owns gates.

04.10 focused gate completed:504PASS/0FAIL/0ignored (300.46s). Source frozen at
cb1b1dba…7a6210, stock2048KiB run still executing. Reviewer confirms the real SQL
reconciliation test satisfies D2's lazy-path case and F012 has explicit disabled/secret
retirement assertions; these are progress observations, not final PASS/acceptance.
Source was not changed during review.

Stock gate first attempt:390PASS/3FAIL, no stack overflow. Failure log preserved
`gate-stock.log`; two `lease.release commit outcome unknown` assertions and one
completion persistence-bound timeout in existing reconciliation tests. Same paths
passed in the earlier focused504. No pre-existing/flaky attribution is made. Root
rechecks the three at stock stack (`stock-failure-recheck.log`) then repeats the scoped
stock gate before remaining static checks using `resume-gates.sh`; original logs are
not overwritten. New workflow permit-timeout and lazy evidence retry tests PASS in
the failed overall stock run. Source unchanged, review digest still frozen.

### 04.10 verification investigation — R004 / F014

Second scoped stock run:391PASS/2FAIL, no stack overflow (`resume-gate-stock.log`).
Different failures: delayed remote reservation count read and immediate backend-idle
assertion in cancellation ordering. R004 code inspection PASS but overall CHANGES
REQUIRED for F014; review freeze released for corrections. Server log shows the
reservation resolution timed out behind its COMMIT, and the original COMMIT was
still running when the failed test dropped its isolated fixture. Investigate test
synchronization before changing production behavior. Static gates run separately in
`static-gates.sh`; their outcomes remain individually recorded.

04.11 product clarification: user explicitly selected **users and channels; reject
groups for now**. No group model or company-wide recipient expansion will be added.
Exploration only; no architecture decision approved or implementation started.

F014 correction candidate: `stalled_remote` now uses the existing `Gate::drain`
barrier before inspecting rows/removing abort configuration. An elapsed resolution
may return before its original COMMIT finishes, so return alone was not a durable
state barrier. `assert_drained` now observes backend idle state under the existing
10-second test hang guard rather than requiring an instantaneous pg_stat_activity
sample. Exact run NOWAIT probes and primary-attempt/negative-control assertions
remain unchanged; production bounds, retry policy and the three-void ceiling remain
unchanged. Focused stock tests include all shared drain-helper callers and negative
controls. SQLx prepare/check and offline all-target PASS. Strict Clippy found two
unnecessary explicit dereferences in MCP; corrected without behavior change.
Source is no longer frozen; next review uses a new digest and affected-delta review.

### 04.10 correction frozen for R005

Focused correction stock gate:12PASS/0FAIL, all classified branches reached first
attempt, including late-release and resolved-COMMIT negative controls
(`synchronization-recheck.log`). Corrected scoped/default-parallel stock and static
suite now running in `corrected-gates.sh` (separate logs; prior failures preserved).
Frozen base remains8ce1fd49b5a961da705fddf0b92cac3182da427b; digest
`2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c`.
Exact exclusion unchanged: `plan/workflow-foundation/docs`.
Manifest `logs/04.10/corrected-review-snapshot.txt`. Delta from R004 is only the two
test synchronization corrections and two automatic-dereference Clippy corrections
in MCP. No implementation changes during recheck; root owns all fixture checks.

Corrected gates completed2026-10-17T18:07:49Z: scoped stock2048KiB393PASS/0FAIL/
0ignored (303.74s); SQLx prepare and check, offline all-target compilation, strict
Clippy, fmt, diff, transport boundary and regression, credential CLI regression,
graft build all PASS. `corrected-gate-status.log` and `corrected-gate-*.log` retain
commands/results. Fresh migration/schema equivalence and focused504/0 remain valid;
focused correction12/0 covers the only test changes. Post-gate snapshot digest
matches2be42e3e…bc4c. Reviewer final R005/F014 disposition pending. Combined full
all-target test suite, full stock-stack and lease budget still belong to04.11 final
gate, and are not claimed as complete at this batch boundary.

### 04.10 ROOT ACCEPTED — 2026-10-17

Root independently recomputed the unchanged2be42e3e…bc4c digest after reviewer final
R005 PASS, inspected saved R005 note/REVIEW and F014 resolution, and verified required
batch results above. No unresolved04.10 blocker. **04.10 ACCEPTED** on base8ce1fd4,
full digest2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c;
exclusion onlyplan/workflow-foundation/docs. Scope proceeds to04.11, not05+.
Preserve failure logs and all review history. Root owns continuing fixture and checks;
reviewer/root/reviewer retained. No commit, deployment or migration squash performed.
The following RESUME status edit is administrative acceptance metadata after this
snapshot, not a substantive change to the accepted implementation.

### 04.11-D1 architecture gate proposed

Original acceptance and product choice unchanged. Decision artifact
`logs/04.11/decision-D1.md`, SHA256
`ebb523dfdfcf5d7b937ddf48b47161bb79b70ad9bc2d890b511f63ef58b8ad83`.
Cohesive decisions: scoped built-ins/destinations, shared action authority and atomic
message/delivery/link/wait, explicit handler suspension, direct Pending port with
phase06 owner integration, delivery receipt events via existing workflow poller,
known-failure/uncertainty separation, current permission and cancellation ordering.
Per-batch focused checks and final combined suite specified in D1 section6. Gate
required because schema, ownership, transaction and concurrency contracts change.
No04.11 implementation before independent PASS for current decision revision.
Root author remains Astra/high per user confirmation; B/root/reviewer reused with
same independent Astra/medium role/context. Context occupancy/compaction controls
remain unavailable. Root will trace shared symbols/callers before dependent edits.


### Final user-stop checkpoint — 2026-10-17

**STOPPED at user request.** See [current handoff](HANDOFF-04.11-USER-STOP-20261017.md).
04.10 R005/root acceptance remains valid; 04.11 R001 is incomplete, NO PASS,
with provisional F001–F004 retained. No04.11 implementation. Reviewer returned
stopped. Root verified fixture system identifier7697669855432439122, stopped only
`/private/tmp/workflow0410-20261017` with pg_ctl fast stop, and confirmed no server
running; data retained. No author checks remain active. Worktree/evidence preserved;
no commit or deployment. Final handoff/RESUME edits are administrative documentation;
full combined tests/stock/lease gates remain pending. Further work requires a new
continuation instruction. Stop-checkpoint verification: `git diff --check`.

### 04.11 continuation — 2026-10-17 — implement-opus-reviewed-2.1 (fresh root)

User instruction: "use /implement-opus-reviewed-2.1 and continue RESUME.md". Scope
unchanged: 04.11 messaging (users/channels, groups rejected) then combined phase04
gate. No commits, deployment, migration squash, production changes or phases 05+.
Base HEAD `8ce1fd49b5a961da705fddf0b92cac3182da427b`; preserve all worktree changes.

Root cannot confirm xhigh effort → thin coordinator, fills no role. Prior Astra
root-author/reviewer handles are stale (fresh root); fresh workers spawned per skill
recovery rules: A and B both opus / xhigh (requested). `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75`
is now present in `.claude/settings.json` env for this session.

Disk: ~3.5 GiB free on /System/Volumes/Data at resume — builds may need cleanup of
task-owned targets; A must check before compiling.

Next: B completes the 04.11-D1 architecture review (R001 was interrupted; provisional
F001–F004) with a full verdict; A then corrects all findings in one revision (D2)
for a single recheck before implementation.

| Role | Handle | Model/effort | Assignment |
| --- | --- | --- | --- |
| A | agent a4461afdf8d6050a9 | opus / xhigh (requested) | 04.11 author (orientation first) |
| B | agent a4750bc4ff449a216 | opus / xhigh (requested) | 04.11-R002 completes D1 architecture gate |

- A orientation (2026-10-17): worktree outside docs matches accepted 04.10 manifest
  `logs/04.10/post-gate-snapshot.txt` exactly; current digest `24254ecf…5b0e` differs
  from accepted `2be42e3e…bc4c` only by the administrative RESUME.md edit. D1 SHA
  matches. Fixture `/private/tmp/workflow0410-20261017` sysid 7697669855432439122,
  shut down cleanly. `target/` 17G regenerable; root approves `cargo clean -p mail_agents`
  (~14G, keeps deps/registry) before first build. Awaiting B R002.

- 2026-10-17: B 04.11-R002 complete — CHANGES REQUIRED for D1. Actionable F001, F002,
  F005 (medium), F003, F006, F007, F008 (low); F004 optional. A assigned one correction
  round → `logs/04.11/decision-D2.md`, then B recheck by finding ID. No implementation yet.

#### 04.11-D2 correction — 2026-10-17 (A, implement-opus-reviewed-2.1)

Author A wrote a single correction round for 04.11-R002:
`logs/04.11/decision-D2.md`, SHA256
`1dcd1a9e99e57cd32e15bf095490961722f185153c66945cd82d29bd4d943a44`.
- D2 is self-contained and supersedes D1 in full. D1 is unchanged at
  `ebb523df…ad83` and retained as history.
- It addresses F001, F002, F005 (medium), F003, F006, F007, F008 (low), and
  optional F004, with a per-finding disposition table in D2 §0.

Main decisions:
- **Delivery model.** One canonical message per destination, each with its own RFC
  Message-ID. `message.send` always opens a new Standalone thread. `message.reply`
  uses the source's thread and headers.
- **One effect kind.** All messaging uses a single `message` marker/receipt kind.
- **Bounds.**
  - Admission is capped at 128 message markers per execution. It is refused before
    any write with the terminal `message.action_limit`, and backed by an SQL trigger.
  - At most 20 destinations per message. Subject limited to 998 bytes.
- **Database-derived proofs.**
  - A trigger captures the fence expiry while the job is still live; the deferred
    commit guard re-checks it.
  - A receipt guard recomputes the result from authoritative delivery states.
- **Receipts and waits.**
  - Receipt events come only from a parent-status trigger; the reaper locks parents
    first.
  - The consumer handles one action per transaction, in pages of 8, with poison
    backoff to `stalled`.
  - The effect-wait deadline equals the run deadline, and `resume_on` has an explicit
    effect branch.
- **Recovery and reconciliation.**
  - Message kinds become marker-owned.
  - Outcome table across the shared predicates (D2 §5).
  - Unknown deliveries are marked `needs_reconciliation` and parked.

**Deferral for root to confirm (D2 §5.3).** D2 defers a verifier-attested evidence
path for `outcome_unknown` deliveries: no email ledger or verifier exists to feed one.
This is justified in D2 §5.3 against phase 04 items 5–6. Root may reject the deferral
and require the path inside 04.11.

**Observation (not widened here).** Remote HTTP/MCP admission past 128 markers remains
the accepted 04.9/04.10 behavior: it fails closed through the proof scans.

Records and next steps:
- No source edits, builds or fixture start in this step. REVIEW, notes and findings are
  untouched.
- The `decision-D2.md` content was reformatted on disk by tooling after it was
  written. A verified the reformatted text against the intended decisions and
  corrected three wordings before hashing; the SHA above is of the final file.
- Next: B rechecks D2 by finding ID. Implementation starts only after B's PASS on this
  SHA.

- Root (2026-10-17): D2 SHA `1dcd1a9e…3a44` sent to B for recheck R003. A's §5.3
  deferral (no verifier-backed evidence path for unknown deliveries) not accepted by
  root; B asked to judge it against phase04 items 5–6 / acceptance, escalating to the
  user only if it is a product/scope choice.

- 2026-10-17: B 04.11-R003 — CHANGES REQUIRED for D2. F001–F008 resolved at decision
  level; blocking F010 (medium, verifier-backed message reconciliation required by
  phase04 item 6) and F011 (low, lease-budget bound/leading locks); F009 optional.
  Root ruling: §5.3 deferral rejected (criteria not weakened; user has not reduced
  scope). A assigned D3.

#### 04.11-D3 correction — 2026-10-17 (A, implement-opus-reviewed-2.1)

Author A wrote a single correction round for 04.11-R003:
`logs/04.11/decision-D3.md`, SHA256
`c462e4eb56cb76ac816fa107b7b01f7651706796b8723fed8f206c6315858a89`.
- D3 is an explicit delta. The effective decision is D2 (`1dcd1a9e…3a44`) with D3
  §§A–L replacing or extending the named D2 sections; the map is in D3's header
  table. D2 and D1 are unchanged.
- It applies root's ruling: the D2 §5.3 deferral is rejected and the verifier path is
  in scope.

Dispositions:
- **F010 (medium), adopted.**
  - Message invocations use the existing 04.7 `ActionReconciliationService`, its
    authority and the `ActionEvidenceVerifier` port.
  - One subject per `outcome_unknown` destination delivery (at most 20); a new
    `ReconciliationTarget::MessageDelivery` is included in the request digest.
  - Applied → that delivery is `delivered`.
  - Final not-applied on a live run with attempts left → only that delivery becomes
    retryable, through a one-use reopen. On a terminal or cancelled run, or with a
    cancellation intent → superseded. With attempts exhausted → `dead_letter`.
  - Unknown → no change. A finality breach records a conflict and blocks further
    reopens. An expired proof stops the provider call at `begin_part`.
  - Tests use a test-host ledger verifier; no production verifier is claimed.
- **F004 premise and §5.1 rows 11–13, re-examined (D3 §D, §G).**
  - `delivered` and `dead_letter` stay final (trigger-enforced).
  - `outcome_unknown` exits only through evidence settlement, plus the expired-proof
    re-entry.
  - The event bound is restated; the consumer reads only `max(seq)`.
  - Outcome-table rows 15–21 are added.
- **F011 (low), adopted.**
  - The consumer runs under `WriteBound` `message.consume` with the workflow worker's
    `LeasePolicy`.
  - Leading locks: for consume, run / action / parents; for publish, the `authorize_on`
    locks plus destination-owner locks.
  - Delay and contention tests are marked `Injected`, use the `workflow_message_race_`
    prefix and stay outside the lease filters.
  - No threshold is raised.
- **F009 (optional, low), adopted.**
  - The admission trigger locks the fenced attempt before counting, with a concurrent
    SQL-only test.
  - The step-origin guard requires the `EffectParked` close cause.

Records and next steps:
- No source edits, builds or fixture start in this step. REVIEW, notes and findings are
  untouched.
- As with D2, tooling reformatted `decision-D3.md` on write. A reviewed the
  reformatted text, fixed four precision points (the back-to-unknown entry rule, the
  reopen proof condition, the lease clearing on proof expiry, and the attested
  acceptance time), and hashed the final file.
- Next: B rechecks F009–F011 and the F004 / §5.1 premises that F010 touches.
  Implementation starts only after B's PASS on this SHA.

- Root (2026-10-17): D3 SHA `c462e4eb…8a89` (delta on D2) sent to B for recheck R004
  (F009–F011, F004 premise, §5.1 rows 11–21, §L phase06 deferral judgement).
  Anomaly noted: A reports D2 and D3 were reformatted on disk after writing (likely a
  local hook); A re-read and re-hashed the final text each time.

- 2026-10-17: B 04.11-R004 — CHANGES REQUIRED: F009–F011 resolved; blocking low F012
  (resume_on effect branch ignores evidence conflict) and F013 (four schema-convention
  gaps). §L phase06 deferral judged acceptable with three conditions. A assigned D4 delta.

#### 04.11-D4 correction — 2026-10-17 (A, implement-opus-reviewed-2.1)

A wrote a single correction round for 04.11-R004: `logs/04.11/decision-D4.md`, SHA256
`6bf5efcb3de54949df516b45725c8c9d4fe8cc1ff968a38671a367862e1fa276`.
- It is a small delta. The effective decision is D2 `1dcd1a9e…3a44` + D3
  `c462e4eb…8a89` + D4. D4's header table names every amended section. D1–D3 are
  unchanged (hashes re-verified).

Dispositions:
- **F012 (low), adopted (D4 §M).**
  - The `resume_on` effect branch checks `workflow_action_has_evidence_conflict` under
    the run lock before any advance, for accepted and failed receipts alike.
  - A conflict also wakes the effect wait.
  - On a conflict the step gets the same Reconcile outcome that `complete_io`/`retire_on`
    produce for Terminal `action.evidence_conflict` / EffectOutcomeUnknown:
    - wait `failed/evidence_conflict`, job `failed`, run `waiting/reconciliation`;
    - `record()` is called;
    - unsent siblings are superseded with reason `evidence_conflict`;
    - no route, successor or output; the receipt is retained.
  - Tests are in §S.
- **F013 (low), all four items adopted (D4 §N–§Q):**
  - `applied_request='delivery'` for message applied evidence, kept out of the remote
    `unattributed` logic;
  - the reopen guard has an SQL evidence-conflict check;
  - a message effect-state branch right after the receipt branch, so a row-16 reopen
    projects `possible_dispatch`;
  - the existing conflict orientation: `evidence_id` = breached proof,
    `contradictory_evidence_id` = applied evidence, `finality_breach`.
- **R004 §L conditions, adopted (D4 §R, §S):**
  - BRIEF-04.11 records the phase-06 reopen obligation and its trigger;
  - message settlement never enters the shared `continue_on`; the reopen outcome is
    renamed `DeliveryReopened` with no scheduled job;
  - a port-seam test checks that the parked agent execution stays parked while its
    receipt lands and `retry_safe` turns TRUE;
  - the 04.7 `continue_on` divergence is recorded.

**Correction of earlier records.** The D2 and D3 correction subsections above say the
decision files were "reformatted on disk by tooling" after writing. That was a mistake
by A.
- On re-inspection, the on-disk text of D2 and D3 matches the content A passed to the
  Write tool.
- The only post-write change to D2 was A's own `perl` fix of two line-number citations
  (`ingress.rs:39`, `20261001090000:8`), which raised the harness "changed on disk"
  notice.
- The four precision edits to D3 and the three wording edits to D2 were A's own Edit
  calls, made before hashing.
- No process rewrote the files. D4 on disk also matches its input.
- The recorded SHA256 values are of the final files and remain valid.

Records and next steps:
- No source edits, builds or fixture start. REVIEW, notes and findings are untouched.
- Next: B rechecks F012 and F013 only. Implementation starts only after B's PASS on
  this effective decision.

- Root (2026-10-17): the "reformatted on disk" anomaly noted above is retracted — A
  reports the files match its own Write/Edit calls (only its own perl citation fix
  followed). Recorded hashes stand. D4 SHA `6bf5efcb…a276` sent to B for recheck R005.

### 04.11 architecture gate ACCEPTED — 2026-10-17

B 04.11-R005 PASS for the effective decision D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89`
+ D4 `6bf5efcb…a276` (hashes recomputed by B before and after). Root accepts the gate
(decision level only). Open: optional F014 (shape-CHECK wording; conflict refusal for
multi-message model_tool executions — not reachable in phase04). Lightly covered areas
deferred to batch review: completion_job interplay, reconciliation Rust shapes,
claim/episode/budget SQL for message rows, fair_polling/inspection.
Next: A implements the cohesive 04.11 batch, then combined phase04 gate, freeze, B
whole-batch review.

### 04.11 implementation — 2026-10-17 (A, implement-opus-reviewed-2.1)

Effective decision: D2 `1dcd1a9e…3a44` + D3 `c462e4eb…8a89` + D4 `6bf5efcb…a276`
(R005 PASS; root accepted). Optional R005 F014 handling:
- F014.1: the NULL `applied_request` shape alternative stays subject-neutral, and the
  effect-kind ⇔ subject rule lives in the commit guard.
- F014.2: adopted. An execution-conflict refusal is added to message publication and to
  the workflow `begin_part` recheck.

Resources:
- Disk: 3.5 GiB free. `cargo clean -p mail_agents` (root-approved) removed 18.9 GiB,
  leaving 18 GiB free.
- Fixture `/private/tmp/workflow0410-20261017` started with pg_ctl. Verified system id
  7697669855432439122, role `workflow0410`, both databases, PG 18.6.
  Log: `logs/04.11/pg-start.log`.
- Warm build `cargo test --locked --offline --lib --no-run`: exit 0
  (`logs/04.11/warm-build.log`).

Planned sub-batches (focused checks per D2 §8.1 / D3 §K / D4 §S areas):
1. Migration `20261017120000_workflow_messaging.sql`.
2. Registry, publication and authority built-ins.
3. Handler, supervision and publication transaction.
4. Delivery engine changes.
5. Consumer, effect-wait resume and conflict veto.
6. Recovery predicates.
7. 04.7 reconciliation extension.
8. Tests.
9. `lease-budget.sh`, BRIEF-04.11 and `NO_BACKWARD_COMPATIBILITY_NOTES`.
10. Final gate.

Implemented (A). All ten sub-batches. The migration was edited after its first apply,
so the fixture test database was reset and re-migrated (no backward compatibility;
`logs/04.11/migrate-reset.log`).

Main surfaces:
- Migration `20261017120000_workflow_messaging.sql`: vocabulary, the four message tables,
  evidence delivery subjects, delivery provenance, admission/capture/aggregate/receipt/
  state/wait guards, replaced dispatch and evidence commit guards, reconciliation guard,
  effect-state view and fenced-completion guard (effect park).
- Registry built-ins (`message.send`/`message.reply`, `InputCheck::Message`), trusted
  snapshots in publication and binding readiness, the messaging resource in
  `resources::inspect_on`.
- Application: `workflow/message.rs` (+ `message_runtime.rs`), `WorkflowHandlerOutcome`
  and `confirm_suspension` in supervision, `PollWork::MessageReceipt`,
  `ActionFamily::Message`, `marker_owned_effect_kind` for message kinds, reconciliation
  `ReconciliationTarget`, `DeliverySubject`, `AppliedEvidenceRequest::Delivery`,
  `ReconciliationOutcome::DeliveryReopened`.
- Persistence: `workflow/message.rs`, `message_publish.rs`, `message_receipts.rs`,
  `action_reconciliation/delivery.rs`, `delivery/evidence.rs`
  (`settle_unknown_delivery_on`), workflow cancellation in `delivery/cancellation.rs`,
  `begin_part` rechecks, `complete_part` sending guard and reaper parent locks in
  `delivery/queue.rs`, wait/poll/controls/maintenance/recovery wiring.
- Composition: `src/adapters/workflow_message/mod.rs` (`MessageRuntime`).
- `scripts/lease-budget.sh` (`message.publish`, `message.consume`, filters
  `workflow_message_publish_`/`workflow_message_receipt_`; no threshold changed),
  `BRIEF-04.11.md` (phase-06 obligation and trigger, `continue_on` divergence) and its
  `NO_BACKWARD_COMPATIBILITY_NOTES` entry.

Implementation details and deviations from the decision text (none changes a decided
behavior):
1. Purposes are `workflow_send` (initiates; refused by a reply-only binding) and
   `workflow_reply` (allowed by one), rather than one workflow value. The accepted
   delivery test now names both new-conversation purposes.
2. Email composition uses the scheduled-delivery relay precedent (hop 0, the sending
   channel); the predicted Message-ID derives from `invocation:<id>` and the destination.
3. The delivery and part provenance guards apply only to workflow-linked deliveries;
   other deliveries keep their existing transitions.
4. `ReconciliationSnapshot` carries `delivery: Option<DeliverySubject>` beside the
   unchanged remote `entries` instead of a `SubjectFacts` enum; `validate()` enforces that
   exactly one applies and that it matches the command target.
5. Applied delivery evidence carries `accepted_at` (for the finality-breach check in the
   same settlement transaction) and no attested provider key; the provider key stays the
   predicted Message-ID.
6. The effect park writes its attempt/job close inside the publication transaction
   (`park_on`). The accepted fenced-completion guard is replaced to admit exactly that
   close (`effect_parked`, waiting effect wait); every other close is unchanged.
7. A failed receipt completes the wait, takes `recovery::route_failure` and closes the
   parked job `failed`, as retirement's final error does. A later resume of that
   completed wait is refused (the wait no longer owns the step).
8. The accepted-receipt resume completes through `prepare_io` (the step is an I/O step).
9. A `Remote` target on a message marker (or the reverse) is the non-disclosing missing
   subject (`marker_on` filters on the target's effect kind); D3 §H replaced D2's
   `reconciliation.unsupported_effect`.
10. An ambiguous publication COMMIT resolves by reading the frozen intent directly; the
    dispatch `load_on` requires a live run actor and would never see a committed park.
    The three-way race test found this.
11. `authority::authorize_company` takes `&mut PgConnection` (all callers unchanged by
    deref coercion) so `begin_part` can reuse it.
12. HTTP and MCP runtimes keep their step logic in `run()` (a completed result) and wrap it
    in `WorkflowHandlerOutcome`; existing tests call `run`.
13. The SQL-only admission backstop serializes two writers whose first write is the marker
    (54000 for the second). A writer that inserts its intent in the same transaction first
    takes a foreign-key share lock on the execution, which the competitor's deferred
    dispatch guard then needs `FOR UPDATE`; PostgreSQL aborts one with 40P01. Exactly one
    still commits. This sharpens the D3 §A wording; no code change.
14. Receipt discovery is one `CANDIDATES_SQL` branch that does not depend on
    `execution.completed_at`; its `deadline` arm now requires an incomplete execution.
    Indexes added for open actions per execution, action waits and run-scoped links.
15. The remote evidence insert is the accepted 04.7 statement unchanged; only message
    evidence adds the subject columns.
16. `confirm_suspension` uses a 2 s statement bound (the trait has no policy argument).
17. Accepted catalog pins updated for the deliberate D3/D4 replacements: the attribution
    grammar/shape constraint texts, the dispatch and evidence commit guard sources (now the
    messaging migration), the coverage RAISE line (27 → 39), the coverage fixture's column
    exclusion, and the upgrade test's preserved-bytes check (added columns read NULL).
    Fixtures and examples use UUID destinations and sources.
18. Test infrastructure: `lease_resolution_gate.rs` gained `GatePool` and module-wide
    visibility so the message race test can arm it.

F014.1: the NULL `applied_request` alternative stays subject-neutral; the commit guard
enforces effect kind ⇔ subject and message applied ⇔ `delivery`.
F014.2: adopted; publication refuses a conflicted execution and workflow `begin_part`
refuses with `evidence_conflict`.

Known gaps (not covered by a dedicated test; behavior implemented):
- inbound resolution of a recipient's reply to a workflow email (D2 §1.5 says the
  inbound path is unchanged; not re-tested here);
- Slack sources for `message.reply` (refused by the same rule as outbound sources);
- a delayed COMMIT past the captured fence expiry or the run deadline (guards present);
- wrong-association reconciliation actors (non-admin and cross-company are tested);
- deliberate races of settlement against the consumer, `begin_part` and cancellation;
- reopen-guard refusal for an expired proof and remote evidence carrying a delivery
  subject (shape CHECK and foreign keys tested; conflict and double use tested);
- the step-level `message.approval_required` (the built-in policy never requires
  approval in phase 04);
- the D4 §S F012 variant "breach, then every destination accepted" (the breach test
  covers a pending sibling and the failed receipt that follows);
- an explicit per-transaction lock-count assertion (bounded by the 20-destination CHECK).

Focused checks (`logs/04.11/focused-checks.sh`, statuses `focused-status.log`): A 63,
B 33, C 90, D 386, E 99, F 197, H 256 tests — all pass. The first run failed 14 accepted
tests (A 1, D 12, E 1; H repeats D's 12) on the deliberate catalog and fixture changes of
item 17. The rerun overwrote those logs; the failures are described here, not retained.

Head note: commit `5c1de32` ("workflow-foundation 4.10, 4.11", author matjazonline,
2026-10-17 20:24 UTC) landed during implementation and includes part of this work. A made
no commit.
Commit `5c1de32` contains all 53 files of the accepted 04.10 snapshot
(`logs/04.10/post-gate-snapshot.txt`, base `8ce1fd4`) plus 26 files that only 04.11 touches.
The 04.11 files already in the commit are:
- the messaging migration;
- `delivery/{cancellation,queue}.rs`;
- workflow persistence `action_uncertainty`, `controls`, `lease`, `maintenance`,
  `message{,_publish,_receipts}`, `polling{,_tests,_integration_tests}`,
  `fairness_worker_tests`, `wait_commit` and `waits`;
- application `lease`, `message{,_runtime}`, `polling`, `registry/{checks,schemas}`,
  `supervise{,_tests}` and `waits`;
- `domain/entities/transport/delivery.rs`.

Shared 04.10 files changed by 04.11 (`authority.rs`, `recovery.rs`, `resources.rs`, the
HTTP/MCP runtimes and adapters, the registry and `actions`) are also in the commit. The
snapshot below is therefore relative to `5c1de32`: the full 04.11 change is that commit's
04.11 content plus the listed working-tree delta.

#### 04.11 final gate — 2026-10-17 (A)

Scripts:
- `logs/04.11/final-gates.sh`: statuses in `final-gate-status.log`, one log per gate at
  `logs/04.11/final-gate-<name>.log`.
- `logs/04.11/final-gates-rerun.sh`: statuses in `final-gate-rerun-status.log`.

Environment:
- Fixture: `127.0.0.1:55450`, database `workflow0410_test` (PG 18.6).
- Variables: `SQLX_OFFLINE=true` except where a gate sets `false`, and `CARGO_BUILD_JOBS=4`.
- Rust tests run under `scripts/test-network-isolation.sh` (loopback allowed; external
  IPv4/IPv6 rejected).

| Gate | Command | Result | Log |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check` | PASS | `final-gate-fmt.log` |
| diff | `git diff --check` | PASS | `final-gate-diff.log` |
| transport | `scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh` | PASS | `final-gate-transport.log` |
| credential-deploy | `scripts/tests/credential-key-rotation.sh && scripts/tests/deploy.sh` | PASS | `final-gate-credential-deploy.log` |
| offline | `cargo check --locked --offline --all-targets` | PASS | `final-gate-offline.log` |
| clippy | `cargo clippy --locked --offline --all-targets -- -D warnings` | PASS | `final-gate-clippy.log` |
| fresh-schema | new `workflow0410_fresh_test`, `cargo sqlx migrate run` (`SQLX_OFFLINE=false`), `pg_dump -s` of fresh and fixture, `diff -u` | PASS: empty diff, 14454 lines each | `final-gate-fresh-schema.log`, `fresh-schema.sql`, `fixture-schema.sql` |
| sqlx-prepare | `cargo sqlx prepare -- --locked --offline --all-targets` (`SQLX_OFFLINE=false`) | PASS | `final-gate-sqlx-prepare.log` |
| sqlx-check | `cargo sqlx prepare --check -- --locked --offline --all-targets` (`SQLX_OFFLINE=false`) | PASS | `final-gate-sqlx-check.log` |
| no-run | `cargo test --locked --offline --all-targets --no-run` | PASS | `final-gate-no-run.log` |
| full-suite (1st) | `scripts/test-network-isolation.sh cargo test --locked --offline --all-targets` | FAIL 101: 2654 passed, 2 failed, 22 ignored | `final-gate-full-suite.log` |
| stack-budget (1st) | `scripts/test-network-isolation.sh scripts/stack-budget.sh --offline` | FAIL 101, unrelated to stack: 2655 passed, 1 failed (same query-health test), no overflow | `final-gate-stack-budget.log` |
| lease-budget | `scripts/test-network-isolation.sh scripts/lease-budget.sh --offline` | PASS: 97 tests, every op within bound | `final-gate-lease-budget.log` |
| graph | `graft build` | PASS: 906 cards | `final-gate-graph.log` |
| ci-stat-settings | `SHOW` track=`top`, track_utility=`off`, log_parameter_max_length(_on_error)=`0` | PASS | `final-gate-rerun-status.log` |
| full-suite (rerun) | same as the first full-suite run | PASS: 2661 passed, 0 failed, 22 ignored (lib 2656 + other targets 5) | `final-gate-full-suite-rerun.log` |
| stack-budget (rerun) | same as the first stack-budget run (2048 KiB) | PASS: 2656 passed, 0 failed, 22 ignored | `final-gate-stack-budget-rerun.log` |

In the lease-budget table:
- `message.publish`: 27 events, max work 19 ms against a 2000 ms bound.
- `message.consume`: 53 events, max work 2 ms against a 2000 ms bound.
- 92 injected events are excluded.
- No threshold was changed.

Diagnosis of the first-run failures (no source change; `logs/04.11/rerun-full-suite-failures.log`):
- `database_query_health::tests::database_snapshot_executes_or_reports_that_the_library_needs_preloading`
  (`database_query_health.rs:272`, `!snapshot.utility_tracking`) is environmental.
  - The fixture had `pg_stat_statements.track_utility=on` (the default), and CI sets it
    `off` (`.github/workflows/ci.yml:80`, checked at line 91).
  - The fixture now carries CI's settings via `ALTER SYSTEM` + `pg_reload_conf()`
    (`logs/04.11/fixture-ci-stat-settings.log`): track `top`, track_utility `off`,
    compute_query_id `on`, log_parameter_max_length(_on_error) `0`.
  - The file is unchanged since before 04.10.
- `recovery_tests::workflow_failure_exhausted_expiry_routes_and_deadline_delay_rolls_back`
  (`recovery_tests.rs:337`, `result.is_err()`) is a load-sensitive timing window in an
  accepted, unchanged test.
  - The test gives the run a 0.15 s deadline, then takes a snapshot and calls
    `retire_expired_io`.
  - If the deadline passes before `retire_on` reads `clock_timestamp()`, retirement takes
    the deadline route and returns `Ok` rather than meeting the delayed-commit guard.
  - It passed 5 of 5 isolated runs (while the stack-budget run loaded the machine), in the
    stack-budget run and in the full-suite rerun.
  - Corrected in R006 (F018): 04.11 does change two functions near this path, though
    neither moves the window. `recovery::report` now classifies message kinds as
    marker-owned (this test's step kind is unaffected), and `maintenance::expire_on` runs
    one added run-scoped `cancel_message_deliveries_on` statement, but only on the deadline
    route, after the window is already lost. `retire_on`, the retirement guard and the delay
    trigger are unchanged. No stashed-baseline rerun was done (see "04.11 R006 corrections").

Checks not run:
- CI frontend steps: npm install, generated CSS, Playwright browser protocol, SSE
  navigation. 04.11 changes no frontend, CSS or SSE code.
- The CI query-statistics `queryid` probe. The fixture settings were checked
  (`ci-stat-settings`), but the probe query itself was not run.
- `scripts/stack-frames.sh` (not needed: no overflow).

The full required gate (D2 §8.3 as amended) passes after the rerun. Only the full suite
and stack budget were rerun; no source changed between the first run and the rerun.

#### 04.11 frozen for independent review — 2026-10-17 (A)

`PATH=/private/tmp/workflow0410-tools:$PATH bash .claude/skills/implement-opus-reviewed-2.1/scripts/snapshot.sh plan/workflow-foundation/docs`
(`logs/04.11/implementation-snapshot.txt`):
- base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`
- digest `8891d74001e78bac6b65e0238c3d73ac25b92f3970178edb08f296417c75cabd`
- 51 files relative to the base; only `plan/workflow-foundation/docs` is excluded besides
  the script's own `.agents`/`.claude`.

Edits to TASK.md after the snapshot are excluded from it. A made no commit, deployment,
migration squash or production change, and did no phase 05+ work. REVIEW.md and the
agent-review files are untouched.

- Root (2026-10-18): A reports 04.11 implemented and frozen: base `5c1de32` (user commit
  made during implementation, not by A), digest `8891d740…cabd`. Combined gate rerun
  green (full 2661/0/22 ignored; stack 2656/0; lease 97; schema/SQLx/clippy/fmt/graft).
  Sent to B for whole-batch + final-integration review R006, including judgements on the
  18 deviations, known untested gaps, first-run failure handling and gate-deferred areas.

- 2026-10-18: B 04.11-R006 — CHANGES REQUIRED for `8891d740…cabd`. Blocking F016 (medium,
  missing decision-named tests incl. races), F015 (low, two unlisted deviations:
  revocation→Retryable not Terminal message.rejected; reply relay trace reset). Optional
  F017, F018. Deviations do not reopen gate; first-run failure handling accepted;
  gate-deferred areas sound. A assigned one correction round.

### 04.11 R006 corrections — 2026-10-18 (A, implement-opus-reviewed-2.1)

Input: R006 CHANGES REQUIRED for `8891d740…cabd` (F015 low, F016 medium; F017, F018
optional). REVIEW.md and the agent-review files are untouched.

Per-finding disposition:

| Finding | Disposition | Where |
|---|---|---|
| F016.1 F012 variants | Added; no code defect. One test-wording amendment proposed (A1 below). | `message_reconcile_veto_tests.rs` `…finality_breach_then_every_destination_final_vetoes_the_receipt` (accepted and failed receipt) |
| F016.2 F005 | Added: per-recipient inbound resolution and Slack-source refusal. | `message_thread_tests.rs` `…inbound_reply_resolves_each_recipients_copy`; `Source::Slack` in `…reply_refuses_sources_it_cannot_answer` |
| F016.3 F007 | Added: delayed COMMIT past the captured fence expiry and past the run deadline. | `message_race_tests.rs` `workflow_message_race_delayed_commit_past_fence_expiry_or_deadline_rolls_back` |
| F016.4 F010 SQL negatives and actor | Added: reopen from expired, non-final and foreign evidence; wrong-association actor. | `message_reconcile_veto_tests.rs` `…sql_reopen_refuses_expired_non_final_and_foreign_evidence`, `…wrong_association_actor_is_refused` |
| F016.5 races | Added four real competing-claimant tests, all `Injected` with the `workflow_message_race_` prefix. | `message_reconcile_race_tests.rs` |
| F016.6 §L seam | Added: `DeliveryReopened` on a `model_tool` execution reopens only the delivery. | `message_reconcile_veto_tests.rs` `…model_tool_delivery_reopen_never_reopens_the_job` |
| F015.1 row 2 | Conformed (code); step-level revocation tests added. | `MessagePublication::Rejected`, `message_publish.rs` `rejected_on`, `message_runtime.rs`; `message_authority_tests.rs` |
| F015.2 reply relay | Conformed (code); test added. | `message.rs` `source_relay_on`, `RouteTarget::ReplyEmail.relay`, `EmailPlan.relay`; `…external_reply_continues_the_source_relay_trace` |
| F017 lease-lost rollback | Adopted. | `delivery/queue.rs` `begin_part` |
| F017 inspection | Not wired; claim withdrawn (A2 below). | — |
| F018 | TASK wording corrected; oracle recorded; no baseline (below). | TASK "04.11 final gate" diagnosis |

Implementation notes:

1. **F015.1.**
   - `MessagePublication::Rejected(AppError)` is returned by `observe` and
     `publish_on` when `authorize_on` or the route recheck refuses
     (`NotFound`/`BadRequest`/`InvalidCredentials`) and the invocation has no marker (the
     check runs in the same transaction).
   - The step maps it like a preparation refusal: Terminal `message.rejected`,
     `SafeToRetry`. The tool path returns the original error.
   - With a marker, the error stays the retryable dispatch failure.
   - A demotion already present before the step was, and still is, refused by the shared
     request preparation (`http_runtime::facts` → `prelock_on`) as Terminal
     `message.rejected`. The retry the review found applies to revocations landing after
     preparation; the hooked tests place them at `observe` and at `publish`.
2. **F015.2.**
   - The recorded relay trace exists only in the source's inbound task payload
     (`InboundTaskPayloadV1.hop_count`/`trace_channels`, one task per source message and
     channel).
   - `source_relay_on` reads it in route resolution, and `plan_email` continues it exactly
     as the agent answer does (`dispatch.rs`: hop as recorded, trace as recorded, the
     renderer adds the hop and the sending channel).
   - A source with no recorded task starts a chain at hop 0, which is what an agent answer
     to direct mail sends.
   - A payload that does not decode is refused (`message.rejected`), not guessed.
   - User email keeps the scheduled-delivery precedent (hop 0, trace = run channel).
3. **F016.5 race harness.** A separate connection holds the run row `FOR UPDATE` until
   every competitor is observed queued behind it (`resolution_gate::waiters`, transitive
   over `pg_blocking_pids`), then releases it, so the competitors race for the real locks.
   Assertions are order-independent:
   - settle (applied) vs the consumer of the same action: one receipt, accepted, then the
     step resumes;
   - a reopening settle vs the consumer vs `begin_part` of an unsent sibling: each applies
     once, with no consumer failure;
   - two settlements from one revision: exactly one reopen;
   - settle vs workflow cancellation: the revision loser refuses cleanly and its retry
     converges on `dead_letter`/`workflow_cancelled`, with no provider request.
4. **F016.3.** A deferred trigger named to fire before the dispatch guard sleeps past the
   bound the guard reads. A sequence (not rolled back) proves the COMMIT was reached. The
   COMMIT fails, and markers, action, destinations, deliveries, waits, messages and the
   user thread are all absent.
5. **F016.4.**
   - A control insert that differs only in the refused fact passes the guard and is
     stopped by one-use consumption (23505), so each 23514 isolates expiry, finality or
     foreignness.
   - Cross-run and cross-invocation evidence is additionally excluded by the composite
     foreign keys.
6. **Lock count.** Structurally bounded by the `destination_count BETWEEN 1 AND 20` and
   ordinal 0–19 CHECKs; no separate assertion added.

Proposed decision amendments, for the reviewer to judge (no gate reopened):

- **A1 (D4 §S F012 wording).**
  - The receipt variants' "an uncertainty row with `action.evidence_conflict`" cannot hold
    for a receipted marker. D4 §M step 4's own writer `action_uncertainty::record` skips
    receipted markers ("receipts retain precedence"). The accepted
    `workflow_action_reconciliation_guard` raises 23514 for an observation of a marker
    with a receipt.
  - The tests assert what the decided writer produces:
    - the first-uncertainty observation (`message.outcome_unknown`) is retained and no
      new row is added;
    - the `finality_breach` conflict row;
    - `effect_state` `committed` with `evidence_conflict` true;
    - the full park.
  - The conflict-while-pending variant (no receipt) still records
    `action.evidence_conflict` (`…finality_breach_vetoes_resume_and_reopen`).
  - Proposed text: "the execution's uncertainty as recorded: a receipt-less marker gains
    `action.evidence_conflict`; a receipted marker keeps its observations and shows the
    conflict through the effect-state view".
- **A2 (D3 §E).** No inspection surface consumes candidate discovery (`inspection.rs` is
  the run-head reader). Discovery is shared by `polling::poll_work` and `fair_polling`,
  and BRIEF-04.11 never claimed inspection. Proposed: drop "and `inspection`".

F018:
- The diagnosis wording is corrected in place under "04.11 final gate".
- The window is a known load-sensitive oracle of an accepted test. The reviewer's
  suggested fix (deadline after `snapshot`) would change an accepted test and is left for
  a reviewed change.
- No stashed-baseline rerun was done, so this is reported as not run. Stashing would
  disturb the shared worktree that must be preserved, and a separate worktree at `8ce1fd4`
  needs a full rebuild that exceeds the free disk margin.

Focused checks (`logs/04.11/r006-focused-checks.sh`, same areas and filters as the first
round; statuses in `r006-focused-status.log`, logs `r006-focused-<area>.log`):
- A 63, B 36, C 91, D 386, E 100, F 199, H 267: all pass.
- New-test iterations: `r006-new-tests-1.log` had 18 passing and 1 failing (the boundary
  test first expected the pre-step demotion to reach `observe`; preparation refuses it,
  so the test moved its revocation to the `observe` hook). `r006-new-tests-2.log`: 2/2.

Final gate rerun (`logs/04.11/r006-final-gates.sh`, the same commands as
`final-gates.sh`; statuses `r006-final-gate-status.log`, logs
`r006-final-gate-<name>.log`; fixture still on CI's `pg_stat_statements` settings):

| Gate | Result |
|---|---|
| fmt, diff, transport, credential-deploy | PASS |
| offline (`cargo check --locked --offline --all-targets`) | PASS |
| clippy (`-D warnings`, all targets) | PASS |
| fresh-schema | PASS: empty diff, 14454 lines each (`r006-fresh-schema.sql`, `r006-fixture-schema.sql`); no migration changed in R006 |
| sqlx-prepare, sqlx-check | PASS |
| no-run | PASS |
| full-suite (network-isolated, all targets) | PASS: 2674 passed (lib 2669 + 5), 0 failed, 22 ignored |
| stack-budget (2048 KiB) | PASS: 2669 passed, 0 failed, 22 ignored, no overflow |
| lease-budget | PASS: 99 tests; `message.publish` max work 21 ms, `message.consume` 2 ms, `message.observe` 11 ms; 92 injected events excluded; no threshold changed |
| graph (`graft build`) | PASS |

Not run, as before: the CI frontend steps, the `queryid` probe and `stack-frames.sh`.

Changed in R006:
- Source:
  - `src/application/workflow/message.rs`
  - `src/application/workflow/message_runtime.rs`
  - `src/application/workflow/actions/failure.rs` (doc comment)
  - `src/adapters/persistence/workflow/message_publish.rs`
  - `src/adapters/persistence/workflow/message.rs`
  - `src/adapters/persistence/delivery/queue.rs`
- Tests:
  - new: `message_authority_tests.rs`, `message_reconcile_veto_tests.rs`,
    `message_reconcile_race_tests.rs`
  - changed: `message_race_tests.rs`, `message_thread_tests.rs`,
    `message_reconcile_tests.rs` and `message_tests.rs` (module registration)
- Docs: `plan/workflow-foundation/BRIEF-04.11.md` (row 2 refusal and relay sentences), this
  file.

Refrozen with `PATH=/private/tmp/workflow0410-tools:$PATH bash .claude/skills/implement-opus-reviewed-2.1/scripts/snapshot.sh plan/workflow-foundation/docs`
(`logs/04.11/r006-snapshot.txt`):
- base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`
- digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf`
- 56 files; the five new to the delta are `delivery/queue.rs`, `actions/failure.rs` and
  the three new test files.

No commit, deployment, migration squash, production change or phase 05+ work.

- Root (2026-10-18): A R006 corrections frozen at digest `e76ef46c…c2cf` (base 5c1de32);
  full 2674/0/22, stack 2669/0, lease 99, static/SQLx/schema/graft green. Proposed
  amendments A1 (F012 receipted-variant uncertainty assertion) and A2 (drop inspection
  from D3 §E) sent with B recheck R007.

### 04.11 ROOT ACCEPTED; phase04 combined gate complete — 2026-10-18

B 04.11-R007 **PASS** for digest `e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf`
(base `5c1de32ecb3c1a7e59e1a60deb74ab5c42e755df`, exclusion `plan/workflow-foundation/docs`),
covering the 04.11 whole batch and combined phase04 final integration. Effective decision
D2+D3+D4 with reviewed amendments A1, A2. Root independently recomputed the digest
(match) and read `logs/04.11/r006-final-gate-status.log`: offline, clippy, fresh-schema,
SQLx prepare/check, no-run, full suite (2674/0/22 ignored, network-isolated), stack budget
2048 KiB (2669/0), lease budget (99, no threshold change), graft — all PASS; fmt, diff,
transport, credential/deploy PASS earlier in the same script. Findings F001–F018 resolved
or optional-dispositioned; none open. **04.11 ACCEPTED; phase04 (04.1–04.11) complete.**

Not run (explicit gaps): CI frontend/CSS/Playwright/SSE steps (no frontend change), CI
`queryid` probe (settings only checked), `stack-frames.sh`, F018 stashed-baseline rerun
(disk/worktree constraints). Task fixture `/private/tmp/workflow0410-20261017` changed to
CI `pg_stat_statements` settings during the gate; root verified sysid 7697669855432439122
and stopped it (`pg_ctl -m fast stop`, data retained). No author/reviewer checks running.
No commit, deployment, migration squash or phases 05+ by the workflow (the user made commit
5c1de32 during implementation). Review rounds: 04.11-R002…R007 (R001 historical).

## Phase 05 — 2026-10-18 — implement-opus-reviewed-2.1

### Scope and authorization

User instruction 2026-10-18: "use /implement-opus-reviewed-2.1 and continue RESUME.md
(phase 05)"; "'an eligible company reviewer group' — limit to explicit reviewers";
"squash migrations". Authorized: migration squash and phase 05 (05.1–05.4 + phase05
gate). Not authorized: commits, deployment, production data, phases 06+.

Product decisions (user): reviewers are **explicit only** — no reviewer group or
group-membership model (consistent with 04.11 rejecting groups). Plan text in
`05-human-decisions-and-waits.md` / EXPANSION 05.3 to be amended to match.
Migrations are squashed into one init schema; no backward compatibility, databases reset.

Original requirements: [Phase 05](../05-human-decisions-and-waits.md), EXPANSION rows
05.1–05.4, accepted phase 04 contracts/briefs (approval seams, waits/effect waits,
04.11 messaging for notifications), `AGENTS.md` (incl. NO_BACKWARD_COMPATIBILITY_NOTES
for any new plan) and applicable `src/**/AGENTS.md`.

Baseline: HEAD `4b54485` (user commit "workflow-foundation 4.x complete"; contains the
accepted 04.11 state), clean worktree. Disk ~9 GiB free.

### Batches (root proposal; A may refine in its gate with reasons)

S. **Migration squash** — collapse all migrations into a single init schema; fresh
   schema must be equivalent to pre-squash fresh schema (normalized dump diff); remove
   or adapt tests/scripts/docs that depend on individual migration files or populated
   upgrades; full suite. Accepted before phase 05 builds on it.
05. **Human decisions and waits** — 05.1–05.4 with one architecture gate (schema,
   concurrency, authorization, credentials) and one batch review, unless A justifies a
   split at a stable contract; then phase05 gate.

### Workers

Root: Opus 5.5, effort not confirmed xhigh → thin coordinator. Reusing A
`a4461afdf8d6050a9` and B `a4750bc4ff449a216` (opus / xhigh requested).

### Batch S — migration squash — 2026-10-18 (A, implement-opus-reviewed-2.1)

Baseline: HEAD `4b54485` with a clean worktree, apart from root's edits to this file. Logs are in
`logs/S/`. No commit, deployment, production data or phase 06+ work.

**Fixture.** The task fixture `/private/tmp/workflow0410-20261017` was restarted
(`logs/S/pg-start.log`).
- Identity verified: system identifier `7697669855432439122`, PG 18.6, role `workflow0410`,
  CI's `pg_stat_statements` settings still in place.
- Databases:
  - `workflow0410_presquash`: fresh from HEAD's 56 migrations; kept as the equivalence baseline.
  - `workflow0410_postsquash`: fresh from the squash.
  - `workflow0410_test`: dropped, recreated and migrated (`logs/S/reset-test-db.log`).
  - `workflow0410_fresh_test`: used by the gate.

**Gate decision.** No architecture gate is needed: no choice here is non-mechanical or costly to
unwind.
- No data seeds. A fresh pre-squash database holds no row in any table
  (`logs/S/presquash-nonempty.txt` is empty). The later migrations' data statements are backfills
  over existing rows, or trigger bodies.
- The `sqlx` ledger keeps version `20260817000000` ("init schema"). Its checksum changes, so a
  database that applied the old history must be reset; that is the standing no-backward-
  compatibility rule. Only the removed upgrade tests read migration versions.
- No grants, roles, custom types, `SECURITY DEFINER` or new extensions (only the baseline's citext
  and pg_stat_statements).

**Method.** Squash by pg_dump, the same shape as the previous squashed baseline:
1. `pg_dump --schema-only --no-owner --no-privileges --exclude-table=public._sqlx_migrations` of
   the fresh pre-squash database (`logs/S/squash-source-dump.sql`).
2. Remove only:
   - pg_dump 18's psql `\restrict`/`\unrestrict` meta-commands, which are not SQL;
   - the version banner;
   - `SET transaction_timeout`;
   - `set_config('search_path','')`, because sqlx records its unqualified ledger row in the same
     session.
3. Replace the dump banner with the baseline header.
4. Write four CHECK constraints back in their source form, `octet_length(x) BETWEEN m AND n`:
   `workflow_action_reconciliations_failure_code_check`,
   `workflow_budget_receipts_reservation_key_check`, `workflow_definitions_title_check` and
   `workflow_http_credentials_envelope_check`.
   - pg_dump deparses BETWEEN as a nested `>= AND <=`. That re-parses as one flattened AND, which
     is the same predicate but a different stored expression.
   - With the source form back, the catalogs are identical.
5. Delete the other 55 files. Input record: `presquash-head.txt`, `presquash-migrations.txt`,
   `presquash-migrations.sha256`.

**Equivalence proof** (`logs/S/equivalence.sh`, `equivalence.log`; also the `squash-equivalence`
gate step). Fresh post-squash against fresh pre-squash:
- Full `pg_dump --schema-only`, covering owners, ACLs, comments, functions, triggers, views,
  sequences, extensions and the sqlx ledger table. It is normalized only for the random
  `\restrict` key and the version banner. Result: empty diff, 14454 lines each.
- Independent catalog fingerprint, 5868 entries. Result: empty diff. It covers:
  - `pg_get_functiondef` and function comments;
  - trigger definitions and enabled state;
  - constraint definitions with their deferral and validation flags;
  - index and view definitions;
  - each relation's kind, persistence, ACL and comment;
  - each column's type, nullability, default, identity, generation, comment and ACL;
  - sequence parameters, extensions and versions, non-base types, default ACLs, and namespaces with
    their ACLs.
- Rows: none in either database.
- Intended difference: the ledger holds 1 row instead of 56. No dropped-column slots existed
  before the squash, so column numbering is unchanged.

**Dependents adapted.**
- Removed the populated-upgrade tests. The plan's rule forbids them, and their historical
  migrations no longer exist.
  - `action_reconciliation_upgrade_tests.rs` (3 tests), `action_reconciliation_upgrade_catalog_diagnostics.rs`,
    and the test-only historical claim owner `action_reconciliation_upgrade_legacy_claim.rs` with
    its module declaration.
  - `workflow_budget_schema_retained_migration_preserves_exact_activations_and_seals`.
  - `workflow_wakeup_migrations_reconcile_existing_terminal_and_queued_children`, with its
    `remove_wakeup_schema`.
- Added `test_support::tests::a_fresh_database_applies_only_the_init_schema`, which generalizes the
  old fresh-ledger check. The fresh ledger is exactly the embedded init migration with its
  checksum, a rerun applies nothing, and no table is seeded.
- Six catalog pins read individual migration files: attribution, consumption identity, high bounds,
  native attribution, native coverage and state witness. They now read the init schema through one
  helper, `workflow::tests::init_function_body`, replacing four copies of the extraction code.
- `test_support::own_database_with_migrations` is now private; only the upgrade tests called it
  from outside.
- Comments that named migration files are updated (conflict-immutable test, chronology fixture), and
  so is the README "Database Migrations" section.
- Checked and left unchanged, because they are generic:
  - CI (`cargo sqlx migrate run`), `scripts/check.sh`, `scripts/reset-db.sh`, the transport-boundary
    scripts and the Dockerfile;
  - `scripts/tests/harness-query-plans.py`, whose regex still finds both functions in the new init;
  - `.sqlx` (see the prepare/check gates) and `src/infra/db.rs`;
  - `NO_BACKWARD_COMPATIBILITY_NOTES`, since no plan was added.
- Historical records are left unchanged: the BRIEF, CONTRACT, EVIDENCE, EXPANSION and ADMISSION
  history, and `docs/query-evidence/*` that cite past migration files.

**Needs the user's approval (not applied).** `src/adapters/persistence/AGENTS.md` §1 "Migration"
still says applied migrations are immutable and changes take a new follow-up migration. That
contradicts the user's squash decision and the phase 05–10 plan text. It is an agent-instruction
file, so I did not edit it on an agent's request. Proposed replacement for its first paragraph
and the `NOT NULL` paragraph:

> Pre-release, nothing is deployed and existing data is not preserved.
> `migrations/20260817000000_init_schema.sql` is the only migration and holds the whole schema: a
> schema change edits it in place, and the development and test databases are dropped and
> recreated (`scripts/reset-db.sh --all`) rather than upgraded. Do not add follow-up migrations,
> backfills, compatibility shims or populated-upgrade tests; verify against a freshly migrated
> schema. State a column's `DEFAULT` in SQL when queries that predate the change read it.

The apply-locally and `\d` confirmation steps that follow would stay as they are.

**Checks.**
- Focused (`logs/S/focused.log`): `a_fresh_database_applies_only_the_init_schema`,
  `test_support::`, `workflow_budget_schema`, `workflow_wakeup` and `action_reconciliation`, with
  every adapted catalog pin among them. Result: 272 passed, 0 failed.
- Full gate (`logs/S/gates.sh`: the 04.11 combined gate commands plus `squash-equivalence`;
  statuses in `logs/S/gate-status.log`, logs in `logs/S/gate-<name>.log`):

| Gate | Result |
|---|---|
| fmt, diff, transport, credential-deploy | PASS |
| offline check (all targets), strict Clippy | PASS |
| fresh-schema (fresh vs reset fixture test DB) | PASS: empty diff |
| squash-equivalence (pre vs post squash) | PASS: empty dump and fingerprint diffs |
| sqlx-prepare, sqlx-check | PASS; `.sqlx/` unchanged |
| no-run | PASS |
| full suite (network-isolated, all targets) | PASS: 2668 passed (lib 2663 + 5), 0 failed, 22 ignored |
| stack budget (2048 KiB) | PASS: 2663 passed, no overflow |
| lease budget | PASS: 99 tests; `message.publish` max work 96 ms, `message.consume` 2 ms, `message.observe` 11 ms, all against their bounds; 92 injected events excluded; no threshold changed |
| graph (`graft build`) | PASS |

- The lib count is 6 lower than the R006 rerun (2669): 7 upgrade-path tests removed (3 upgrade, 2
  catalog-diagnostic, 1 budget, 1 wakeup) and 1 fresh-ledger test added.
- Not run: the CI frontend steps, the `queryid` probe and `stack-frames.sh`, as before.
- Disk: 8.3 GiB free after the gate; no cleanup was needed.

**Frozen.** `PATH=/private/tmp/workflow0410-tools:$PATH bash .claude/skills/implement-opus-reviewed-2.1/scripts/snapshot.sh plan/workflow-foundation/docs`
(`logs/S/snapshot.txt`):
- base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`
- digest `0aeeca933e66244420f6b05b37d645cc79facbc2307890591b187d0f9ac43fa5`
- 74 paths: the 55 deleted migrations, the rewritten init schema, README, and the test files
  above (3 deleted).

- Root (2026-10-18): batch S frozen by A at digest `0aeeca93…3fa5` (base 4b54485); sent
  to B for review S-R001. Open user decision: amend `src/adapters/persistence/AGENTS.md`
  §1 (migration immutability) per A's proposed text above — asked of the user; not
  edited by agents.

- 2026-10-18: B S-R001 — CHANGES REQUIRED: S-F001 (low, lost coverage of sealed-legacy
  budget debit rule) blocking; S-F002 optional. Equivalence proof independently confirmed.
  Root: keep the sealing branch; A adds the test. B observation: persistence AGENTS.md
  lines 30, 160 and §1 all conflict with the squash — included in the user question.

#### Batch S — S-R001 correction — 2026-10-18 (A)

Input: S-R001 CHANGES REQUIRED for `0aeeca93…3fa5`. S-F001 is blocking; S-F002 is optional. The
squash itself was accepted.

- **S-F001.** Added `budget_schema_tests::workflow_budget_schema_sealed_roots_exhaust_every_debit`.
  It runs on a fresh schema, with no migration replay or schema reconstruction.
  - Two raw root runs are inserted beside the admitted one: one whose bundle has
    `compiler_revision` `"1"`, and one whose bundle does not decode. Each gets
    `workflow_root_budgets.provenance='sealed_legacy'` with NULL limits and zero usage.
  - A positive debit for each of `activation`, `model_call` and `repetition` comes back
    `exhausted`, and usage is unchanged.
  - Control: the admitted current-revision root is `frozen_v2` with limits [2,2,2], and its
    `model_call` debit is `granted` (usage becomes [0,1,0]).
  - Root's decision, recorded: the sealing branch stays for now; it is not removed in batch S.
- **S-F002, adopted.**
  - `workflow::tests::init_function_body` now searches only the function's own statement (up to the
    next pg_dump `-- Name:` header). It panics naming the function when there is no dollar-quoted
    body, and a `should_panic` test pins this on `valid_creation_provenance`.
  - The vestigial migrator parameter is gone: `test_support::migrated_own_database` runs
    `sqlx::migrate!` itself.
- AGENTS.md is still untouched; it waits for the user.

Checks (logs in `logs/S/`):
- Focused (`sr001-focused.log`): 28 passed, 0 failed, 1 ignored (the optional HydraDB adapter
  test). It covers `workflow_budget_schema` (with the new test), the helper's `should_panic` test,
  `a_fresh_database_applies_only_the_init_schema`, and every repinned catalog pin.
- `sr001-checks-status.log`: fmt PASS, Clippy (all targets, `-D warnings`) PASS, no-run PASS.
- `git diff --check` fails only on `agent-review-findings.md:1073`, a trailing space in the
  reviewer-owned file. Excluding that file it passes (`sr001-diff-excluding-review-log.log`). I left
  the file untouched.

Refrozen (`logs/S/sr001-snapshot.txt`):
- base `4b54485830495dfeb68d45c61c0d6d8dd684e4c5`
- digest `33ae9028d28d3047de936b2e9580ffe267bd0187dafe2092601d0cef51514552`
- 74 paths.

- 2026-10-18: B S-R002 PASS for `33ae9028…4552` (S-F001, S-F002 resolved). Root awaits
  A's network-isolated full-suite rerun (`logs/S/sr001-full-suite*`) before accepting S.

Full suite after S-R001 (root request; shared test infrastructure changed; no project file
edited). Command: `scripts/test-network-isolation.sh cargo test --locked --offline --all-targets`.
Statuses are in `logs/S/sr001-full-suite-status.log`.

| Run | Log | Result | Failed tests |
|---|---|---|---|
| 1 | `sr001-full-suite.log` | FAIL 101: 2663 passed, 2 failed, 22 ignored | `workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry` (entry 579 ms after a 335 ms deadline window); `workflow_mcp_refusals_and_protection_are_terminal_without_network` (`lease.claim` persistence bound elapsed) |
| 2 | `sr001-full-suite-rerun.log` | FAIL 101: 2661 passed, 4 failed | `…bounds_128_prior_consuming_129_total_then_lost_refuses` (`lease.release` commit outcome unknown); `…proof_expiry_reservation_enter_and_successful_replay` (`action.reserve_remote` bound elapsed); `…eligibility_exhausted_pending_has_no_retired_attempt` (`action.reserve_remote` commit outcome unknown); `…eligibility_poison_codes_preserve_history` (count 0 vs 1 after a timed-out step) |
| 3 | `sr001-full-suite-rerun2.log` | FAIL 101: 2662 passed, 3 failed | `…bounds_128_prior…`, `…proof_expiry_reservation…` (revision conflict after a timed-out step); `workflow_action_dispatch_resolution_adopted_reservation_refused_after_fence_loss` (`lease.claim` bound elapsed) |

- **Isolation.** Every failed test passed when rerun alone against the same binary
  (`sr001-full-suite-failures-rerun.log`): the run-1 pair 5/5, the run-2 four 3/3, the run-3 three
  3/3; 11 runs, 0 failures.
- **Failure family.** Every failure is a persistence-bound elapse, an ambiguous COMMIT ("commit
  outcome unknown"), or an assertion that follows one, in heavy lease, dispatch or reconciliation
  tests. The failing set differs from run to run, and no failing test touches the S-R001 changes:
  the sealed-budget test, `init_function_body`, or `migrated_own_database`, whose migrator is the
  same embedded `sqlx::migrate!` it received before.
- **Earlier runs.** The same family failed the 04.10 stock gates (`logs/04.10/gate-stock.log`,
  `resume-gate-stock.log`, including `bounds_128_prior…`). The batch S gate full suite on the
  pre-correction tree passed at 05:58Z.
- **Host conditions.**
  - Load averages were about 3.6–5.9 during these runs (`sr001-full-suite-rerun*-load.log`).
  - The reviewer's concurrent recheck and other agent sessions were running.
  - Disk free was 7.2–7.7 GiB (97% used).
  - No other database sessions were open between runs, and the fixture log shows only normal
    forced-drop terminations and sub-20 ms checkpoints.
- **Conclusion: no PASS is claimed for this tree's full suite.** The failures read as host-load
  timing and match the known family, but this is not proven, because the pre-correction tree was
  not rerun under the same load. A PASS needs a full-suite rerun on a quiet host: no concurrent
  reviewer runs, and optionally the build cache cleared to recover disk.
- Digest unchanged: `33ae9028…4552`.

Quiet-host rerun and control (root request, 2026-10-18). Statuses with start and end load
averages are in `logs/S/sr001-full-suite-status.log`.

| Run | Tree | Log | Load (start → end) | Result |
|---|---|---|---|---|
| Quiet rerun | S-R001 (`33ae9028…4552`) | `sr001-full-suite-quiet.log` | 2.79 → 3.31 (1 min) | FAIL 101: 2661 passed, 4 failed, 22 ignored |
| Control | pre-correction (`0aeeca93…3fa5`, verified before the run) | `sr001-full-suite-control.log` | 2.33 → 5.87 (1 min) | FAIL 101: 2656 passed, 7 failed, 22 ignored |

- **Quiet-rerun failures** (same family):
  - `…bounds_128_prior_consuming_129_total_then_lost_refuses` ("expected Pending provider's own
    loss");
  - three `workflow_action_dispatch_resolution_*` tests: `lapsed_reservation_stays_unknown`,
    `stalled_reservation_adopted` and `adopted_reservation_refused_after_fence_loss` (a revision
    conflict, or a reservation count of 0 after a lapsed bound).
- **Control failures** (same family, on the tree whose gate passed at 05:58Z):
  - `…siblings_128_and_129_snapshot_and_actual_recovery_never_truncate` (`lease.renew` commit
    outcome unknown);
  - four `…commit_applied_injected_*` tests (fixture lease lapsed between heartbeats; observer gate
    timeout);
  - `workflow_mcp_json_and_sse_run_end_to_end_with_typed_refs` and
    `workflow_message_publish_revocation_after_a_marker_stays_retryable` (`lease.claim` persistence
    bound elapsed).
- **Conclusion.** The pre-correction tree, which passed this suite earlier, now fails the same way.
  So the failures come from the current host's state, not from the S-R001 change.
- **Host state.** 10 CPUs and 16 GiB RAM, but swap is 4.47 of 5.12 GiB used (`vm.swapusage`), and
  only 7.0 GiB of disk is free (97% used). The suite itself drives the load to about 6. Every
  failure is in the persistence-bound and lease-timing family, and each test passes alone.
- **Restore.** The control temporarily rewrote the three S-R001 files to their pre-correction text,
  verified by digest. Afterwards the S-R001 copies were restored (saved outside the repo; sha256
  `383afc52…`, `0af03b0d…`, `e1864194…`), and the digest was re-verified as `33ae9028…4552`. No
  other source changed.
- **Still not claimed:** a green full suite on this host in its current state. Recommendation for
  root and the user: free memory and disk (close other sessions, clear regenerable build output),
  then rerun once, or accept the earlier batch S gate pass plus this control evidence.

- Root (2026-10-18): batch S full-suite not green on this host: quiet rerun 2661/4 and
  control on the pre-correction tree 2656/7 (same lease/dispatch/reconciliation timing
  family; all pass in isolation). Host under memory pressure (≈4.5 GiB swap used, 97%
  disk). The S-R001 change is not the cause (the control fails the same way), but root does not accept a red
  gate: S stays **not accepted** pending a green full run after the user frees host
  resources. Meanwhile A starts phase 05 investigation and gate drafting only
  (read-only; no source edits).

- User (2026-10-18): approved amending `src/adapters/persistence/AGENTS.md` §1, line 30
  and line 160 to the in-place init-schema model, and asked for a startup log flagging
  it for removal. Assigned to A within batch S (startup `serve()` warning updated; file
  already in NO_BACKWARD_COMPATIBILITY_NOTES). Root host check 10:01: swap used 4.2 GiB
  of 5 GiB (sticky), memory-free 79%, load 1.26 — A to rerun the full suite on the
  updated tree; B rechecks the delta.

- Root (2026-10-18): A's agent-relayed AGENTS.md edit was blocked by the permission
  classifier and reverted. The user approved the change directly to root, so root applied it:
  `src/adapters/persistence/AGENTS.md` (§1 in-place init schema + reset-db, composite-FK
  bullet, credential paragraph; all marked pre-release/temporary) and the `serve()`
  warning text in `src/main.rs`. fmt/diff-check pass. HEAD is now `feefe3e` (user commit
  "plan", contains frozen S tree; A verified S digest reproduces). A runs clippy, no-run,
  full suite, stack budget on this tree; B to recheck the delta.

#### Batch S — user-approved AGENTS/startup log — 2026-10-18 (A)

- **Edits.** Root applied them (user-approved): `src/adapters/persistence/AGENTS.md` (§1, the
  composite-FK bullet, the credential paragraph; all marked pre-release) and the `serve()` warning
  in `src/main.rs`. A reviewed both and found no defect. No test pins the warning text or the notes
  list (searched `*.rs`, `*.sh`, `*.toml`, `*.yml`).
- **Freeze.** Base `feefe3e060a851fd8860e4bacb90c11c7b284888` (user commit "plan"). That commit
  reproduces the frozen S digest `33ae9028…4552` against base 4b54485, with added files hashed as the
  untracked set. Snapshot digest `03ca0bea615bfc261ce6191b60147d4df20f9bab8115d0ec5b2dbebbaa40cbae`;
  changed files `src/adapters/persistence/AGENTS.md` and `src/main.rs`.
- **Checks.** All logs are under `logs/S/`; host state is in `s-final-status.log`.

| Check | Result | Log |
| --- | --- | --- |
| Strict Clippy (`--locked --offline --all-targets -D warnings`) | PASS | `s-final-clippy.log` |
| `cargo test --no-run` | PASS | `s-final-no-run.log` |
| Network-isolated full suite | **PASS**: 2665 + 5 passed, 0 failed, 22 ignored. Start 08:09:18Z (load 2.83, memory free 61%); end 08:16:53Z (load 5.90, memory free 72%); swap 4.2 GiB sticky | `s-final-full-suite.log` |
| Stack budget (2 MiB), run 1 | **FAIL** (exit 101, "unrelated to the stack budget"): 2663 passed, 2 failed — `workflow_message_publish_competing_dispatchers_publish_once` (`message.unresolved`), `workflow_message_recovery_control_retry_replays_the_failed_receipt` (`lease.claim` persistence bound elapsed) | `s-final-stack-budget.log` |
| Stack budget, rerun 08:25–08:32Z (load 2.76 → 3.64) | **FAIL** (exit 101, unrelated): 2662 passed, 3 *different* failures — `…siblings_128_and_129_…` (`action.commit_remote` bound elapsed), `workflow_message_race_delayed_commit_past_fence_expiry_or_deadline_rolls_back`, `workflow_message_sql_evidence_attribution_shape_rejects_forgeries` (`message.unresolved`) | `s-final-stack-budget-rerun.log` |
| All 5 stack-run failures in isolation at `RUST_MIN_STACK=2097152` | PASS (5/5) | `s-final-stack-isolated.log` |

- **Stack budget conclusion.** No stack overflow occurred in either run; the script itself classifies
  both failures as unrelated. The failures fall in the known persistence-bound/lease timing family,
  differ between runs, and pass in isolation at 2 MiB. The stack-budget gate is **not green** and is
  not claimed. The full suite is green.

### Phase 05 — investigation and D1 — 2026-10-18 (A)

Docs only: no source, migration or test edits, and no cargo was run for these. The base is the tree
above.

| Deliverable | SHA-256 |
| --- | --- |
| `logs/05/investigation.md` | `484af70eab2fcdcf0be924aecb68d1f85263dfec39a103a8f1c08de90187975e` |
| `logs/05/decision-D1.md` | `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931` |

D1 proposes:

- **Batches.** Two: 05A (the decision core, member reviewers, comments, timeout/cancel, and the
  run-scoped event inbox) and 05B (external credentials, decision notices, action approval, and the
  phase 05 gate). The split sits at the stable decision model and settlement contract.
- **Questions for B and root.** Eight open questions in D1 §16.
- **Plan-text amendments** for explicit-only reviewers are in investigation §6.
- **`NO_BACKWARD_COMPATIBILITY_NOTES`** is covered in investigation §7 and D1 §15. A
  `BRIEF-05.md`, if created, must carry the note and be listed. The phase plans `05-…` to `10-…`
  already carry the note but are not listed.

The D1 gate awaits B.

### Phase 05 — user answers to D1 §16 (2026-10-18, direct to root)

- Q1 reviewers: explicit list of 1–16, first valid authorized response wins.
- Q2 timeout: Terminal `decision.timed_out` over the declared `error` route; no route → run fails.
- Q6 external reviewers: **none in phase 05** — external email-only reviewer credentials
  deferred (user scope reduction). Plan text/acceptance line "external decision
  credentials" to be amended accordingly.
- Q8 production wiring of deliverer/sweeper/producer stays in phase 08.
- Q3/4/5/7 (technical) left to A's recommendation subject to B's judgement.

Batch S: full suite green on `03ca0bea…cbae` (base feefe3e); stack-budget runs exit 101 on
timing-bound tests only (no overflow); root proposes reusing the S-gate stack pass (delta
is one string literal + docs) subject to B S-R003. B also running 05-R001 on D1
`4e3a46eb…7931`.

- User (2026-10-18, direct to root): add plans 05–10 to `NO_BACKWARD_COMPATIBILITY_NOTES`
  in `src/main.rs`. Deferred to the start of 05A implementation so B's in-flight S-R003
  digest `03ca0bea…cbae` stays valid; A owns the edit (list entries only).

### Batch S ROOT ACCEPTED — 2026-10-18

B S-R003 PASS for digest `03ca0bea615bfc261ce6191b60147d4df20f9bab8115d0ec5b2dbebbaa40cbae`
(base `feefe3e060a851fd8860e4bacb90c11c7b284888`); S-R001/S-R002 covered the squash and
equivalence proof (S-F001/S-F002 resolved). Evidence on this tree: strict Clippy, no-run,
fmt/diff PASS; network-isolated full suite 2665+5/0/22 ignored PASS. Stack budget: the
command exited 101 in both runs (timing-bound tests only); every lib test completed at
2 MiB in at least one run with zero overflows, and all 5 failures pass alone at 2 MiB —
root accepts the stack requirement on that current-tree evidence (correcting root's
earlier "one string literal" premise per B) and discloses the non-zero exit as a gap.
Host timing flakiness of the lease/dispatch/reconciliation family is a recorded open risk.
Optional S-R003 nit: warning could say "recorded migrations or checksum".
**Batch S ACCEPTED.**

- 2026-10-18: B 05-R001 — CHANGES REQUIRED for D1 `4e3a46eb…7931`: medium 05-F001 (timeout
  indistinguishable on error route), 05-F002 (drop all external/credential design; legacy
  email-link approval dependency in REPLACEMENT-MAP:72 needs user decision), 05-F003
  (notice transport → delivery engine; audit unique-index defect); low 05-F004…F007.

- User (2026-10-18, direct to root): email-link approvals are **dropped with the legacy
  approval code** — approvals member-only; phase 08 removes legacy approvals with no link
  replacement; REPLACEMENT-MAP §4 gate to be amended. A assigned D2 for 05-F001…F007.

### Phase 05 — D2 correction — 2026-10-18 (A)

Docs only; no source edits and no cargo. D2 supersedes D1 as an explicit delta (§1 gives
every D1 section's status).

| Deliverable | SHA-256 |
| --- | --- |
| `logs/05/decision-D2.md` | `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3` |

- **User decision (2026-10-18, direct to root).** Email-link approvals are dropped with the legacy
  approval code. Approvals are member-only. Phase 08 removes legacy approvals with no link
  replacement, so non-member `approver_email` approvals stop working. Recorded in D2 §2. The
  REPLACEMENT-MAP §4 amendments are rows 72 and 74, and NOT_PLANNED gains a new row (D2 §15).
- **Dispositions** (D2 §0; all of 05-F001…F007 fixed):
  - **F001:** choice (a) — `decision.reviewer_unavailable` fails the run, unrouted. The error route
    carries `null`. The code is durable as the audit event `decision.timed_out`. The phase 07
    obligation is recorded, with two tests.
  - **F002:** all external and credential design removed with no seams. The reviewer item is
    `{kind:"user", id}`. The complete plan-text list is D2 §15.
  - **F003:** notices are standalone `notification` deliveries enqueued in the 05A park
    transaction. The notice run event is dropped, and the reviewer row links to its delivery.
  - **F004:** `companies FOR SHARE`, the claim corrected, a two-runs test.
  - **F005:** an advisory-lock-exact bound, run-scope semantics stated, competing-producers and
    stale-event tests.
  - **F006:** the 05B addendum is D2 §16, gated with D2. Approvers are owner/admin, rechecked at
    submit.
  - **F007:** all five clarifications.
- **Batches.** 05A now also carries the member notices. 05B is the dispatch-seam batch (action
  approval and per-reason wait uniqueness) plus the phase 05 gate.
- **Status.** D2 awaits B recheck by finding ID.

- Root (2026-10-18): D2 `83189027…15b3` sent to B for recheck 05-R002 (incl. §16 05B addendum).

### Phase 05 — D3 correction — 2026-10-18 (A)

D3 is a narrow delta to D2: it replaces D2 §16.2, the `workflow_decision_reviewers` block in D2
§5.1, D2 §7.1 steps 5–6 and one D2 §10 bullet, and appends to D2 §16.1. Docs only; no source
edits and no cargo.

| Deliverable | SHA-256 |
| --- | --- |
| `logs/05/decision-D3.md` | `0424a6e8b761e9c74668202f034ebfe5054d5cb4f6d09c908e29d444d60d2df7` |

- **05-F008 (medium), fixed.**
  - The parked attempt is consumed at reopen (`retry_count + 1`). The park close is unchanged,
    the same shape as the accepted effect park.
  - Park precondition: `retry_count + 1 < max_retries`. Otherwise the call fails Terminal
    `approval_required` and nothing parks.
  - New deferred branch in `check_workflow_fenced_completion`: every workflow
    `completed → pending` is refused unless it is the approval reopen. That requires the
    `approval_parked` attempt, `+1` accounting with an unchanged bound and ids, an open
    execution, the run `running`, and `approve` submitted in the same transaction (new
    `workflow_decisions.submitted_xact = pg_current_xact_id()`).
  - Three tests.
- **05-F009 (low), fixed.**
  - `UNIQUE (notice_delivery_id)`; the delivery is inserted first; the column is `NOT NULL`
    with `NO ACTION`; the reviewer row is fully immutable.
  - A BEFORE INSERT provenance trigger binds purpose, standalone shape and the key prefix
    (decision and principal) to the tenant-scoped decision.
  - A transaction-scoped DB test rejects another decision's, another company's or another
    principal's delivery, and a non-notification delivery.
- **Composer observation, classified terminal.** Composition runs before any write, and a
  `BadRequest` settles `decision.reviewer_unavailable` (run fails, unrouted). Tested.
- **05B observation.** The approval park with 16 approvers is in the lease-budget coverage, with a
  bound test.
- **Self-approval by the run actor.** A placeholder is in D3 §1, awaiting the user's answer via
  root.
- **Status.** D3 awaits B recheck of 05-F008 and 05-F009.

- User (2026-10-18, direct to root): self-approval **allowed** — any listed owner/admin,
  including the run actor, may approve a protected action. A fills D3 §1.

- **User decision (2026-10-18, given directly to root): self-approval is allowed.** Any listed
  owner or admin may approve, including the run actor. D3 §1 now records it in place of the
  placeholder; the rules in D2 §16.1 are unchanged. Test:
  `workflow_action_approval_run_actor_may_approve` (the actor approves, the action dispatches
  once; demoted before the submit, the actor gets `NotFound`).
- **D3 re-hashed after this edit:** `logs/05/decision-D3.md` `bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69`. This supersedes the hash
  recorded above.

### Phase 05 — D4 correction — 2026-10-18 (A)

D4 is a small delta to D3, fixing 05-F010 with root's option (a). Docs only; no source edits and no
cargo.

| Deliverable | SHA-256 |
| --- | --- |
| `logs/05/decision-D4.md` | `e8ed88c46e1420e3ededa3da33b282a8e6c9cefc9495e3146a420ed77fe1fc86` |

- **The provable-key check.** After composing each notice and before any write, the park checks the
  expected unhashed key prefix (`notification:workflow-decision:{decision}:principal:{principal}:`).
  If it is missing, the park settles `ReviewerUnavailable`: the run fails, the failure is never
  routed, nothing is parked and nothing is retried. The provenance trigger remains as the backstop.
- **Correction.** D3 §3.1's "bounded address" sentence is corrected: member addresses are
  unbounded, and a key is hashed for an address over 391 bytes.
- **Test.** `workflow_decision_park_unprovable_notice_key_fails_run_unrouted`, plus a 05B twin.
- **Not adopted.** A separate `decision.notice_unavailable` code: it gives the same outcome, and
  the existing code already covers "the reviewer cannot be reached".
- **Out of scope.** Option (b), bounding `users.email`.
- **Status.** D4 awaits B recheck of 05-F010.

### Phase 05 architecture gate ACCEPTED — 2026-10-18

B 05-R004 PASS for effective decision D1 `4e3a46eb…7931` + D2 `83189027…15b3` + D3
`bd937b86…0b69` + D4 `e8ed88c4…fc86`; 05-F001…F010 resolved. Root accepts the gate at
decision level. Deferred outside phase 05: phase07 timeout-code mapping (D2 §4.3), phase08
routes/production wiring, bounding member email (option b). Next: A implements 05A (first
edit: plan-text amendments + plans 05–10 in NO_BACKWARD_COMPATIBILITY_NOTES), focused
checks, freeze, B whole-batch review; then 05B; then phase05 gate.

### 05A implementation — 2026-10-18 (A, implement-opus-reviewed-2.1)

Effective decision: D1+D2+D3+D4, gate accepted by 05-R004. Base: HEAD `feefe3e` plus the
user-approved AGENTS.md/main.rs edits (S digest `03ca0bea…`).

- Step 1 (done): the D2 §15 plan-text amendments (plan 05 lines 18, 63-65, 68-69 and the
  acceptance line; EXPANSION 05.3; NOT_PLANNED line 17 plus a new row; REPLACEMENT-MAP rows 72
  and 74); plans 05–10 in `NO_BACKWARD_COMPATIBILITY_NOTES`; the S-R003 wording nit in `serve()`
  ("recorded migrations or checksum").
- Progress log: each later step is appended below as it lands.

#### 05A USER STOP — 2026-10-18 (A)

Stopped at a safe checkpoint, on the user's instruction relayed by root. The tree **compiles**:
`cargo check --locked --offline --all-targets` passes with one warning (unused
`DecisionNotices::unrenderable`, a test-only hook not yet used by a test). No cargo, rustc or
test process is running. The PostgreSQL fixture was stopped with
`pg_ctl -D /private/tmp/workflow0410-20261017 -m fast stop` after verifying system id
7697669855432439122; the data is kept. The init schema was edited in place, so before the next
run, reset the fixture's `workflow0410_test` and `workflow0410_fresh_test`.

**First edit (D2 §15): every item done.**
- Plan 05 lines 18, 63-65, 68-69 and acceptance line 89.
- EXPANSION 05.3.
- NOT_PLANNED line 17, plus the new email-only row.
- REPLACEMENT-MAP row 72 (owner column and gate) and row 74 (both phrases).
- `NO_BACKWARD_COMPATIBILITY_NOTES` gains plans 05–10.
- The S-R003 `serve()` wording nit ("recorded migrations or checksum").

**Implemented (compiles; mostly untested):**
- **Domain.** `Binding::references()`, a recursive walk (`domain/workflow/context/mod.rs`).
- **Registry and language.**
  - `reviewers` replaces `reviewer`: 1–16 unique `{"kind":"user","id":<uuid>}` items, `MAX_REVIEWERS`.
  - A static-root check that accepts only `params` references (`registry/mod.rs` `static_roots`).
  - The `wait.event` literal-name rule with one schema per name (`event_identity`).
  - Help text, the example, `CONTRACT_REVISION` bumped to `workflow-catalogue-v1.3`.
  - Fixtures: `reviewed-support.yaml`, `human-revision-round.yaml` (reviewer moved from `/input`
    to `/params`) and `repeated-human-revision.yaml`.
- **Schema (init, in place).**
  - New tables `workflow_decisions`, `workflow_decision_reviewers` and
    `workflow_decision_comments`, with keys, FKs, immutability triggers, and the reviewer guard
    (person principal plus the D3/D4 notice provenance prefix).
  - `UNIQUE (notice_delivery_id)`, `NOT NULL`, `NO ACTION`.
  - `workflow_wait_events` is run-scoped: `execution_id` dropped; `consumed_execution_id` added
    with its FK, UNIQUE partial index, unconsumed index and FK to runs; the immutability function
    allows only the one consumption mark.
  - The waits consumed-event FK is run-scoped.
  - `check_workflow_wait_commit` is extended to `decision`, plus the consumed-event ownership
    check.
- **Application.**
  - `application/workflow/decisions.rs` (new): commands, views, `WorkflowDecisions` port,
    `specification`, `prepare_submission`, `hex_digest`, `DecisionNotices` (compose plus the
    unhashed-prefix check).
  - `waits.rs`: `WorkflowEvent`, `EventIngress`, `MAX_UNCONSUMED_EVENTS`, `record_event`.
  - `worker.rs`: an optional `notices` field; `decision.human` → `park_decision` →
    `resume_wait`.
- **Persistence.**
  - `persistence/workflow/decisions.rs` (new): park with resolution, composition and notice
    deliveries inserted in the transaction; `ReviewerUnavailable` and `InvalidInput`
    settlements; submit, comment and read under the §8 lock order; replay;
    `closed_at_commit` (23514 → Conflict).
  - `authority.rs`: `authorize_reviewer_on` (companies `FOR SHARE`).
  - `waits.rs`: `record_event_on` with the per-run advisory lock, run-scoped dedup, the exact
    bound, ingress payload validation and `Unmatched`.
  - `wait_commit.rs`: run-scoped consumption, `decision_timed_out` (audit `decision.timed_out`,
    `route_parked_failure`), `saved` and `saved_route` for every committed route, `fail` made
    `pub(super)`.
  - `polling.rs`: the event clause is run-scoped and limited to event waits.
  - `pending_recovery.rs`: the `ReviewerUnavailable` variant, and `decision.human` in the
    known-no-effect list.
  - `recovery.rs`: `route_effect_failure` renamed to `route_parked_failure`.
- **Tests migrated.** All event-test call sites now use the run-scoped port through the new
  `recorded()` helper in `workflow/tests.rs`; `wait_tests` covers the scope parts as
  `Unmatched`. `WorkflowWorker` literals gain `notices: None`.

**Checks run:**
- `cargo check --locked --offline --all-targets`: PASS, one warning.
- `cargo test --lib -- registry:: compiler:: fixtures::`: 69 passed, **1 failed**
  (`registry::runtime_tests::workflow_registry_each_type_validates_dynamic_required_inputs`).
  It binds the old `reviewer` field to `/input/value`. The test needs updating for the new
  `reviewers` field and the static-root rule. Log: `logs/05/05a-focused-language.log`.
- Nothing else has run: no DB-backed tests, fmt, Clippy, schema or SQLx gates.

**Not started:**
- Every new 05A test named by D1–D4: decision park/submit/comment/timeout/cancel/notices/auth/
  stale/invalid/restart, the race tests, bounds, the provenance and immutability SQL tests,
  two-runs-one-company, composition error, unprovable key, past deadline, reviewer unavailable,
  sixteen reviewers, competing producers, stale event, compile disclosure, static-root and
  event-name compile tests, and timeout routing with and without an error route.
- Updates to existing registry tests: `runtime_tests.rs:72`, `contract_tests.rs:55`, and any
  catalogue or revision pins.
- The full verification gates.

**Deviation (flag for root/B):**
- D2 §4.2's static-reviewer rule rejects `/input` references. The phase-07 placeholder fixture
  `human-revision-round.yaml` read its reviewer from `/input/reviewer`. I moved it to
  `/params/reviewer` with a parameter schema and dropped the parent's input mapping.
- **Phase 07 obligation:** a child round must receive its reviewers through its binding
  parameters, or through a 07 language change. No gated decision was changed.
- Minor: `CONTRACT_REVISION` bumped, because the catalogue contract for `decision.human` changed.

**Next steps:**
1. Restart the fixture: `pg_ctl -D /private/tmp/workflow0410-20261017 -l <log> start`, then
   drop and recreate `workflow0410_test` and `workflow0410_fresh_test` and run
   `cargo sqlx migrate run` (the init file was edited).
2. Fix `runtime_tests.rs:72`, `contract_tests.rs:55` and any pins; rerun
   `registry:: compiler:: fixtures::`.
3. Run the existing focused DB filters: `workflow_wait_`, `wait_guard`, `wait_deadline`,
   `control_wait_order`, `polling`, `fairness`, `workflow_message_`, `control_`,
   `adapters::persistence::workflow::tests`. Fix regressions.
4. Write the new 05A tests listed above (a new `decision_tests.rs` with test support from
   `message_test_support::member`), using `DecisionNotices::unrenderable` for the
   composition-error test.
5. Run the full gate list; record it under "05A implementation"; freeze; report.

- **WIP snapshot (unreviewed, not frozen for review):** base
  `feefe3e060a851fd8860e4bacb90c11c7b284888`, digest
  `c90f78c3733ccb02ffb7c66b474efc8cd335f1b05334f33abc368a863454f52d`.

### USER STOP — 2026-10-18 (root)

Stopped at the user's request after A's safe checkpoint ("05A USER STOP" above). Root
verified: WIP digest `c90f78c3…f52d` on base `feefe3e` recomputed and matching; fixture
stopped; no cargo/rustc/test/postgres processes. Handoff:
[HANDOFF-05A-USER-STOP-20261018](HANDOFF-05A-USER-STOP-20261018.md). Workers stopped (idle).
Further work requires a new user instruction.

### Phase 05 resumed — 2026-10-18 — implement-astra-reviewed-2.1

User renewed continuation and confirmed root as Astra/high author. Root owns authorship;
a fresh independent Astra/medium reviewer will start at the first review boundary.
Scope remains 05A, then 05B, then final phase05 gates; no commits, deployment or phase06+.
Preserve all inherited edits. Starting HEAD is feefe3e060a851fd8860e4bacb90c11c7b284888;
resume digest ec4d7426c256190fed2c601e8a9d17173ff75cc9af11f968d4027d85bc998846
(exact exclusion: plan/workflow-foundation/docs), different from historical stop digest;
no prior code acceptance is claimed. Reuse the task-local realpath wrapper documented above.
Host compaction threshold/occupancy unavailable; continue with host defaults and checkpoints.

Verified retained PostgreSQL system identifier 7697669855432439122; restarted fixture
/private/tmp/workflow0410-20261017, port55450. Reset workflow0410_test and
workflow0410_fresh_test only, preserving S evidence databases. Fresh test migration passed.
Root owns all fixture-mutating checks. Language check running in logs/05/resume-language.log.
Fixed inherited registry pins: dynamic decision input test now exercises deadline; static
reviewer shape test uses reviewers array and valid UUID with prohibited extra field.
Required checks and batch order remain D2 §14, amended D3/D4; current state unreviewed.

#### 05A resume implementation checkpoint

Root author changes: corrected registry pins; added decision lifecycle/replay/auth/comment/
notice/race tests and SQL provenance/bounds tests; added static reviewer/event-name compile
checks and run-inbox competing-producer/stale-correlation tests. Updated inherited event
fixtures/assertions for consumed_execution_id. Corrected decision lock order to put
association before execution/job. Wait lock now permits unleased failed jobs so a timeout
can replay its persisted outcome. Consolidated canonicalization with actions, retaining
action bounds and using domain context bounds for decision artifacts; normalized JSON
numbers and bounded fingerprint input before cloning. These remain unreviewed.

Evidence:
- resume-language.log: 70 passed.
- resume-regression.log: 699 passed / 6 failed. Five old event-test expectations corrected;
  reconciliation control_serialization_expire_before_command failed a drain assertion.
- decision-first/second logs: test compile corrections, preserved.
- decision-third.log: 31 passed / 2 failed; reconciliation drain test passed in this focused
  run; timeout replay and bound expectation corrected.
- decision-fourth.log: 58 passed / 2 test-fixture failures (user vs principal id; fault
  injection expected wrong error class); corrected.
- decision-fifth.log: 59 passed / 0 failed (decision, registry, wait, shared action tests).
- Added decision_lifecycle_tests.rs after fifth run: past deadline/unavailable reviewer,
  crossing-deadline rollback and two-runs-one-company concurrency; not run yet.
No reviewer spawned or 05A freeze yet. Fixture remains running. Next: finish remaining
05A acceptance coverage, focused/static/schema gates, then freeze and independent B review;
05B has not started. Scope and exact snapshot exclusion unchanged.

#### 05A frozen for independent batch review — implement-astra-reviewed-2.1

Base feefe3e060a851fd8860e4bacb90c11c7b284888, digest
e2a59ac010a2b8c819f88a490729a21d97e9dde868ec4a8681ae54277a3e053a;
exact exclusion `plan/workflow-foundation/docs`. Manifest logs/05/05a-review-snapshot.txt.
Root author (user-confirmed Astra/high); fresh B Astra/medium independent context.
No project edits while B reviews. Prior S AGENTS changes are preserved; reviewer scope
is all 05A source/schema/tests/plan changes since base, including inherited unreviewed WIP.
D1–D4 accepted gate remains authoritative, with /params child fixture deviation flagged.

Latest focused decision-sixth.log: 17/0 including crossing-deadline rollback and two runs
one company. New notice atomicity/privacy/dead-letter/late-delivery, disclosure compiler and
recursive binding walk tests are included in the ongoing batch gate.
05a-gates.sh has passed fmt/diff/transport/credential-deploy/offline/strict Clippy,
fresh-schema dump equality, sqlx prepare and --check (no .sqlx diff). No-run/workflow
and delivery regression suites are in progress. Root owns fixture, serial gate process.
Acceptance is pending B PASS and all batch checks; full suite/stack/lease gate follows 05B.

Reviewer handle: /root/reviewer_05 (Astra/medium, fresh context). All-target no-run passed;
network-isolated 944-test workflow/application/domain batch suite running. B owns REVIEW,
review-notes and findings; root owns TASK and serialized check resources. No edits since freeze.

#### 05A R005 corrections and reopened architecture gate

05-R005 CHANGES REQUIRED, F011–F013. Source unfrozen after completed review. Gate
05a-gate-workflow-suite.log: 937/7; five reconciliation persistence/lease timeouts,
message failed-wait replay expectation, and whole-object disclosure compile assertion.
Delivery regression and graft passed. Corrected replay expectation to saved final error;
resolved-input disclosure rejection test passes (stage amendment pending D5).
Correction-first.log: 23/4. Actual membership removal is blocked by new principal FKs;
visibility fixture used absent slug column; feedback boundary exceeded default context.
D5 reopens architecture for local historical identity snapshots and disclosure-stage
amendment. No dependent schema edits before reviewer PASS. Other corrections in progress.
Disk exhaustion avoided by cargo clean -p mail_agents (regenerable package artifacts,
50.7GiB reported removed); use CARGO_INCREMENTAL=0 for subsequent builds. Evidence and
PostgreSQL data preserved. Reviewer reused /root/reviewer_05 for D5.

05-R006 D5 gate CHANGES REQUIRED, F014 actual remove_member lifecycle. D6 amends D5
with external/NULL conversion, FOR SHARE snapshot checks, bounded contention semantics
(no claimed global no-cycle proof), actual-port and raw-delete history/race coverage.
Dependent schema edits still held pending D5+D6 PASS.

Root accepts architecture gate 05-R007 PASS for D5 ec061ebe…cdb + D6 93aabee9…c80;
F014 architecture resolved. Implementing local historical identity snapshots and tests.
No batch acceptance or 05B implementation yet.

#### D5+D6 implementation checkpoint

Removed the three ephemeral-principal FKs only; added write-time tenant/person snapshot
checks with FOR SHARE, immutable reviewer user_id populated by the guard, and read
projection from that snapshot. Park eligibility locks member/principal. Fresh test DB
reset/migration passed (d5-schema-reset.log). correction-second.log: 34 passed / 1 failed
(channel test attempted immutable binding association update); fixture now creates a
new binding. Other passing coverage includes actual remove_member preserving open and
settled history, actual removal waiting on submitted/parked authority, conversion-first
refusal, raw delete membership races, cross-tenant snapshot/decision/wait guards,
notice unique/provenance isolation, character/byte feedback bounds and context limits.
Added explicit message-input mapping positive and invalid whole-object activation proof,
raw-delete settled history and in-flight park/remove races; fourth focused run includes
these plus original reconciliation timeout failures. Earlier third compile diagnostic
(PreparedInputs.value accessor) corrected; all logs retained. No correction review
freeze yet; no 05B work. Known graft tally at this checkpoint >=663047 tokens saved
across the continued task, excluding one truncated early lookup's unavailable tally.

#### Corrected 05A artifact frozen for recheck

Focused correction-fourth.log passed 44/0, including all five original reconciliation
timeout failures, the new disclosure activation/mapping cases, raw-deletion settled
history, in-flight park removal, actual-port removal and all F011–F013 checks.
Freeze base feefe3e060a851fd8860e4bacb90c11c7b284888; digest
2a428cd707ec4427ddb31ca9ea60d875ba829c1bb0c4c743ab7f38929d99bef3;
exact exclusion plan/workflow-foundation/docs, unchanged. Manifest
logs/05/05a-correction-snapshot.txt. No source edits during correction review.
B /root/reviewer_05 rechecks F011–F013 and D5+D6 implementation plus affected integration.
Root owns serialized broader correction gates. Independent strict clippy is running;
05a-correction-gates.sh reruns static/schema/SQLx/no-run/workflow/delivery/graph checks.
05B remains unstarted until matching PASS and required batch checks pass.

#### 05A accepted — implement-astra-reviewed-2.1

Root accepts 05A on base feefe3e060a851fd8860e4bacb90c11c7b284888, digest
2a428cd707ec4427ddb31ca9ea60d875ba829c1bb0c4c743ab7f38929d99bef3, exact exclusion
plan/workflow-foundation/docs. Root recomputed matching digest after all gates
(logs/05/05a-acceptance-snapshot.txt). Independent 05-R008 CODE PASS matches; F011–F013
resolved, D5+D6/05-R007 accepted and F014 architecture resolved; no open batch blocker.

Evidence: correction-fourth.log 44/0; correction-clippy.log strict all-target PASS;
05a-correction-gate-status.log all PASS (fmt, diff, transport, credential/deploy, locked
offline all targets, fresh-schema dump equality, SQLx prepare/check with no .sqlx diff,
all-target no-run, network-isolated workflow/application/domain suite 961/0, delivery
regressions 47/0, graft build). The earlier 937/7 gate and all failed correction logs
remain retained; no baseline claim is made about the earlier timeout failures.

05B may now begin under D1 §9, D2 §16, D3 §2–4 and D5/D6 history invariants. Root remains
Astra/high author, B /root/reviewer_05 independent Astra/medium. Read-only preparation is
logs/05/05b-investigation.md. Final full suite, stock stack and lease budgets remain
required after 05B. PostgreSQL fixture running, root owns checks. No commits/deployments.
Known cumulative graft savings >=860241 tokens; priced recent calls report $0.10 plus
five < $0.01 entries (earlier calls supplied no dollar value).


#### 05B implementation in progress (continuation)

05A acceptance above remains historical. 05B is not frozen or independently reviewed.
Implemented approval schema/dispatch/settlement/wait selection and HTTP/MCP/message
suspension plumbing under D1–D6. Third all-target offline compile passed
(`logs/05/05b-compile-third.log`) after test-only explicit completed-outcome extraction
and exhaustive diagnostic matches. The task test database alone was reset; edited
schema migration passed (`05b-schema-reset.log`). Presquash/postsquash databases preserved.

Audit found arbitrary operation names cannot classify approval rejection/expiry;
classification now uses the restored frozen target resource kind (http/mcp/messaging).
New real HTTP and registered-tool tests are in progress (`05b-http-first.log`);
no 05B test pass or batch acceptance yet. Production runtime composition remains phase08;
tests directly compose the existing HttpActions with notice-enabled SQL dispatch.
Remaining: full 05B acceptance matrix, targeted regressions/lease budget, freeze and
independent review, final phase05 full verification. Root retains serialized checks.

05B continuation: fresh HTTP integration exposed two corrections: direct http.request
must strip workflow-only approval before provider argument validation; http_for_action's
last credential/access check must recognize exact persisted approval after current
checks. Corrected both. 05b-http-fourth.log was 6/1 (registry + refusal/bound/SQL guards
passed, resumed HTTP failed); diagnostics identified the provider-side check. New MCP
pre-open and message two-wait tests are compiling/debugging; no 05B acceptance.

D7 architecture amendment submitted to retained B /root/reviewer_05: decision-D7.md
SHA256 d980ffa58aea5eb8f58b7627e2ba4b68506a8912b47f6dc2db72ec30a13ebcf9.
D1's provider-replay predicate does not cover the alternative not-applied proof path;
D7 specifies one remote entry per approval, DB guard and consumed-approval refusal.
Dependent replay correction held pending PASS. Reviewer owns only review records/logs,
read-only inspection; root retains serialized DB checks. Other 05B work proceeds.

Root accepts D7 architecture 05-R009 PASS at the recorded SHA256; implementing the
consumed-approval reservation refusal, marker non-reuse, and deferred second-entry guard.
05b-focused-third.log: 10 passed / 2 failed (MCP fixture named wrong step; message passed
workflow approval into action arguments). Fixed fixture step and message input stripping.
The passing checks include real approved HTTP + registered-tool competing dispatch,
self-approval/replay/supervisor history, 16 approvers, expiry/reject, admin demotion,
reviewer competition, SQL reopen refusal and registry matrix. D7 not covered by that run.

05b-focused-sixth.log passed 18/0: HTTP/tool and MCP positive dispatch, one effect under
competition, exact digest refusal/FK matrix, D7 genuine not-applied + raw SQL refusal,
message two-wait history, local/remote/message lost-ack applied/aborted resolution,
expiry/reject/demotion/last-attempt/reviewer races, registry reviewer/deadline matrix.
The final business-submission helper extraction and test-only tuple cleanup followed
this run; the upcoming 05B broader gate covers them. reserve_action extraction was
covered by the sixth run. No raised resource limits. lease-budget default filters now
include workflow_action_approval_ (deliberate-delay tests mark Injected).

05B gate script logs/05/05b-gates.sh runs static checks, locked offline all targets,
strict Clippy, fresh-schema equality, SQLx prepare/check, all-target no-run, isolated
workflow/application/domain and delivery regressions, lease budget and graph refresh.
Root owns the serialized checks. After all pass: freeze whole 05B and request independent
code review, resolve all findings, accept, then final phase05 full/stock verification
and combined-integration review. No source freeze or 05B acceptance yet.


#### USER STOP — 05B safe checkpoint, 2026-10-18

User requested handoff and stop. See [handoff](HANDOFF-05B-USER-STOP-20261018.md).
05A remains historically accepted (05-R008); D7 accepted (05-R009). 05B has no
independent code review or acceptance. Focused18/0 predates final submit_business_on
extraction and test tuple cleanup. Subsequent gate PASS: fmt/diff/transport,
credential-deploy, locked offline all-target compile, strict Clippy, fresh schema,
SQLx prepare. SQLx check interrupted at stop; no-run/workflow/delivery/lease/graph
and final full/stock verification remain outstanding. Evidence retained in logs/05.
Gate process tree terminated; task PG stopped cleanly, all data retained. Reviewer
completed/idle. No commits/deployment/phase06+. Resume only on new user instruction.
WIP snapshot: logs/05/05b-user-stop-20261018-snapshot.txt; exact exclusion remains
plan/workflow-foundation/docs. Checkpoint only, not a reviewed/accepted artifact.

Stop checkpoint identity:
```text
base:   feefe3e060a851fd8860e4bacb90c11c7b284888
digest: 68b1c01bf2e2e9eb6ae5a21f8d712e9129a8be32d99d0be39b9f8b789a3a2d4b
```


#### RESUMED — 05B, 2026-10-18

User authorized continuation under implement-astra-reviewed-2.1; prior root author
confirmation retained. Preserved stopped-check evidence; resumed gates start at
SQLx check, with new logs 05b-resumed-gate-*. Root owns task PG and serialized checks.
05B remains unaccepted pending all gates and independent whole-batch review.


05B resumed batch gates PASS: SQLx check, all-target no-run, network-isolated workflow
979/0 and delivery47/0, lease115/0, graph refresh. Exact commands and logs:
logs/05/05b-resumed-gates.sh and 05b-resumed-gate-status.log. Prior static/offline/
Clippy/fresh-schema/SQLx-prepare passes remain applicable to unchanged production.
Author audit found the explicit approve-versus-expiry competing-claimant case missing;
added workflow_action_approval_submit_competes_with_expiry using the established
run-lock/deadline race pattern. Only test source changed. First focused compile stopped
to correct its new state-query join (background_tasks has workflow_execution_id, not
workflow_run_id); no result claimed. Second focused run: 05b-resumed-focused-second.log.

#### 05B frozen whole-batch review assignment

Artifact base feefe3e060a851fd8860e4bacb90c11c7b284888, digest
b42023ba2177a20937e6aa0c14b4abf6dc6e41fcbcf2a89a6fa711370f6fd630;
exact sole exclusion plan/workflow-foundation/docs. Manifest logs/05/05b-review-snapshot.txt.
Independent retained B /root/reviewer_05 assigned whole 05B actual code/criteria/callers
review under D1–D7. Source frozen; root retains DB check ownership, no checks running.
B may inspect read-only; coordinate any DB-mutating checks. B owns only review records.
05B implemented/verified at batch scope but not independently code-reviewed or accepted.

Latest test delta: approval-focused17/0 including submit-vs-expiry; fmt/diff/strict
Clippy/SQLx prepare/check/graft all PASS (05b-race-gate-status.log). Earlier production
regressions workflow979/0, delivery47/0 and lease115/0 remain applicable; new test alone
followed them. Full phase05 all-target/stock checks remain after batch acceptance.
Final script prepared at logs/05/phase05-final-gates.sh, not yet run.


05-R010 CHANGES REQUIRED, one low verification finding 05-F015; no implementation
bypass identified. Source unfrozen for test-only corrections: direct SQL reopen matrix
(prior-transaction approval, reject, wrong retry count, valid positive control), exact
guard-message assertion on existing no-submission case, and long-address approval notice
refusal with no waits/decisions/reviewers/deliveries/dispatch/entry/receipt or provider call.
Focused run logs/05/05b-f015-focused.log. Production and architecture unchanged.

05-F015 correction frozen for one recheck. Focused approval19/0 and fmt/diff/strict
Clippy/SQLx prepare/check/graft PASS; logs/05/05b-f015-focused.log and
05b-f015-gate-status.log. Production unchanged; prior broad evidence reused.
base:   feefe3e060a851fd8860e4bacb90c11c7b284888
digest: ac6e12f99bf53762a9056a6fcea62ac4a90513d4315dc47e5e3b087d21d4a84a
Exact sole exclusion plan/workflow-foundation/docs. Retained B assigned delta and
affected criteria by 05-F015; root owns PG, no tests running.


#### 05B ROOT ACCEPTED — 2026-10-18

05-R011 independent PASS matches root recomputation: base feefe3e060a851fd8860e4bacb90c11c7b284888,
digest ac6e12f99bf53762a9056a6fcea62ac4a90513d4315dc47e5e3b087d21d4a84a;
sole exclusion plan/workflow-foundation/docs. 05-F015 resolved; no open findings.
Review records and disposition persisted. Whole 05B coverage 05-R010 plus correction
05-R011, focused19/0 and static/SQLx passes, unchanged production broad979/0,
delivery47/0 and lease115/0. No source edits after freeze.

Starting final phase05 gate logs/05/phase05-final-gates.sh. Root owns serialized PG
checks; retained reviewer idle until final combined-integration evidence review.
Final full/stock verification pending; phase05 not yet declared complete.

Final gate first full-suite result:2726 passed/1 failed/22 ignored. Failure is
workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry,
action_dispatch_remote_tests.rs:622. Captured entry425ms/deadline389ms; dispatch
correctly returned Interrupted before provider polling. Test's LeaseFirst premise
sleeps2s inside the unchanged3s lease before reservation/entry; measured real windows
were run0.865s/lease0.361s. This is a fixture timing failure, not a waived baseline
claim. All other library tests passed; cargo stopped before other targets. First
stock gate still running; no source change yet. Planned correction keeps the fresh
lease and sets the negative run deadline after the persisted lease, retaining4s
watchdog, real provider polling/drop, and persisted-window causal assertions. No
production bound/protocol changes. Root will rerun affected tests and final proof,
then obtain independent delta/combined-integration review before phase completion.


First final gate complete: stock script2727/0/22ignored at2048KiB, lease118/0
(see log for authoritative count), graph PASS; full suite failed as recorded above.
Test-only correction applied to action_dispatch_remote_tests.rs after gate completion:
prepare_pending_window keeps fresh lease, LeaseFirst puts run deadline after the persisted
lease by existing RUN_MARGIN. Removed the negative control's2s pre-dispatch sleep.
Causal oracle still checks genuine persisted windows; negative control now asserts the
specific lease-first rejection, while real provider poll/drop and4s watchdog remain.
No runtime/resource bounds raised. Caller trace covers pending_case and both tests.
Focused filter workflow_action_dispatch_pending_; then corrected final full all-target
suite at RUST_MIN_STACK=2097152 jointly proves full+stock on corrected source. This
supersedes the first full failure and supplements its unchanged-production stock PASS.
Static/SQLx and lease rechecks included; unchanged schema/migration/transport evidence
reused. Final independent delta/combined-integration review required before completion.


#### Final phase05 verification PASS; final review assignment

Corrected gate logs/05/phase05-corrected-gate-status.log: fmt/diff/offline/strict
Clippy/SQLx prepare/check/no-run/full-stock-suite/lease/graph all PASS. Full all-target
suite under network isolation with RUST_MIN_STACK=2097152:2727 library+5 binary
passed,0 failed,22 existing ignored. Lease118/0, maximum measured code-owned work
35.7% of its bound (budget.reserve357ms/1000ms; threshold50%);106 injected events
excluded under existing policy. Fresh migration/schema equality, transport and
credential/deploy script passes reused from the first final gate: no production or
schema edits since. No resource limits raised. Initial full failure, focused compiler
error and every interrupted/failed log retained; no baseline waiver.

Final code delta since 05-R011 is only action_dispatch_remote_tests.rs fixture setup
and causal assertions. Focused2/0 (final-deadline-focused-second.log); full/stock and
lease checks cover it. RESUME updated administratively with final verification.
Root assigns final delta and combined-integration evidence review to retained B.
Production composition/member HTTP routes remain phase08; child feedback propagation
and timeout-code mapping remain phase07. Frontend CI/browser steps, query-statistics
probe and stack-frames diagnostic not run; no corresponding changes or expanded
claims. PG still running for review; root owns it and no author checks are active.

Frozen final review identity:
```text
base:   feefe3e060a851fd8860e4bacb90c11c7b284888
digest: 59fde0cfbc2a70abdd01614b15dedc6d5929c4e40aeec74d66fb41120983edba
```
Exact sole exclusion plan/workflow-foundation/docs. Source frozen for B.


#### PHASE05 COMPLETE — ROOT FINAL ACCEPTANCE, 2026-10-18

Independent final delta/combined-integration PASS05-R012 matches root recomputation:
base feefe3e060a851fd8860e4bacb90c11c7b284888, digest
59fde0cfbc2a70abdd01614b15dedc6d5929c4e40aeec74d66fb41120983edba,
sole exclusion plan/workflow-foundation/docs. Root manifest:
logs/05/phase05-root-final-snapshot.txt. No open findings; F015 resolved under05-R011.
05A accepted under05-R008,05B under05-R010/R011, final fixture correction and combined
integration under05-R012. All notes/findings/dispositions persisted.

Completed authorized scope: phase05A+05B and final verification. Corrected full all-target
suite at2MiB passed2732 total (2727library+5binary),0fail,22existingignored. Lease118/0;
all required static, offline, migration/schema, SQLx, transport and graph checks PASS,
with the evidence reuse and first failed run documented above. No runtime/resource
bounds raised. No commits, deployments, production resets or phase06+ implementation.
Production runtime/member-route wiring remains phase08; phase07 owns the recorded
child-feedback/timeout-code obligations. Other CI/diagnostic omissions are listed above.

Resources: task PostgreSQL `/private/tmp/workflow0410-20261017` (PG18,port55450,
role workflow0410,system identifier7697669855432439122) stopped cleanly via
pg_ctl -m fast -w stop after final review. All data retained, including
workflow0410_test/workflow0410_fresh_test and workflow0410_presquash/postsquash.
Verified no task gate or task PostgreSQL process remains. Reviewer /root/reviewer_05
completed/idle; no worker activity outstanding. Continue phase06 only on a new user
instruction. Preserve current worktree and all evidence.

This continuation's reported graft savings total169586 tokens; no dollar estimate
was supplied by those calls, so none is inferred.
