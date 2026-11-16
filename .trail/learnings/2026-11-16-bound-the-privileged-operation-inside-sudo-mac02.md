---
title: Bound the privileged operation inside sudo
author: Mac02
date: 2026-11-16
branch: feature/plan03_04
id: l-2fdea430
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
  - scripts/check.sh
  - scripts/test-network-isolation.sh
---

## Decided
Put a native operation timer inside the sudo boundary so it can terminate the actual privileged child. Retain the outer noninteractive-sudo bound and restore default child signals when the cleanup owner ignores repeated INT/TERM. The operation uses3s+1s inside the existing5s+1s outer bound; no limit is raised.

## Tried and ruled out
Timing out an unprivileged shim after real deletion passed without proving that privileged work stopped. A stronger fixture held the real privileged command, tracked its PID and late effect, and exceeded15s with the outer timer alone. Resetting signals alone and preserving the real sudo process tree alone were insufficient. The inner privileged timer passed that actual-child check.

## Watch out
A sudo fixture must exec the real sudo for the deadline case. An unmanaged subprocess between timeout and sudo changes cancellation topology. Use a wrapping process only for the intentional successful-creator pause used by signal-window tests. Require no surviving privileged child, no late effect, visible cleanup failure and no leaked owned group/chains.

In the code: scripts/test-network-isolation.sh and services/memory/scripts/test-network-isolation.sh use services/memory/scripts/network-isolation-linux.sh. scripts/tests/network-isolation-scope.py holds actual privileged cleanup. Retained failure and passing source identities are in plan/03-principal-addresses/phases/p04/docs/logs/R10-scope-failure-annotations.json, R11-scope-failure-annotations.json, R12-scope-failure-annotations.json and R13-ci-43846081398-jobs1.json.
