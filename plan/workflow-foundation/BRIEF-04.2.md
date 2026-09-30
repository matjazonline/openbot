# 04.2 — current action authorization

Status: foundation authorization observation VERIFIED by implementer/reviewer; root owns acceptance record.
Original04 Execution item1 remains authoritative.
04.1 foundation accepted; its intent, model-call mapping and applied migration are preserved.

Contract: shared ActionService authorization loads the saved invocation/digest and authoritative
run actor, validated frozen bundle and resource bindings through application ActionAuthorities.
SQL locks the run first and rechecks current company/principal membership. Pure checks require
the exact selected ToolSnapshot in the frozen run; caller ceiling can only narrow its capabilities,
never expand them. Intent actor must equal the admitted run actor. Connection identity must equal
the run's frozen resource slot; local targets cannot claim a connection slot.

Current ResourceDirectory and explicit trusted ActionPolicyDirectory checks require exact tenant,
resource identity/kind, readiness, tool support and compatible frozen operation/policy. Missing,
revoked or errored lookups reject. Current approval requirements may strengthen the frozen
requirement; neither produces human approval. No credentials enter these ports.

Returned access decision is an observation, never a reusable dispatch token.04.3 must repeat these
checks in its fenced dispatch transaction to close revocation races. Protected effects remain
undispatched pending05. Existing SQL MCP readiness lacks supported-contract authority; HTTP/local
owners and trusted current policy adapters join04.9/10 and remain fail closed here.06 owns direct
agent principal/capability selection (bounded by this run authority); no caller-supplied actor
substitution or legacy shim. Production shared dispatch/bypass removal remains04.3–11/06/08.

Acceptance: frozen actor/company/tool/policy/ceiling/slot rejection; bounded current policy and
resource facts; current revocation on repeated step/tool observation; policy/directory errors
propagate; stricter approval cannot weaken protection. Real SQL saved-subject scoping, run actor
provenance, current membership revocation and run-first serialization exercised. No new protocol
queue, migration, resource bound or state owner.

Worker01a0f1d4-e180-73e1-89fc-a6ab40fa407c Sol6.1/high, startup25460/2584009.85%
10:21:12Z; expansion79165/25840030.64%10:24:22Z. Reviewer
/root/actions_authorization/review04_2 UUID01a0f1d7-dde6-7561-b839-38318629b755
Astra/medium configured; startup28959/25840011.21%10:24:22Z. Sources usage/runtime capacity.
Root owns PROGRESS/RESUME; worker owns this brief/code/affected expansion/taskPG.
TaskPG RUNNING retained, both URLs postgres://mac03@127.0.0.1:55439/workflow_admission;
no reset. Exact verification/review evidence appended at checkpoint.

## Implementation and check evidence

Application actions/authorization.rs adds ActionAuthorities, authoritative run snapshot,
trusted current policy port, bounded5s directory/policy observations and pure ceiling/provenance/
connection/current-policy checks. Common ActionService.authorize serves every saved subject;
step/tool preparations preserve the04.1 identity. SQL action_authority.rs loads exact scoped
invocation/digest + validated bundle/workflow/version/run actor/resources, with run-first and
current company/principal locks. Deadline is checked again after lock waits. The transaction
commits before external directories run. No policy decision/intent mutation or dispatch exists.

Tests:11PASS,0FAIL,0IGNORE (7accepted04.1 +2new application +2new isolatedSQL), stock2MiB.
Exact command: `env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_`.
Log:/private/tmp/workflow-04.2-tests.log; TCP execution used required escalation.
Current access/lookup errors/schema-policy drift/approval preservation:
workflow_action_authorization_current_revocation_errors_policy_and_approval.
Frozen actor/ceiling/contract/target/tenant/run negatives:
workflow_action_authorization_frozen_provenance_ceiling_and_binding.
Real scoped subject/saved actor + current membership revoke:
workflow_action_authority_sql_scope_actor_and_current_membership.
Real competing company/principal revoker committed while authorization waits:
workflow_action_authority_revocation_contender_blocks_then_rechecks.
Initial compilation found missing Value and private RuntimeResourceId import; corrected.
Run schema uses frozen bundle+workflow/version identity (no nonexistent bundle_hash column).
Final static/SQLx/graph and independent actual-code reviews PASS (final evidence below).

Gate commands (sequential Cargo/SQLx): `cargo fmt --check`; `git diff --check`;
`git diff --cached --check`; `env SQLX_OFFLINE=true cargo check --locked --offline --all-targets`;
`env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`;
`env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx prepare -- --all-targets`;
same URLs `cargo sqlx prepare --check -- --all-targets`; `graft build`.
Offlinealltargets PASS:/private/tmp/workflow-04.2-check.log. Both whitespace checks PASS.
Reviewer inspected actual04.2 files only; parent edits were quiescent during inspection.
Worker intermediate112545/25840043.55%10:39:50Z usage/runtime sources.

Initial actual-code review PASS authorization-observation scope, no blocking findings;
reviewer73318/25840028.37%10:40:38Z usage/runtime sources. Three nonblocking direct-test
gaps grouped before final gates: valid connection slot + missing/mismatched frozen binding,
5s resource/policy timeout, after-lock deadline expiry. Added tests; final run PASS.
StrictClippy PASS before and after test additions. No runtime code changed after review.

Final targeted suite14PASS,0FAIL,0IGNORE at stock2MiB; exact command unchanged above.
Full log:/private/tmp/workflow-04.2-tests-final.log. Includes7accepted04.1 +7new04.2 tests
(4application and3isolatedSQL). Fresh per-test migrations exercised, no DB tests skipped.
Added direct regressions: workflow_action_authorization_exact_frozen_connection_slot,
workflow_action_authorization_lookup_timeouts_fail_closed,
workflow_action_authority_run_lock_wait_cannot_cross_deadline.
Correction review PASS, earlier runtime PASS retained, no unresolved findings; reviewerUUID
01a0f1d7-dde6-7561-b839-38318629b75578732/25840030.47%10:45:02Z usage/runtime sources.
Minor reviewer note:200ms deadline-test entry window can fail safely under severe scheduling delay;
no observed failure. Final sequential gate script:/private/tmp/workflow-04.2-gates.sh,
session17989 completed exit0. Logs:/private/tmp/workflow-04.2-{fmt,whitespace,check-final,clippy-final,
sqlx-prepare,sqlx-check,graft}.log. No production/combinedphase04/full-suite gate claimed.

Final gate completed session17989 exit0:fmt, staged+unstaged whitespace, lockedofflinealltargets,
strictClippy,SQLxprepare+check alltargets,graft ALLPASS. SQLx cache unchanged: runtime SQL was
exercised by real isolatedDB tests; no additive migration required. All applied migrations immutable.
Reviewer quiescent/completed (interrupt confirmation). No active checks/SQLx/approvals remain.
TaskPG remains RUNNING at retained identity for successor; ownership returns to root. No reset,
stage/unstage/commit/deploy or other-user edit changed. Root PROGRESS/RESUME preserved.
No04.3 begun; successor must repeat authorization under fenced dispatch ownership rather than
using this observation. Existing MCP contract authority and production policy adapters remain
04.9/10; frozen agent capability narrowing/direct-tool caller integration remains06/08.
Observed graft savings estimate~2,938,629tokens (whole-file/map baseline estimate, not runtime usage).
Worker final checkpoint123873/25840047.94%10:46:56Z usage/runtime capacity sources;
retiring conservatively before50%, no dependent assignment. Reviewer final parent-observed
79196/25840030.65%10:45:11Z token_count.info sources; subtree quiescent.
