---
title: Aggregate report measures before joining nullable group keys
author: Mac02
date: 2026-11-16
id: l-3e92c816
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
Aggregate each measure by group and currency before joining it to paginated groups. For nullable group keys use hashable equality over both the null flag and coalesced value. Preserve the exact two-second query budget and a real 10,000-row export boundary test.

## Tried and ruled out
An ungrouped overview with 10,000 clicks and 1,000 orders was fast, but joining every raw measure to 10,000 export groups with IS NOT DISTINCT FROM caused a nested-loop timeout. The exact maximum export returned 503. Raising the deadline would have hidden the growth. Aggregation plus null-safe hashable equality passed both 10,000-row acceptance and 10,001-row rejection under the same deadline.

## Watch out
A small overview is insufficient query-plan evidence for grouped exports. Preserve null groups, contextual purchased-product counts and independent financial cardinalities when changing the query. Stock PostgreSQL can choose sequential scans for a fixture that selects almost every row; that alone is not a missing-index defect.

In the code: services/attribution/src/adapters/analytics/query.rs and bounds_tests.rs. Evidence: plan/attribution-service/phases/p08/docs/logs/AS-P08-bounds-2.txt (timeout) and AS-P08-bounds-3.txt (pass).
