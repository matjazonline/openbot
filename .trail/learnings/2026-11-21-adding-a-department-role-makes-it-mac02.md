---
title: "Adding a department role makes it a selectable workflow role, so the step help text must name it too"
author: Mac02
date: 2026-11-21
branch: plan08_ph1
cost: { minutes: 19, tokens: 96523 }
id: l-b8a60a2e
scope: repo
status: active
src: agent
touches:
  - plan/08-goals/COORDINATION.md
  - plan/08-goals/phases/p02/docs/TASK.md
  - plan/08-goals/phases/p02/docs/logs/08G-P02-startup-map.md
  - src/application/workflow/registry/help.rs
  - src/application/workflow/publication/role_tests.rs
  - src/application/workflow/reviewer_assignment.rs
  - src/domain/entities/department.rs
---

## Decided
Every value in the department role enum is offered as a workflow role (step `role`, reviewer selectors and escalation `notify` all iterate the full enum). The step help strings list the roles literally, so a new role must be added to them by hand. When the owner and admin roles arrived from main, the help still said "(lead, reviewer, sponsor)". It now lists all five and notes that owner and admin are held on the company root.

## Watch out
A role-list test (help must contain every enum role) and the canonical role-ordering test in reviewer assignment both fail when a role is added without updating these literal lists. Both failures look like unrelated test breakage after a merge; the fix is in the help text and expectations, not in the enum.

In the code: src/application/workflow/registry/help.rs (four "literal role (...)" / "roles (...)" strings); tests src/application/workflow/publication/role_tests.rs `workflow_role_source_sample_and_help_explain_the_same_requirement`, src/application/workflow/reviewer_assignment.rs `root_membership_assignment_canonical_roles_follow_stored_strings`; enum `DepartmentRole::ALL` in src/domain/entities/department.rs.
