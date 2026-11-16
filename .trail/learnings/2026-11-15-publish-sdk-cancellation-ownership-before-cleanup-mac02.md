---
title: Publish SDK cancellation ownership before cleanup can run
author: Mac02
date: 2026-11-15
branch: feature/plan03_04
id: l-057c03ea
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/ci.yml"
  - ".github/workflows/memory-service.yml"
  - plan/03-principal-addresses/COORDINATION.md
  - plan/03-principal-addresses/phases/p04/docs/HANDOFF.md
  - plan/03-principal-addresses/phases/p04/docs/REVIEW.md
  - plan/03-principal-addresses/phases/p04/docs/TASK.md
  - plan/03-principal-addresses/phases/p04/docs/agent-review-findings.md
  - plan/03-principal-addresses/phases/p04/docs/agent-review-notes.md
  - src/adapters/http/routes/agent_api_tests.rs
  - src/adapters/http/routes/agent_api_tests/public_settings.rs
---

## Decided

One per-connection cleanup gate must cover SDK queue admission and publication of the real request ID, then release before waiting for the remote response. Serializing cancellation and DELETE alone is incomplete: another SDK task can dispatch the queued call while the caller's poll is preempted before its ID reaches shared cleanup state. Preserve the single pending-ID owner, fixed cancellation message, current-source guards and existing cancellation/cleanup budgets.

## Tried and ruled out

The first actual competing drop/shutdown tests and 115 focused cases passed while leaving the enqueue/publication interval uncontrolled. Independent review found the missing interleaving. A synchronous per-connection pause with a held real HTTP call and actual SDK deletion entry now forces that interval; removing only the registration lock makes the real wire-order assertion fail. A plain synchronous pause parked Tokio's core and prevented SDK dispatch; block_in_place hands the core to another worker without introducing an async cancellation point in the tested handoff.

## Watch out

Do not hold the registration gate through await_response: cleanup must be able to cancel a pending effect. Do not infer IDs, clear uncertain transport failures as completed responses, or treat joined workers as proof a cancellation notification was sent. The installed SDK's no-options Sender.send handoff has no further suspension after enqueue; recheck cancellation safety when changing SDK/options or adding an await there. Release test pauses on failure and prove the old ordering fails the actual protocol assertion, not just a mutex check or probe timeout.

In the code: src/adapters/mcp/mod.rs owns admission/publication; http.rs owns serialized cancel/delete; registration_test_support.rs is per-connection cfg(test) only. cleanup_tests.rs mcp_enqueue_preemption_cancels_original_call_before_shutdown_deletes_session uses real SDK/loopback credentials, negotiated version/session and exact original request ID. Plan03 p04 D2/r4 and D2-r4-old-ordering-proof.json retain the decision and counterexample; B1-R4/F1 is the independent finding.
