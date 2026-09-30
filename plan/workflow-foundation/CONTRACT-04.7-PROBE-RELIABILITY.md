# 04.7 — exact-run observation without an error transaction

Status: bounded Architecture proposal, frozen for independent Sol check. Not
implementation or row2c acceptance. Root alone accepts. This replaces only the
derived observer mechanism in CONTRACT-04.7-CANCEL-SERIALIZATION.md steps 1/3/5;
all owner ordering, attribution, outcomes and gates in that contract remain.

## Requirement and evidence boundary

BRIEF-04.7.md:35–45,137–152,192–239,273–277 requires authentic authorization,
owning-run serialization, receipt truth and no terminal advancement. It does not
require NOWAIT, SQLSTATE55P03, explicit observer transactions or a 300ms probe.
Those are fixture choices. The accepted cancellation contract correctly requires
independent exact-run observation as well as actual company contention: company
contention alone cannot detect an omitted early run lock.

The original composite BEGIN/NOWAIT/ROLLBACK observer failed all four tests once
at 300ms. Its identical-source pass and six instrumented passes do not identify
the cause. PostgreSQL logged expected NOWAIT errors after fixture force-drop had
already begun; this does not identify query, logging, protocol or scheduling delay.
There is no evidence establishing an external host cause or a pre-existing flake.
Preserve all original failed logs and review R1 as historical unresolved cause.

Replace the multi-command, expected-error transaction with a single successful
autocommit query that distinguishes an existing locked row from an absent row.
This reduces the work/lifecycle under the *unchanged* 300ms bound; it is not a
retrospective root-cause claim or proof against arbitrary machine stalls.

## Concrete observer

Use the already dedicated observer connection outside an explicit transaction.
Bind the exact command company/run. One suitable statement is:

```sql
SELECT visible.id,
       (SELECT locked.id
          FROM workflow_runs AS locked
         WHERE locked.company_id = visible.company_id
           AND locked.id = visible.id
         FOR UPDATE OF locked SKIP LOCKED) AS lockable_id
  FROM workflow_runs AS visible
 WHERE visible.company_id = $1 AND visible.id = $2
```

The outer MVCC read must return exactly the expected run ID. No outer row, wrong
ID, unexpected additional row or any database/protocol error fails observation;
none is a held-lock result. For the known unique scoped run, the inner value is
the exact ID when lockable and NULL when its row lock cannot be acquired. Require
the exact ID for the positive pre/post probes and NULL for the held probe.
Never infer a lock solely from `fetch_optional == None` on a SKIP LOCKED query.

PostgreSQL's [SELECT locking documentation](https://www.postgresql.org/docs/18/sql-select.html#SQL-FOR-UPDATE-SHARE)
specifies that SKIP LOCKED skips rows whose row locks cannot be acquired
immediately; it does not exempt table locks. This fixture owns its database and
runs no concurrent DDL. The outer visible row explicitly excludes missing-row
ambiguity. The observer performs no mutation or business transition.

Keep the execution-only gate and complete a positive probe before either owner
starts. Then observe the first real owner's exact execution query waiting on the
gate PID, and poll that owner alongside the held probe, failing on early return.
Before the second owner starts, the first is the sole possible owner of a
conflicting run lock: the gate was positively excluded, the dedicated observer
has no retained transaction and there is no other fixture writer. Both business
orderings remain necessary to prove each owner's early run lock independently.
If these exclusivity assumptions cease to hold, stop and use the accepted
PID-linked blocking-observer fallback; do not infer attribution from NULL alone.

Retain the actual second-owner company-query/first-PID wait for both cancellation
orders and direct run-query/first-PID wait for both expiry orders. Release the
execution gate, drain both actual owners and require the positive exact-run probe
again. Keep all twelve Applied/Final/Unknown cases and every outcome/history
assertion unchanged.

## Completion, failure and bounds

Fully consume the observer result through PostgreSQL ReadyForQuery before accepting
the observation or polling the next business owner. SQLx0.8.6 PostgreSQL
`connection/executor.rs:412–440` drains `fetch_optional` through the stream;
`:360–363` handles ReadyForQuery before stream completion. `fetch_all` also fully
collects it. Do not use an early-dropped row stream or a spawned/detached query.
In autocommit this releases any successful observer row lock at statement end.
Inspect the actual pinned SQLx implementation during independent check, and include
the observer PID in the final backend assertion of no active transaction/query.

Keep the existing 300ms outer bound around each complete probe; keep existing
300ms competing-owner observations, bounded failure snapshot and 900ms real-owner
completion bound. Keep stock2MiB/default test parallelism and every production
deadline/stack setting. No bound is raised, retried, silently ignored or replaced
by a sleep. The same CI tests therefore retain the existing early latency failure
signal while doing less work per probe. Do not add a new larger cleanup grace.

Retain compact probe phase/elapsed/run/company/PID diagnostics and the bounded
gate-connection backend snapshot before panic cleanup. A timed-out autocommit
query is not a successful observation: fail the case and dispose of its isolated
fixture, never return the incomplete connection to further business work. Keep
existing fixture-owned force cleanup. Do not assert that dropping a Rust future
synchronously cancels or rolls back an in-flight PostgreSQL query.

## Discriminatory checks and acceptance

Sol independently checks this proposal against the originals and actual source
before edits. Implementation is limited to the serialization observation helper
and narrowly needed test support; production owners/migrations remain unchanged.

Retain explicit negative controls using the same observer and held-result
classification: an existing unlocked exact run (execution gate held, no first
owner started) must be rejected as held, and a nonexistent scoped run must fail
observation rather than count as held. Positive pre/post observations and these
negative controls must use the same SQL/decoder as the real held observation.
This makes an omitted or postponed first-owner run lock produce the unlocked
result, while an incorrect scope/missing row cannot fabricate contention. A
database-backed check must exercise both controls, not just a Rust NULL predicate.

Run the four final tests/all12 variants and the negative controls at stock2MiB
and default parallelism once, then the required affected suite and static/SQLx/
schema gates specified in BRIEF04.7. Record query/drain timings and final backend
quiescence. No repeated campaign until green. Any new timeout remains a failure
with diagnostic evidence and reopens investigation. Independently review actual
source plus results against original invariants, including the single-query
statement's scope, attribution, cleanup and negative controls. A green rerun of
the old constructor alone is still insufficient; acceptance here depends on a
reviewed replacement mechanism plus discriminatory checks. Root then decides
whether R1 is resolved by replacement, explicitly retaining unknown historical
cause. No full04.7 or remaining-row2c acceptance follows from this subgroup.
