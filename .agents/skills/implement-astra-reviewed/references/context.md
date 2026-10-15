# Context measurements, rotation, and resumption

Read before assigning workers. This helper is read-only and targets Codex. Resolve
its path relative to this installed skill, not the project's working directory:

```sh
python3 /absolute/path/to/implement-astra-reviewed/scripts/context_usage.py --runtime codex --json
python3 /absolute/path/to/implement-astra-reviewed/scripts/context_usage.py --runtime codex --thread-id VERIFIED_SESSION_UUID --json
```

## Identify and measure the right session

Obtain each worker's verified `CODEX_THREAD_ID` from its own runtime environment or
authoritative session metadata. Collaboration handles are separate identifiers;
never guess a UUID or choose the newest log. Workers self-report identity and root
verifies measurements using that UUID. Read the depth-0 row for the requested agent;
never sum context percentages across agents.

Use latest recorded request input divided by runtime-reported context capacity.
Do not use cumulative tokens, add cached input twice, or infer capacity from a model
name. Record session ID, input, capacity, percentage, sample timestamp, measurement
source, and capacity source. The sample excludes growth after that model request:
reserve headroom for output, tool results, coordination, and checkpointing. Never
present this as an exact live occupancy or hard context guarantee.

Measure root at startup, before assignments, after preparation/accepted points,
and after substantial coordination. Workers measure at startup, before large reads
or edits/reviews, during substantial assignments, and when returning. Root verifies
a fresh sample before reuse; a sample taken before the completed assignment is
insufficient. Reuse samples at coincident boundaries only without intervening
substantive work. After compaction, remeasure; compaction does not create a fresh
session or cancel a retirement decision.

The helper prints JSON errors for unavailable identity/log access. Rows with null
percentages or a non-null `reason` are unresolved even if its exit status is zero.
This includes missing capacity, compaction after the last sample, or inconsistent
accounting. Prefer an authoritative host context metric when available and record
its provenance. This script measures context, not total token spend.

## Preserve continuity within thresholds

- Below 50%: reuse A for related expansion, implementation, and fixes when the next
  assignment fits with headroom. Reuse B for related reviews under the same rule.
- At or above 50% for either worker: allow only checkpointing and return. Verify it
  stopped, retire it, and spawn a fresh worker of the same role with no inherited
  conversation. The sibling can remain eligible; rotating A does not retire B.
- At or above 60% for root: assign no new substantive work. Request checkpoints from
  workers, interrupt if necessary, and verify both stopped. Save the root handoff
  and end the run. Resume in a genuinely fresh root session; do not spawn a proxy
  root or treat automatic compaction as a reset.

Keep assignments small enough to return before crossing thresholds. Split an
oversized point into verifiable substeps while retaining its final acceptance gate.
Rotate early if the next assignment is too large or unrelated history dominates.
Do not raise thresholds to avoid handoffs or resume a retired worker. Respect agent
slot limits: interruption may not release a handle. If retirement exhausts slots,
checkpoint for a new root session rather than repeatedly trying to spawn.

## Missing or stale measurements

On an unresolved sample, do one bounded read-only identity/log inspection and retry
once. A newly started worker may have no completed request yet: let it return a
minimal identity/status response, then remeasure before substantive work. Never
manufacture a reading by substituting cumulative usage or another session's log.

If a formerly measurable worker remains unresolved, retire it conservatively.
When measurements are generally unavailable, disclose the limitation and use
bounded fallback: one small, independently verifiable dependency group per fresh
A/B pair, no unrelated follow-ups, and checkpoint root at that group's end. Keep A
for that group's expansion and implementation and B for its reviews, but allow only
one correction/recheck cycle across the entire group (architecture and code gates
combined) before checkpointing unresolved work for a fresh run.
This is a workload bound, not a context guarantee. Record fallback status in the
handoff. If the group cannot be bounded safely, stop with a handoff; if the user
requires measured thresholds, stop rather than silently applying fallback.

## Handoff and reconciliation

Before retirement, A records original requirements, decisions and rationale,
affected symbols, exact baseline/diff, preservation constraints, accepted/partial
points, tests/logs/fixture identities, open findings, and the next bounded action.
B records reviewed artifact identity, coverage, findings/resolutions, assumptions
challenged, remaining checks, and invalidated evidence. Root records acceptance,
queue/dependencies, role/session IDs, context readings/fallback, resource ownership,
existing authorizations or pending approvals, and continuation session links.

Link originals and full evidence; do not paste transcripts. A replacement author
reads originals and the decision record before continuing. A replacement reviewer
reads originals, actual artifacts, and review evidence and independently checks the
remaining scope; do not substitute A's handoff for requirements. Preserve accepted
work but reopen review wherever the actual tree or environment invalidates evidence.

On fresh-root resume, measure root, reconcile the handoff with the worktree and
resources, create fresh A/B workers, and continue the first unaccepted point. Count
all linked sessions in task usage. A saved handoff is not itself a fresh session.
