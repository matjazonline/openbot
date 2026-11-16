---
title: "Size company-list responses for JSON bytes, not character counts"
author: Mac02
date: 2026-11-16
branch: attribution-service
id: l-20f04c5f
scope: repo
status: active
src: agent
touches:
  - Cargo.lock
  - Cargo.toml
  - migrations/20260817000000_init_schema.sql
  - plan/attribution-service/01-application-boundaries-and-contracts.md
  - plan/attribution-service/03-single-sign-on-and-company-access.md
  - plan/attribution-service/04-merchant-setup-and-api-credentials.md
  - plan/attribution-service/06-website-tracking-and-checkout-handoff.md
  - plan/attribution-service/07-orders-refunds-and-attribution.md
  - plan/attribution-service/09-operations-retention-and-separate-deployment.md
  - plan/attribution-service/10-integration-verification-and-pilot.md
---

## Decided
The authority client's response cap must accommodate a complete schema-valid company page, including JSON encoding expansion. Keep a hard 128 KiB cap and a contract-derived maximum-page test that fails at 80% of the cap, alongside an over-limit valid-JSON rejection case. A larger cap without that early signal would conceal the next growth regression.

## Tried and ruled out
The original 64 KiB cap rejected a schema-valid 100-company page with 120 four-byte Unicode characters per name: the reviewer measured 73,530 serialized bytes before adding a cursor. Escaped control characters can consume six JSON bytes per character, so four-byte UTF-8 alone is not the worst case. Reducing the public page size would conflict with the already accepted limit of 100.

## Watch out
A green normal-name fixture does not cover a character-bounded wire contract. Keep producer permissions and pagination proofs separate from the consumer's authority stand-ins; consumer success cannot establish that the producer returns current roles.

In the code: services/attribution/src/adapters/identity/authority.rs and authority_tests.rs; accepted schemas in services/attribution/contract/openbot-identity.openapi.json. Evidence: plan/attribution-service/phases/p03/docs/agent-review-findings.md, AS-P03-B1-F3; AS-P03-B1-R2-service-1.txt.
