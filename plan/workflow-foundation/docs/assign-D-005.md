# Assignment D-005 — group B1: expand and implement

Same role and settings (claude-code / claude-opus-5-5 / high).

Group A is root-accepted (`docs/REVIEW.md`, artifact manifest `64ce8860…`). Start
group B, sub-batch **B1** per `docs/GUIDANCE.md` DEC-007:
prepared/protected, post-reservation, pending provider and token-only semantics;
action-bearing cancellation rollback (DEC-003, DEC-006), with DEC-001 coverage
mapping and DEC-002 race/oracle rules. Originals: BRIEF-04.8 §B and its boundary
table; phase 04 execution item 7 and acceptance.

1. Check guidance against current code; expand B1 fully in a new "Group B1"
   section of IMPLEMENTATION.md (obligation → existing exact test name or new test,
   files, fixtures, failure paths, checks), linking DEC IDs rather than copying.
   If a decision is contradicted by code or a production behavior gap appears,
   stop affected edits and return evidence + DEC ID (DEC-008 reopen rule).
2. Implement tests (production edits only if guidance allows; none expected).
3. Verify at stock 2 MiB on the task fixture: new B1 tests isolated and repeated,
   the affected existing filters (`workflow_action_`, control/cancellation filters
   touched), fmt, diff-check, offline check, strict Clippy; SQLx check if queries
   changed. Every filter count must match its `--list`.
4. Write `logs/groupB/source-manifest-B1.json` covering all files changed since
   `8533a79` (group A files included), and its SHA256.

Stop at review-ready B1. Final response: coverage map summary, new tests,
checks with logs, manifest SHA256, open questions.
