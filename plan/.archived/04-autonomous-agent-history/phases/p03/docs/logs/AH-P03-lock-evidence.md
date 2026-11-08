# Actual P03 lock evidence

D1/r2 starts with existing global authority. authority.rs:45–81 authorizes company
with FOR NO KEY UPDATE before the source run, through action_authority prelocks.
A genuine child preparation blocked on the tenant admission leaf still owns that
global row. Actual parent cancellation and a reused-root preparation therefore
queue at this company lock before reaching conversation locks. The final parent
test asserts those exact observed SQL statements, retained parent pointer while
blocked, joined completion, actual private child branch/cancel, and later reused
root ownership. It does not claim a late three-way conversation cycle was observed.
The D1/r2 conversation-before-admission leaf/hierarchy rules remain conservative
requirements; no company lock, authority owner or bypass was changed to manufacture
concurrency. Separately the actual final-checkpoint COMMIT gate observes queued
turn resume and cancellation waiting behind its transaction, both bounded refusals,
then atomic accepted output/release, successor ownership and harmless completed
replay/cancellation. Actual same/different-stream boundary claimants obey32/256.
A genuine raw native INSERT bypasses the adapter's cap precheck and is rejected by
the agent_history_capacity trigger without position allocation.

Root/A found two implementation defects before review: parked ambiguous COMMIT
needed a retired-attempt receipt serialized behind the original transaction without
demanding its old fence; a separate read-only source-share receipt transaction now
rolls back before a fresh current-authority Ready transaction. Four real Ready/Parked
applied/aborted gates pass. Companion exchange FK needs deferred NO ACTION so
whole-company teardown can finish existing cascades; retained companion still
blocks exchange-only DELETE at commit. Both positive/negative real database cases
pass. Neither adds retention policy, a second writer/queue, new permission or bound.

Earlier failures are retained; no phase acceptance is inferred from this evidence.
