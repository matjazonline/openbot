---
title: Native effect locks must start before shared authority helpers
author: Mac02
date: 2026-11-15
branch: feature/plan03_04
id: l-070f6689
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/ci.yml"
  - ".github/workflows/memory-service.yml"
  - plan/03-principal-addresses/COORDINATION.md
  - plan/03-principal-addresses/phases/p04/docs/HANDOFF.md
  - plan/03-principal-addresses/phases/p04/docs/REVIEW.md
  - plan/03-principal-addresses/phases/p04/docs/TASK.md
  - plan/03-principal-addresses/phases/p04/docs/agent-review-findings.md
  - plan/03-principal-addresses/phases/p04/docs/agent-review-notes.md
  - src/adapters/persistence/workflow/action_account_hints.rs
  - src/adapters/persistence/workflow/action_authority.rs
---

## Decided

The live native effect prefix belongs in the existing purpose-specific sorted account union. Merge the original workflow actor/requester with broker and credential-owner/reviewer accounts before company ownership, preserve the native NOWAIT mode through the outer run acquisition, and revalidate lineage afterward. Select the prefix by the operation being authorized. Reservation entry, live output return and the final credential guard still require current source authority after approval and remote-marker insertion. Structural replay and explicit receipt/history/reconciliation reads keep their separate contracts.

## Tried and ruled out

An inner native source guard with NOWAIT did not prevent the earlier shared facts helper from taking ordinary waiting account/company locks. The role-selector helper test passed while the real mcp_step_request boundary still blocked; the new genuine originating-workflow test and PostgreSQL wait sampling exposed it. An early complete source authorization would take company ownership before an integration credential owner's account and violate the single sorted union. Treating approval or dispatch-row existence as historical observation is also wrong: those rows can precede the first gateway entry. Do not clone or reconstruct an opaque remote reservation just to manufacture an entry retry after a failed consumed permit.

## Watch out

Native local actions deliberately have Workflow origin. FrozenAction::local_agent rejects Agent origin; a role-selected Agent-origin native fixture would require inventing a new producer or bypassing that guard. Obtain a distinct real requester through a genuine application message and MessageAdmissions, and exercise actor/requester/selected-revision contention at actual auth, admission, preparation, fresh-intent, MCP-use, reserved-entry, output-return and credential-final boundaries. Historical unknown recovery after original principal/member removal must still work. Generic checkpoint and policy/history helpers must not acquire a fresh parked-source dependency merely because a run has a native marker.

In the code: src/adapters/persistence/workflow/action_account_hints.rs owns AccountPrefix and explicit live wrappers; native_mcp/accounts.rs observes current lineage; generic prelock_on remains unchanged. local_agent_mcp_source_tests.rs and its entry/integration children use production admission, real approval and three distinct account owners. `cargo test --locked --offline --lib native_mcp_` passed 43 cases with one pre-existing external-CLI ignore after this correction. Plan03 p04 logs/D1-r5.md and reviews 03PA-P04-B1-R2/F1, 03PA-P04-D1-R4/F1 document the counterexamples and accepted decision.
