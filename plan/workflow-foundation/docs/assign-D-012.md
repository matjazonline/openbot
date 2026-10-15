# Assignment D-012 — correct F-007 and F-008 (R-014/R-015)

Same role and settings (claude-code / claude-opus-5-5 / high).

S3 reviewed C1 artifact `e59bb3f0…` (HEAD `94c77e0`): F-006 resolved; changes
required. Read R-014, R-015 and F-007, F-008 in agent-review-notes.md /
agent-review-findings.md and implement the correction criteria:

- F-007: the RunDeadline mode of
  `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline`
  can pass on an earlier lease expiry. Add a positive run-limited-window witness
  and a negative control that rejects lease-limited termination. The 1.5 s fixture
  allowance is otherwise accepted; no bound changes.
- F-008: run `graft build` at repo root after final corrections; record the delta.

Verify: corrected test isolated ×3 + new control ×3; `workflow_action_dispatch_`
filter; fmt, diff-check, offline check, strict Clippy; then on the final
artifact the full library suite at stock 2 MiB, bin tests, `stack-budget.sh
--offline` and `lease-budget.sh --offline` (record uptime/foreign cargo at each
boundary as before). Then `graft build`. Add "C1 correction (F-007/F-008)" to
IMPLEMENTATION.md, logs in logs/groupC/, manifest
`logs/groupC/source-manifest-C2.json` (vs `8533a79`, record HEAD; the user may
commit — note it if so). Stop after; final response: gate table, manifest SHA256.
