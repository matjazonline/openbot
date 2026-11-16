# P10 independent review notes

## AS-P10-B1-N1 — whole-batch and combined-integration review — 2026-11-16

**CHANGES REQUIRED: AS-P10-B1-F1/P2.** No production runtime defect identified in
this round. Final evidence review also remains incomplete: root full runner and
current-source production image/topology execution are ongoing; no final acceptance.
See [findings](agent-review-findings.md), [TASK mapping](TASK.md),
[R1 manifest](logs/AS-P10-B1-R1.evidence.json).

### Identity

Workflow implement-astra-reviewed-2-3; phase p10; retained independent reviewer
/root/attribution_reviewer requested Astra/high. Actual model/effort/context telemetry
unavailable. No subagents or project source edits. Reviewer owns only p10 review
records/logs. Author remains user-confirmed root Sol6.1/xhigh.
Worktree `/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
HEAD/start/base `6e55258cbbdc7633940805f366fb17fe240f02aa`; source
`0c42f5f05564bd55bbef877a43380925536f135111e1e33dc8f195b815607fa6`.
Exact11 exclusions: `plan/attribution-service/phases/p01/docs` through `p10/docs`
individually plus `plan/attribution-service/COORDINATION.md`; nothing else excluded.
Main coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root revision27, C001–C008/r1 active. Main byte-equals immutable
[pin](logs/AS-P10-B1-coordination-r27.md). [Identity log](logs/AS-P10-B1-reviewer-identity.txt)
confirms start/end digest, HEAD and diff check. Accepted p09 baseline
542b32ae822314eb5a16050b153fa4e7a625cfc7380d2b66a9298d79fbdb5d9d / AS-P09-B1-N2
remains intact in main; source transfer has not happened.

### Requirements and coverage

Independently read original10A–C and current coordination, reused previously
independently reviewed original01–09 and accepted producer contracts. Read root/src/
HTTP and service AGENTS. Reviewed full p10delta and affected combined paths rather
than inferring requirements from TASK or passing logs. No new architecture decision
was required for mechanical runtime corrections and verification wiring.

- Audited all20 intended paths using15 archive/main preimages and5 new source files,
  independently checking final hashes. All168 unaffected p09 paths match accepted
  main bytes. Identified the additional absolute cache symlink (F1). Working-delta
  JSON was an earlier checkpoint; current transfer manifest and actual snapshot
  determined coverage. [Inventory](logs/AS-P10-B1-reviewer-inventory.txt).
- Read combined e2e fixture completely: actual setup/DNS/redirect and PostgreSQL,
  AdA thenAdB selection, three concurrent payment envelopes including same-event
  duplicate and alternate event ID, same business order, laterAdA non-reassignment,
  conflicting replay, competing business refund duplicates, remaining full refund,
  exact money/units and product reports. Assertions distinguish fact counts and
  immutable attribution rather than merely HTTP success. Reused accepted separate
  browser consent/tracking, analytics clock/currency/CSV and authorization cases.
- Checked the inherited session expiry fixture correction against its constraint:
  all three expiry values now use one statement_timestamp, while the actual expired
  session lookup assertion remains. Let-chain transformations preserve evaluation
  order and short-circuit behavior in service/root cookie parsing, CORS, public
  installation origin checks, admission cleanup and freshness. compose is moved
  unchanged before its test module; numeric separators are value-equivalent.
- Read full standalone verify/extracted scripts and all four new network guard/
  scope-proof files. Migration/metadata use an owned fresh DB; compilation/tests
  stay offline; full gate compiles tests, enforces egress denial, inventories nonzero
  tests, explicitly runs each of three ignored browser fixtures, checks contracts
  and JS syntax. Extracted service copies its own source and uses explicitly
  provisioned Playwright tooling; no parent runtime/source import introduced.
- Linux guard selects an invocation-owned temporary primary GID while preserving
  UID/environment/supplementary groups. Reviewed bounded sudo/native cleanup,
  intent-before-mutation tracking, signal handling, original-loopback PostgreSQL
  DNAT rule, preflight and status propagation. Scope test uses real sockets and
  concurrent actual guards, an extracted wrapper, nested descendants, supervisor
  access, distinct GIDs, environment/argv/status preservation and cleanup equality.
  The negative UID-selector variant distinguishes the control-plane failure.
  Inspected retained nonroot Linux execution; reviewer did not operate root-owned VM.
- Reconciled service CI with its runner and original10B. Root check.sh/check-support.sh
  are byte-identical to user commit6e55258c and cover frontend/deployment/boundary,
  migration/query-statistics, test inventories, actual browser fixture producers,
  stack/lease budgets and independent-service/image gates. Actual topology job is a
  separate required check. Read these execution paths and current CI; no existing
  gate removed or assertion weakened. Final runner outcomes remain pending.
- Read deliverable README/original10 additions against the actual extracted log:
  120 PASS/3 ignored in full unit/database suite, each ignored browser then1 PASS;
  contracts15 and boundary11; actual migration/metadata/fmt/offlinecheck/Clippy.
  Guard preflights confirm loopback PostgreSQL and external IPv4/IPv6 denial. No
  native platform or production policy claim inferred from this fixture evidence.

### Independent checks and retained evidence

Executed under the service-local macOS guard:
`cargo +1.95.0 test --locked --offline --lib attribution_e2e_ -- --nocapture`:
**1 PASS**, with IPv4/IPv6 egress denial and native PG18 maintenance endpoint64326.
Owned UUID fixture only, stock2MiB, /tmp/as-attribution-target;
[output](logs/AS-P10-B1-reviewer-e2e.txt). No reviewer process remains.
Inspected current extracted full gate and all3 actual browser outputs, Linux nonroot
scope proof and source_modules8 PASS. No duplicate full suite run by reviewer.
R1 evidence validator independently passed6 declared PASS/2 FAIL/2 NOT EXERCISED.
Validator retention identity is not behavioral truth; pending gates remain pending.

Root Clippy/Doop failure records inspected. Independently verified each listed
unchanged-owner SHA against HEAD/current source and both user's runner files against
HEAD. Listed Clippy diagnostics are syntactic/type-layout lints in unrelated owners;
owned attribution cookie lint is corrected in this artifact. Doop failure is actual
Python3.13 strict TLS missing-Authority-Key-Identifier in unchanged Doop fixture.
These are separate from service/attribution proof; fullroot remains non-green.
Historical markup/MCP failure audit hashes independently match HEAD/current; fullrun
assertions and final fail list are not yet complete, so their final dispositions
(and inbound crash/recovery uncertainty) remain for the completion supplement.
Do not treat an isolated diagnostic PASS as erasing a failing complete-suite result.

Current root production rebuild and final topology proof remain NOT EXERCISED in
R1 manifest. Earlier production topology1 predates the root cookie style correction;
it is not exact-current artifact proof. Required final gate outputs and failure
classifications must be reviewed before a final PASS. Root check/image resources
remain author-owned; reviewer did not stop or alter them. No complete plan, production
rollout, native-store pilot or full-suite success is claimed.

### Outcome and process

Whole p10 source review is complete apart from correction/recheck and final evidence.
One actionable artifact finding F1; no other implementation finding. This is an
appropriately cohesive batch, and the concurrency guard received real competing
process verification. Current full-gate execution is correctly disclosed as pending.
The snapshot inventory audit was necessary to discover a path omitted from the
transfer manifest; retain this lesson rather than broadly changing instructions.
Recheck can reuse unchanged source coverage and independent tests, inspect cache-link
removal/new identity, and independently disposition completed final gates. Preserve
all actual failures and outstanding/unavailable checks explicitly.


## AS-P10-B1-N2 — correction/final-evidence review in progress — 2026-11-16

**PENDING**, no final PASS. F1 CLOSED by verified source-ignore correction.
Identity: same role/worktree/requested Astra/high/actual telemetry unavailable,
HEAD/start/base6e55258c, exact11 exclusions, coordination27 C001–C008/r1 as N1.
R2 source `55ecc5eabdcb9493aa9c5ecead843cb075554aa686be101994280bb1b38efb97`;
[start snapshot](logs/AS-P10-B1-R2-reviewer-start.snapshot.txt) verified, main
coordination still byte-equal immutable pin. No source edits or new agents.

Correction review completed: [inventory](logs/AS-P10-B1-R2-reviewer-inventory.txt)
verifies all20 R1 source paths unchanged; only `/target` ignore change, three
TLS fixture additions (explicit strict verification, CA subject key identifier,
leaf subject+authority key identifiers), and one new learning. Prior topology
source reconstructed by reversing exactly those additions and byte-compared with
acceptedp09/R1. No verification disablement, deadline increase, runtime change or
weakened assertion. Temporary CA/private leaf remains invocation-local.
[Certificate diagnostic](logs/AS-P10-topology-certificate-diagnostic.txt) identifies
Python3.13 Missing Authority Key Identifier; corrected
[current topology3](logs/AS-P10-production-topologies-current-3.txt) proves both
full journeys with current release image identities3878bace/5930cc9c and unchanged
private DB/filesystem/plaintext/non-bridge403/untrusted-CA assertions. Inspected
current root build output. This supersedes prior debug/old-cookie image limits for
this source; reviewer did not independently rebuild or rerun Docker topologies.
R2 manifest validator passes11 declared PASS/6 historical FAIL/1 NOT EXERCISED.

Failure review in progress: actual fullroot lib4210PASS/20FAIL/27ignored read;
26 attribution tests pass, separate actual browser proof remains required by final
runner. Clippy6lib/7test errors remain in independently verified unchanged owners;
owned cookie lint is absent. Root Doop Python3.13 certificate failure is unrelated
and remains failed. Current markup failure points to unchanged sample.rs:153 and
matches historical pre-attribution evidence; MCP missing cancellation exchange
likewise has prior baseline failure evidence. Inbox fails at unchanged fixture
prewarm SSLRequest0x00, before business test assertions, and its isolated current
binary passes. These distinctions do not mean the root suite passes.

Sixteen Doop deadlines, provisioning persistence timeout and knowledge browser
held-navigation stall need final unrelated-scope disposition. Read actual failure
assertions, current isolated diagnostic passes, unchanged-owner hashes and browser
pending-navigation source. A green isolated rerun alone is not causal proof. Asked
root for the claimed pre-attribution complete Doop timeout record, because the
provided baseline-filter JSON shows passing isolated Doop cases. New company-create/
delete triggers exist in the combined migration, so unchanged test-file hashes
alone are not enough to exclude all integration effects. No source fix requested
for an unclassified failure. Keep failed gates and uncertainty explicit.

Full runner is still executing stock stack then lease/remaining service/deployment
gates. Current final-completion entry remains NOT EXERCISED. Reuse N1's production
source review, guarded e2e1 PASS and inspected full standalone suite120+3browser,
contracts15/boundary11, real Linux scope proof. Await final retained supplement and
failure dispositions before final end-identity check and R2 decision. No redundant
full suite run by reviewer.

R2 evidence addendum (still PENDING): independently read historical Cloudroom
full-gate JSON, SHA7dada4ba, including both full-lib and stock-stack results
4075PASS/4FAIL/26ignored. Both show pre-attribution document-drive and scenario-
lifetime timeouts plus identical markup/MCP failures.
[Compact reviewer extraction](logs/AS-P10-B1-R2-reviewer-historical-failures.txt).
This establishes existing timeout sensitivity, not the exact cause of all current
16Doop/provisioning failures. Parent notified of this bounded conclusion; final
control/disposition and completed runner supplement remain pending.

R2 controls assessment (still PENDING): main coordination remains byte-equal r27
pin. Controls-manifest retained hashes independently match. Read original pristine
baseline compilation failure (61E0659), successful prepared baseline build and
actual 17-case output: 1PASS/16FAIL. Independently audited all16,689 committed
baseline paths: exactly one changed file, reconciliation/contracts.rs, with only
invalid() visibility pub(crate)→pub(super); its bytes equal current. Original
migrations, lockfile and all other tracked source remain baseline. This is a
prepared baseline, not a pristine compilable baseline.
[Independent control audit](logs/AS-P10-B1-R2-reviewer-controls.json).

Provisioning's persistence-bound failure and15Doop document-drive/remote-boundary
deadline failures reproduce without attribution changes. Together with historical
full/stock evidence this supports unrelated-failure disposition for those16, not
merely a claim from green isolated current reruns. The16th current Doop case,
removed-recipient, passes baseline; inspected its full source via Trail and actual
current failure: its45-second arrival wait expires before DELETE company_members
and before business assertions. This remains an unreproduced timing failure,
consistent with the shared drive failures, rather than proven exact root cause.
No business-assertion regression is evidenced; await final current gate disposition.

Independently byte-compared all seven actual knowledge browser inputs, including
original-binary generated HTML/app.js/theme-init.js, to the current failing fixture;
all match the recorded hashes. The browser worker stalled while navigation was
held and was terminated; its isolated current run passes. Exact-input identity
supports unrelated browser/fixture runtime failure, without asserting its exact
cause or erasing the failed browser gate. Dependency metadata explicitly shows
new oauth2 plus subtle(default/std) and url(serde) feature additions; no claim that
all current and baseline build inputs are identical. The prepared baseline retains
original dependencies and features.

Corrected new-test accounting: baseline4244→current4257,12new ordinary tests and
one new ignored realSSO browser, zero removals. Earlier26 passing name-matched
attribution tests included14pre-existing workflow attribution tests. SeparateSSO
execution remains required; do not describe26 as new tests. Fullroot final output,
stock-stack, lease and remaining gates plus managedDoop supplemental outcome and
final artifact identity are still required. No additional source fix or another
full-suite run requested. No reviewer test/resource process remains.

R2 stock/lease/managedDoop addendum (still PENDING): inspected completed stock
output4207PASS/23FAIL/27ignored,4099.70s, no stack overflow and explicit non-stack
failure status. All12new ordinary attribution tests pass; realSSO remains separately
ignored here. Lease gate134PASS with its retained budget table; no budget increase.
Read actual five-case stock2MiB controls (prepared baseline5PASS/current4PASS1FAIL).
Removed-recipient, badDoop-receipt and both history crash cases pass both focused
controls; stock history failure includes lease.claim persistence timeout before
child marker, and recovery deadline. This supports timing sensitivity without
turning the failed complete stock suite green or claiming exact causes.

Department-directory filters remains unclassified: current stock and focused
control both lifecycleConflict; baseline focused passes and current ordinary full
run passes. Independently read Trail/direct exact spans: lease_tests.rs15–17 sets
3seconds; provisioning_dispatch_tests.rs63–108 acquires before bundle setup;
call111–152 reuses original fence; provisioning_directory_tests.rs231–279 creates
2departments and4agents before four listed calls, with no renewal. All3source files
independently byte-equal HEAD. This is a plausible expiry path, not direct observed
proof. Requested a bounded elapsed-lease/denial-cause observation if readily
available; no full-suite rerun or production fix requested for unclassified cause.

Read full managedDoop gate result and failure sections: format/typecheck/lint/
doclint pass;300testfiles pass/2fail/1skip;2701tests pass/1fail/26skip. Both failed
file outcomes are privateS3 fixture setup, not attribution business assertions.
Independently verified all4retained manifest hashes and both runner files against
HEAD. Read actual clean_environment (doop-test.py51–61): it removes Docker host/
config; retained direct Docker probe fails at absent /var/run/docker.sock. This
supports an unrelated environmental limitation; do not claim precise hidden
privateS3 subprocess cause beyond the retained setup error. Remaining managedDoop
build/image/privateacceptance stages NOT EXERCISED after upstream failure.
Fullroot remaining memory/image/service gates and final artifact identity remain
required. No new actionable attribution-source finding in this evidence review.

### AS-P10-B1-N2 final decision — 2026-11-16

**PASS for the verified affected attribution implementation and combined attribution
integration**, exact artifact
`55ecc5eabdcb9493aa9c5ecead843cb075554aa686be101994280bb1b38efb97`,
HEAD/start/base `6e55258cbbdc7633940805f366fb17fe240f02aa`. F1 CLOSED; no open
acceptance-blocking attribution-source finding. This decision permits root's
guarded transfer of this exact source with these records. It is not a whole-
repository green result, a resolution of every workflow failure, or production
deployment/native-platform/pilot acceptance.

Read authoritative main coordination r27 in full before decision; C001–C008/r1
remain active, acknowledged and satisfied by accepted producer phases and the
combined attribution evidence. Re-read original10A–C, final TASK handoff and the
skill's verification-scope/acceptance rules. Exact same11 exclusions as N1/N2.
Reviewer role remains independent B, requested Astra/high; actual model/effort/
context telemetry unavailable. No source edits, subagents, fixture changes or
additional full suites by reviewer. Earlier independent guarded e2e1PASS retained.

#### Final criterion coverage

- AC1/AC3: N1 whole-delta source review, independently executed actual guarded
  commerce e2e, current full service120PASS and separate browser journeys; actual
  duplicate-business/event/refund competitors, durable recovery, cleanup/report
  competitors and independently computed ledger semantics remain covered.
- AC2: all12new ordinary root attribution tests pass in normal and stock runs;
  exact actual Openbot SSO browser8checks passes, including new/existing login,
  independent secure cookies, company authority, logout/rotation/revocation and
  recovery. Current service identity/tenant/security tests and real dashboard
  role/CSRF/foreign-scope assertions pass. Earlier26name-matched count is not a
  new-test count; root inventory adds12ordinary+1ignoredSSO, zero removed.
- AC4/AC6: fresh extracted service120ordinary+3separately exercised browsers,
  contracts15, boundary11, formatting/offlinecheck/Clippy/migrations/metadata/
  testcompile pass. Real nonroot Linux concurrent command-group guard and macOS
  preflight are covered. Actual restore/load/retention proof reused from unchanged
  accepted owners. Both full current-release topologies pass strictTLS, independent
  applications/DBroles, private/plaintext/non-bridge/untrusted-CA refusals and
  outage/expiry/restart/recovery. Root3878bace/service5930cc9c exact images retained.
- AC5: original required complete root runner executed to exit1, all families
  attempted; formatting/offlinecompilation/migrations/metadata/testcompile and
  relevant boundaries pass. Stock4207PASS/23FAIL/27ignored has no overflow;
  lease134PASS. Execution/retention obligation is satisfied, but repository-wide
  success is not established. Failed and dependent unexercised gates below remain
  explicit verification limitations and are not converted into PASS.
- AC7: generic merchant purchase/refund and reports, independent artifact/UI/data,
  exact configuration and deployment/native-store deferrals reviewed against
  original requirements. No hidden runtime dependency or unresolved attribution
  security/durability/ledger/boundedness defect identified.

Read actual final extracted and browser logs, not only manifests or command exit
summaries. Root current production knowledge checker now passes model/relevance/
RSS and all missing/tampered model-check/serve refusals after the owned fixture
mount was supplied. Its current image identity equals the topology image. No
application budget or assertion was raised/weakened. Whole root summary remains
FAIL even where corrected environment checks pass.

#### Remaining repository verification limitations

Confirmed separate failures retain their original evidence: unchanged Clippy
owners; pre-existing markup/MCP failures; provisioning+15Doop deadlines reproduced
on the independently audited prepared baseline; memory HNSW fixture fails with
PostgreSQL53100 shared-memory allocation before search assertions; plugin runtime
install fails package/lock synchronization in two fresh extractions under exact
Node/npm despite reproducible archive. Memory source/lock and plugin manifest/
shrinkwrap/packager owner hashes independently equal HEAD. Memory backup1PASS,
standalone journey13steps with stated stand-in provider, live integration6PASS,
and production imagePASS are separately recorded. Plugin corrected ABI tests133
and guards14 pass; package install remains failed.

ManagedDoop2701tests pass but privateS3 setup fails; actual unchanged sanitized
runner drops Docker endpoint/config and its direct probe cannot reach the absent
default socket. ManagedDoop dependent build/image/privateacceptance are NOT
EXERCISED. Exact seven-input equality supports a separate knowledge-browser
fixture/runtime stall; isolated diagnostic passes but original browser gate fails.
Trail check's two blocking lexical matches are prose, independently inspected at
original01:170 and original03:105; they do not execute/override test commands.

**Unresolved verification gap AS-P10-VG1:** department-directory lifecycleConflict
in stock and five-case current control is not causally diagnosed. Baseline control
passes; ordinary current full run and final observed current case pass. Independently
inspected21read-only samples: no expired lease observed; minimum remaining
0.329799s. Do not claim the original Conflict was expiry or confirmed unrelated.
Unchanged workflow owners, the source trace and current passing observation bound
the evidence; the original failures remain. Other isolated current successes
likewise do not erase earlier failures. This review certifies the directly exercised
attribution contract surface; it does not certify all existing department/workflow
behavior or a globally green integration. Preserve VG1 as unresolved in root's
acceptance/final user report. No further speculative test loop or unrelated source
change is required for this scoped attribution decision.

This scope follows original10B's requirement to run and retain the complete gate,
including unavailable checks, and the skill's separate disposition of verified
affected scope. All required direct attribution checks pass; the global limitations
remain visible. Neither this pass nor root transfer may be reported as complete
whole-repository verification.

#### Final identity and evidence retention

Independently validated all four immutable R2 manifests against exact artifact:
R2(11PASS/6FAIL/1NOTEXERCISED), controls(2PASS/1FAIL), Doop(2FAIL/1NOTEXERCISED),
final(9PASS/8FAIL/1NOTEXERCISED). Retention validation does not decide behavioral
acceptance. Output: [validator](logs/AS-P10-B1-R2-reviewer-final-validation.txt).
End recomputation matches exact digest/HEAD; git diff --check passes; main r27 is
byte-equal pin, SHA30df3e15. [Final identity](logs/AS-P10-B1-R2-reviewer-final-identity.json),
[end snapshot](logs/AS-P10-B1-R2-reviewer-end.snapshot.txt).
All whole-batch source coverage from N1 plus R2 corrections reused unchanged.
No reviewer process remains. Root owns final transfer identity check and resource
cleanup; source acceptance is invalidated by substantive edits.
