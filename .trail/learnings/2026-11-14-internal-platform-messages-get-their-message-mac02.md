---
title: "Internal platform messages get their Message-ID from the email renderer, never a hand-built one"
author: Mac02
date: 2026-11-14
branch: room/check-last-handoff-md-under-plan-02-work-qc8d
cost: { minutes: 263, tokens: 1593299 }
id: l-332d37ae
scope: repo
status: active
src: agent
touches:
  - src/application/transport/ports.rs
  - src/application/transport/mod.rs
  - src/application/transport/ingress.rs
  - src/application/use_cases/thread/ingest/policy.rs
  - src/application/use_cases/thread/ingest/commit.rs
  - src/application/use_cases/thread/ingest/relay.rs
  - src/adapters/protocols/email/egress.rs
  - src/adapters/persistence/thread/inbound.rs
  - migrations/20260817000000_init_schema.sql
  - src/adapters/persistence/workflow/message_thread_tests.rs
---

## Decided
Messages the application creates itself, such as an escalation notice sent to an agent's desk, take their key from `TransportRenderer::application_message_key(company, nonce)` through the email renderer port. Canonical application ingress does the same. The application layer mints the key and persistence uses it as given. A stable nonce, such as the notice id, keeps retries idempotent. A schema guard that has to recognise such a message checks only the nonce in the local part, `split_part(rfc_message_id,'@',1)='<'||id`, and leaves the domain to the transport.

## Tried and ruled out
A literal like `<escalation-{id}@openbot.invalid>` built in persistence breaks the transport boundary in two ways: it is a synthetic `.invalid` identity, and it is an RFC id made outside the email adapter.

## Watch out
Test code may not build `OutboundEmailV1 { .. }` outside src/adapters/protocols/email either; put such fixtures in that adapter's cfg(test) test_support.

In the code:
- src/application/notification.rs `agent_notice_key`
- src/application/use_cases/thread/ingest/canonical.rs `canonical_email_headers`
- src/adapters/protocols/email/egress.rs `application_message_key`
- src/adapters/protocols/email/test_support.rs `relayed_part`
- init schema `guard_workflow_escalation_agent_receipt`
