#TODO ideas for future 

- main /ui dashboard uses server-side db polling for every SSE stream - use db events hybrid
- convert to HTMX 4
- Add the missing historical reconciliation SQL diagnostic test (replay safety in the current
  schema). Reassess whether supported writers can produce authentic evidence from an earlier
  transaction and a fresh insertable claim-episode identity. If reachable, exercise the actual
  deferred trigger on the public table, assert the exact historical-transaction rejection,
  include a genuine current-transaction positive control, and verify complete rollback.
  Currently, reusing an old identity hits uniqueness constraints first, while new commands create
  fresh evidence and their binding atomically. Revisit when reconciliation or scheduling writers
  change; preserve constraints and avoid fabricating protected history. See
  [the recorded obstruction and existing coverage](../docs/workflow-reconciliation-safety.md).
