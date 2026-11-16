---
title: Include key identifiers in temporary TLS certificates
author: Mac02
date: 2026-11-16
id: l-4be3169b
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
Temporary HTTPS fixtures explicitly include subject key identifiers on the CA and leaf, plus the leaf authority key identifier. Verification explicitly enables VERIFY_X509_STRICT so older Python versions exercise the same certificate requirements as Python3.13.

## Tried and ruled out
A valid separate CA/leaf pair passed Chromium, Rust and older Python while the production-image topology readiness loop failed on Python3.13. A local HTTPS reproduction isolated Missing Authority Key Identifier. Extending the readiness deadline or disabling strict TLS would hide a malformed fixture instead of fixing it. Adding the identifiers made both full production topologies pass without changing applications or timeouts.

## Watch out
Do not classify a swallowed certificate error as application startup failure. Python's default verification flags vary across versions; preserve the trusted positive path and the untrusted-CA rejection assertion. Unrelated certificates in other fixtures still need their own evidence and scope.

In the code: scripts/attribution/topology.py main() and Fixture.wait_http(); plan/attribution-service/phases/p10/docs/logs/AS-P10-topology-certificate-diagnostic.txt, AS-P10-production-topologies-current-2.txt and AS-P10-production-topologies-current-3.txt.
