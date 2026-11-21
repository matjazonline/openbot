---
title: Use one clock for ordered cross-host commerce fixtures
author: Mac02
date: 2026-11-16
id: l-63fc03a4
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
Derive ordered commerce fixture timestamps from the same database clock that timestamps the preceding click. Ask the invocation-owned test control for the current database time after each preceding effect. Keep production click eligibility and payment/refund chronology strict.

## Tried and ruled out
A topology journey passed while the host and VM clocks happened to align. After restarting the task-owned VM, its clock was about 235ms ahead of the host. A real redirect committed a click, but a checkout timestamp created immediately with host Date.now() preceded that click, so the service correctly returned no attribution. Waiting a fixed delay would conceal the cross-clock assumption rather than eliminate it.

## Watch out
Browser success in one run does not prove a fixture's timestamp ordering. The test control must remain local to the fixture, never a production time endpoint or a bypass for eligibility. Keep exact boundary tests separate.

In the code: scripts/attribution/topology.py Fixture.action; services/attribution/browser/topology.mjs; p09 AS-P09-B1-F2 and logs/AS-P09-topologies-13.txt.
