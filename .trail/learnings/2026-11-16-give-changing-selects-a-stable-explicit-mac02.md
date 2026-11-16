---
title: Give changing selects a stable explicit label
author: Mac02
date: 2026-11-16
branch: attribution-service
id: l-73b4ae1a
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
Use a separate label with an explicit for/id association for the verified-domain select. Keep browser tests selecting it by its exact label. Rebuilding option text must not change the control's user-facing label.

## Tried and ruled out
The setup browser journey repeatedly timed out on getByLabel("Verified domain", {exact:true}) after DNS verification had succeeded. Disabling action buttons through refresh did not resolve it. The original label wrapped the select and its changing option text. Replacing that wrapping label with an explicit association made the complete eight-check journey pass, without loosening the locator or bypassing the form.

## Watch out
A passing API test cannot establish that the rendered form can be used by its label. Keep the complete keyboard/form journey. Also keep [hidden] explicitly hidden when author CSS sets display on forms or labels, so member views do not reveal management controls.

In the code: services/attribution/assets/setup.html, assets/app.css, browser/setup.mjs. Evidence: plan/attribution-service/phases/p04/docs/logs/AS-P04-browser-3.txt and AS-P04-browser-4.txt (fail), AS-P04-browser-5.txt (pass).
