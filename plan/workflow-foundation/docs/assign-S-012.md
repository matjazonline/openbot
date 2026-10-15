# Assignment S-012 — group C failures: precision portability and timing independence

Same role and settings (codex / gpt-6-astra / high). Read-only. D2 has
background budget scripts running; do not run tests yourself.

Group C full stock-2MiB library suite on artifact `c3379ee3…` (B3, unchanged):
2550 passed / 7 failed / 22 ignored (`logs/groupC/runtime-all-lib.C.log`).
D2's diagnosis is in its output and will be in IMPLEMENTATION.md "Group C":

A. Five deterministic Linux failures, files outside 04.8:
   `adapters::persistence::attention::tests::{agent_work_stays_out_until_a_human_decision_is_required, the_queue_ages_the_generation_rather_than_the_row}`,
   `adapters::persistence::task::tests::extending_an_expired_outreach_serializes_with_the_timeout_sweep`,
   `adapters::persistence::schedule::tests::claiming_anchors_the_next_run_on_the_slot_not_the_clock`
   (`14399 % 3600 = 3599`), and `...wait_tests::workflow_wait_competing_park_signal_resume_and_lost_ack`.
   Claimed cause: nanosecond Rust timestamps vs Postgres microsecond storage
   (macOS clock is microsecond).
B. Two load-dependent 04.7 dispatch tests (host load ~19 from an unrelated
   project): `workflow_action_dispatch_delayed_first_poll_checks_monotonic_window`
   (0.3 s lease lapsed → Conflict) and
   `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline`
   (4 s Elapsed).

User decisions: (A) fix within 04.8 — test-only unless you find a production
precision bug; (B) either rerun when load is lower, or change verification so it
does not depend on timing — the user wants that option assessed.

Task:
1. For each A test: confirm root cause from code; classify test-only vs production
   (e.g. does any production path compare or derive from an un-truncated
   timestamp, e.g. schedule slot anchoring?). Give a correction criterion per test
   (DEC-013). A production bug must be flagged explicitly (reopens scope; root
   will take it to the user).
2. For B tests (and generally DEC-009..012 timing-sensitive tests): can each be
   made load-independent without weakening its oracle — e.g. causal/event
   ordering, genuine lease renewal at quiescent points, deadline evidence rather
   than wall-time budgets — while keeping AGENTS.md "something still fails early"?
   Say which are feasible, the criterion, and which must remain timing-based
   (then the remedy is rerun on a quiet host). Also say whether
   `scripts/lease-budget.sh` / `stack-budget.sh` are load-sensitive and how their
   results should be judged (DEC-014).
Record R-012 and DEC-013/014 in GUIDANCE.md. Final response: per-test
classification, criteria, production-bug flag, and B recommendation.
