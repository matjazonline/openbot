---
title: TLS integration fixtures need a separate CA and server certificate
author: Mac02
date: 2026-11-16
id: l-a47308cb
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
Use an ephemeral CA trust anchor and a separately signed server leaf with CA:FALSE,
serverAuth and the actual host SANs when exercising application HTTPS. Install only
the CA in the container trust store; keep normal application verification enabled.
Pin the fixture leaf key for the local browser when avoiding machine-wide trust edits.

## Tried and ruled out
Serving the self-signed CA itself worked for the browser/OpenSSL fixture but the
actual attribution-to-Openbot callback failed with HTTP 503. Replacing it with a
CA-signed server leaf allowed the real PKCE exchange to complete. Browser HTTPS
success alone did not establish that the application's TLS verifier accepted it.

## Watch out
A test-only TLS bypass can conceal an invalid fixture certificate. Keep the trusted
and untrusted checks separate, and do not claim an OpenSSL probe alone exercises the
application transport. The fixture key is public test material, never a production key.

In the code: scripts/attribution/topology.py; p09 logs AS-P09-topologies-7.txt and
AS-P09-topologies-8.txt retain the failed and corrected callback evidence.
