# 02WR-P02-C2-R1 — Relay delivery versus workflow admission refusal

Narrow read-only contract assessment,2026-11-11. Partial B1 remains mutable/unverified; no whole-artifact digest or implementation PASS. Scoped source hashes in02WR-P02-C2-R1-read-identities.json. Coordination revision5/I001-I003-r1/C001-r1/activeC002-r2 pinned separately; no source edits or checks run.

**Disposition: preserving delivery success plus a later definite workflow intent refusal satisfies the bounded intent-failure portion of02A.3, but does not satisfy its literal same-relay-refusal-path portion. This is an unresolved product/contract choice, not merely synonymous error terminology.**

Evidence:
- Original02A.3 says a refusal admits nothing, records a bounded definite failure on the intent, and is reported through the same path as a disabled channel in the relay. Referenced docs say the *delivery* records that refusal rather than retrying.
- relay.rs:10-31 returns Relayed immediately when ingest.accepted is true; only rejected ingestion yields RelayDisposition::Refused.
- email/egress.rs:488-512 maps Relayed to ProviderSendOutcome::Delivered and Refused to Terminal/DestinationUnavailable. The existing test egress_tests.rs:508-533 confirms the intended terminal-refusal contract (inspected, not rerun).
- message_admission.rs:235-285,385-483 processes an already accepted message later, records one outcome per candidate binding and settles the intent. Current role classification is definite RoleRefused. It neither awaits nor changes the earlier relay disposition/delivery result. The log explicitly says a binding did not admit an accepted message.
- schedule_admission.rs:189-273 shares the definite refusal classification and records occurrence refusal; this is a schedule result, not evidence of relay delivery semantics.
- D1/r2 retains D1's refusal item5: bounded typed intent/occurrence/manual errors, zero admitted run and unchanged non-role protocols. It did not specify a sender delivery-outcome contract or settle the discrepancy with02A.3's relay clause. Earlier D1 PASS therefore does not constitute user authorization to substitute one outcome for the other.

No supplied later user instruction explicitly disposes this relay clause. Unlike C1's department framing, there is no accepted specific user decision here overriding its wording. The pending semantics question can resolve it; elapsed time/no answer does not select either result. Root may continue independent implementation/tests, but should not declare the full02A.3 criterion met solely from green workflow-refusal tests.

If the user accepts delivery success plus separate workflow refusal, retain that explicit scope disposition and verify/document both observable outcomes. If the intended requirement remains sender-visible relay delivery refusal, the present asynchronous intent path leaves that portion unmet and the resulting durable delivery/admission contract change needs an architecture gate before affected edits. This assessment chooses no design mechanism and does not authorize changing ingestion, waiting on workflows or coupling all candidate bindings to delivery.
