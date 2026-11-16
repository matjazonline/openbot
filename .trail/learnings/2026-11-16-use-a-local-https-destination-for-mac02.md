---
title: Use a local HTTPS destination for redirect browser journeys
author: Mac02
date: 2026-11-16
branch: attribution-service
id: l-70a31dc0
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
Use an invocation-owned local HTTPS merchant fixture with Chromium host-resolver mapping when proving attribution redirect navigation. The service still validates an approved HTTPS host and returns its real 302; Chromium follows it to the local fixture at a dynamic port. The fixture certificate exists only in a temporary directory and is removed afterward.

## Tried and ruled out
Playwright context.route for the destination did not intercept the redirected popup navigation in this fixture. The service returned 302 and committed its click, but Chromium tried public DNS and failed with ERR_NAME_NOT_RESOLVED. Replacing a glob with a URL predicate did not fix the missing interception. Do not weaken the journey into only checking an href or mock the service redirect.

## Watch out
The browser's ignoreHTTPSErrors and host mapping are test-only. Service approval still uses the real DNS-protocol fixture, and database assertions must prove the click and its edited product snapshot. Keep the target port dynamic and close the HTTPS server, browser and temporary certificate directory.

In the code: services/attribution/browser/setup.mjs; services/attribution/src/adapters/setup/http_tests.rs; phase p05 logs AS-P05-browser-1/2 failures and AS-P05-browser-3 pass.
