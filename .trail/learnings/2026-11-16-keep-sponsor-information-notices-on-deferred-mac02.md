---
title: Keep sponsor information notices on deferred notification projection
author: Mac02
date: 2026-11-16
branch: plan08_ph1
id: l-88420aa8
scope: repo
status: active
src: agent
touches:
  - docs/departments.md
  - docs/workflow-language-v1.md
  - migrations/20260817000000_init_schema.sql
  - src/adapters/http/pages/attention.rs
  - src/adapters/http/pages/company_departments.rs
  - src/adapters/http/pages/dashboard.rs
  - src/adapters/http/pages/department_ui_tests.rs
  - src/adapters/http/pages/workflow_authoring/bindings.rs
  - src/adapters/http/pages/workflow_authoring/reviewer_picker.rs
  - src/adapters/http/pages/workflow_runs/provenance.rs
---

## Decided
Create sponsor information source events in the decision or followed-role transaction, then let the existing notification projector acquire the recipient account prefix before company/run/decision ownership and compose delivery. Information capacity grants neither reviewer authority nor desk visibility.

## Tried and ruled out
The initial Phase 01 design reused synchronous decision-email composition during followed-role updates. Independent architecture review found that these callers already own the company: composing then would acquire accounts in the opposite order to account retirement. A separate direct-delivery owner would also miss the existing notification retirement cancellation union. Reuse the deferred notification owner instead of adding locks to shared authority helpers.

## Watch out
Runtime fixtures must grant normal desk visibility to sponsor people. Assigning the role alone does not do this: the first scripted test created two decision headers but had no eligible reviewers. Wait for actual parked decisions with reviewer rows, not header counts. Keep informed sources out of reviewer selection and settlement. Test pending/in-flight cancellation and projection after both account and membership retirement.

In the code:
- plan/08-goals/phases/p01/docs/agent-review-findings.md: 08G-P01-D1-R1-F1/F2 and D1/r2 dispositions.
- src/adapters/persistence/workflow/decision_information.rs; notification/reviewer_notice.rs.
- src/adapters/persistence/workflow/sponsor_notice_tests.rs; role_sponsor_runtime_tests.rs.
- Evidence: B1-sponsor-2.txt (visibility fixture failure); corrected fixtures use team desks with the existing production visibility rules.
