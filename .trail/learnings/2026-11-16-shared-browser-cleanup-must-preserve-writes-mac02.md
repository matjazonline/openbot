---
title: Shared browser cleanup must preserve writes made after its read
author: Mac02
date: 2026-11-16
id: l-2cfeeacd
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
Prune only candidate keys that the cleanup read actually considered. A later enumeration cannot treat a newly written key as unwanted merely because it is absent from the earlier local snapshot. Keep each returned handoff bounded, and let each writer's subsequent read prune its own observed candidate set.

## Tried and ruled out
Separate localStorage keys per click and a sequential two-tab test both looked sufficient and passed the full merchant journey. Independent review paused one real Chromium tab between reading candidates and cleanup, let another tab persist a new click, then resumed cleanup: the newer click was deleted. The second tab's in-memory history hid this until navigation. Per-key storage does not make a read/cleanup sequence atomic.

## Watch out
Test a competing writer after the read and before cleanup, then navigate and assert the next consumer's persisted history. An in-memory assertion in the writing tab misses the loss. Expiry and bounded eviction must not delete unseen concurrent keys.

In the code: services/attribution/assets/sdk-1.0.0.js historyEntries; services/attribution/browser/website.mjs CDP-controlled two-tab cleanup/write stage; p06 AS-P06-B1-F1 and reviewer-browser-race probe; corrected browser run AS-P06-browser-5.txt.
