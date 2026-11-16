---
title: Scope test egress rules to the command group
author: Mac02
date: 2026-11-16
branch: feature/plan03_04
id: l-0602646b
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
Linux test-network isolation selects a dedicated temporary primary GID for the preflight and wrapped command tree, while preserving the original UID, supplementary groups and environment. The supervising CI runner retains its original group and its sockets remain outside those rules. scripts/test-network-isolation.sh and services/memory/scripts/test-network-isolation.sh delegate to one mechanism kept inside the standalone Memory service.

## Tried and ruled out
Selecting the invoking UID also selects unrelated CI control sockets. A real non-loopback local TCP fixture demonstrates this: the supervisor can connect during group isolation, and changing only the selector back to UID blocks it. Omitting debug metadata did not prevent the later hosted-runner communication loss; do not attribute every historical loss to OOM or to this one cause.

## Watch out
Bash can deliver a pending INT/TERM after a successful external creator returns but before the next ownership assignment. Defer exit until creation ownership is published, then preserve130/143 and clean exact owned resources. Track installed hooks separately from created chains. Bound cleanup, report incomplete removal, and retain the group if firewall cleanup failed so its GID cannot be reused under a stale hook. Signal tests must pause an actual successful creator, not just return an error before creation.

In the code: services/memory/scripts/network-isolation-linux.sh; scripts/tests/network-isolation-scope.py exercises both entrypoints, the UID counterexample, successful-creation signal windows and cleanup deadlines. Accepted decision D3/r1 and independent findings03PA-P04-B1-R8/F1-F2 are retained under plan/03-principal-addresses/phases/p04/docs/.
