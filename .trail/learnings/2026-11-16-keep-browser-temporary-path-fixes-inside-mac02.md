---
title: Keep browser temporary-path fixes inside the supervised lifecycle
author: Mac02
date: 2026-11-16
branch: feature/plan03_04
id: l-6aa31277
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
  - scripts/check-doop.py
  - scripts/check.sh
---

## Decided
Host Chrome receives a child-only TMPDIR=/tmp so its private singleton Unix socket fits the platform path bound even when an invocation has a deeply nested temporary directory. Keep userDataDir, HOME, sandbox, TLS and process receipts under their existing contracts. Exercise actual private browsers through the existing Python-supervised private/e2e acceptance path; the launch-option protocol test also asserts that parent environment and profile custody do not change.

## Tried and ruled out
An isolated actual Ubuntu counterexample failed with Chrome FATAL Socket path too long under the shared runner's nested clean TMPDIR, and passed with a short temporary base. Shortening only a CI directory would leave other long local paths broken. A separate real-browser preflight looked useful but introduced an unsupervised process owner: the independent reviewer forced created-receipt publication failure and observed that close was never called; a stuck close needed external termination. Remove that duplicate consumer rather than invent another recovery ledger.

## Watch out
A process wrapper must inherit the whole supervisor/recovery lifetime, not only launch options and happy-path receipts. Existing pending ownership must remain fail-closed. Do not promise that killing Chrome deletes every native OS temp file, weaken its sandbox, or serialize raw browser/provider errors containing credentials. The private browser profile remains invocation-owned; only native internal temporary IPC uses the normal short OS base.

In the code: scripts/doop-test/browser-owner.mjs and scripts/tests/doop-probe.mjs. Plan03 p04 logs/R20-browser-launch-annotations.json and R20-browser-launch-workflow.yml retain the actual platform counterexample; agent-review-findings.md R14/F1 and logs/B1-R14-ipc-owner-probe.txt retain the rejected preflight's failure proof. D4/r1 and R13/N2 classify the narrowly scoped temporary-base correction.
