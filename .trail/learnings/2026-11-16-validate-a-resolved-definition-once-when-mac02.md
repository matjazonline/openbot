---
title: Validate a resolved definition once when hydrating provenance
author: Mac02
date: 2026-11-16
branch: feature/plan03_04
id: l-65d46a3b
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
Validate each bounded resolved-definition decode once, then compute the existing canonical JSON digest from that same validated local value. Keep public arbitrary-value hashing fallible and validating. Preserve all tenant, agent, manifest, run, origin and visibility checks in provenance readers.

## Tried and ruled out
A six-file MCP cleanup counterfactual passing did not establish a cleanup protocol cause for a Doop provider-lifetime failure. Actual RPC and cleanup timing was milliseconds; profiling caught repeated JSON-schema compilation while polling run status. Decode already validated the definition, and hashing validated it again. Raw-byte hashing and a durable/global cache would change the contract and were unnecessary.

## Watch out
Separate provenance subreads can still hydrate the same revision. This change removes duplicate validation within a hydration, not every repetition across a request. Retain provider-lifetime and canonical-versus-raw hash tests as early failure signals; do not extend their limits to hide CPU work.

In the code: src/application/agent/definition.rs ResolvedAgentDefinition::decode_and_hash; src/adapters/persistence/workflow/run_provenance/mod.rs RevisionRow::summary; contract tests for canonical pretty JSON, malformed/oversized/secret/schema rejection. Investigation evidence: plan/03-principal-addresses/phases/p04/docs/logs/R6-doop-cleanup-timing2.txt and R6-doop-sample-excerpt.txt.
