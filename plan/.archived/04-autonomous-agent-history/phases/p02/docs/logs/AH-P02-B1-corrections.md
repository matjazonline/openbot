# AH-P02-B1 corrections (in progress)

B1-R1 returned exactly F1/F2, medium, against source0c6df193. Root read complete
REVIEW, new findings and whole coverage notes; B inspected evidence, no runtime
rerun. No new architecture gate: these implement the already approved D1/r2
lifetime and complete-reopen evidence requirements. Source freeze lifted for one
cohesive correction batch; B idle until all findings and focused gates are done.

F1: native DELETE first allows whole-company teardown, refuses queued/active,
and inspects failed source under FOR SHARE NOWAIT. DELETE already owns its tuple,
so it must not wait for the opposite native control run→exchange order. A busy
run fails immediately with55P03. An unexpired failed row remains pinned while
source classification is nonterminal or the actual existing native root retry
predicate permits the same terminal execution/job. Expired or genuinely
nonresumable sources may release their pin; retry authority is not duplicated or
widened. Native same-execution retry test now attempts DELETE and checks refusal
before its existing stable input/order/checkpoint/accounting assertions. Added
actual control retry versus DELETE lock contention, expired/nonresumable cleanup,
and whole-tenant teardown cases. No phase06 TTL/backpressure policy claimed.

F2: new read-only linked_on validates actual source eligibility, server preparation
proof, exact checkpoint link and non-queued state. Ambiguous checkpoint open now
requires that additional effect; queued with an old matching checkpoint is
NotApplied, while legitimate completed/terminal replay remains valid. Fresh
provider Active floor remains. Renamed the previous lost-ack test to durable
replay to describe its actual coverage. Added real existing Gate deferred-COMMIT
injection for both applied/aborted queued-checkpoint reopen, plus enqueue applied/
aborted and final-output unknown acknowledgement with counted actual provider
replay. No new proxy or queue framework and no raised bounds. Native Gate helpers
are reused under their original supervised/isolated fixture ownership.

Fresh ownedp02pair reset and main/test migrations-findings02 PASS with newDDL.
New tests compile/list findings-history-list02 currently running; inspect receipt
before another Cargo/DB operation. No runtime correction pass yet. Preserve all
R1 evidence, source archive and hashes. Same9exclusions/coord5 notices/external36.


## Focused correction results

findings-history-list03 selected58; findings-history-runtime03 passed58/0ignored.
findings-affected-list04 selected238; findings-affected-runtime04 passed238/0ignored.
Both include actual applied/aborted reopen/queue uncertainty, final ack unknown with
exact one real scripted provider request, old-fence and current connection revocation,
DELETE refusal before normal safe retry, genuine DELETE/control contender, permitted
expiry/nonresumable cleanup and whole-company teardown. Every new test ran under
stock2MiB and network isolation. Earlier compile-only two dereference mistakes were
fixed and retained in findings-history-list02; runtime correction03 passed first run.

257 distinct targeted passes are pinned in targeted-runtime-evidence-r2:238 current,
including58 history, plus19 unchanged generic retention cases explicitly reused.
Current migration pair and actual SQLxprepare/check/alltargetoffline passed.
Clippy/fmt/diff/Graft pipeline is still active; wait before R2 freeze/assignment.
