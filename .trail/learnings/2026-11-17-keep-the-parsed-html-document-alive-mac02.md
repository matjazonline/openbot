---
title: Keep the parsed HTML document alive while traversing its handles
author: Mac02
date: 2026-11-17
branch: feature/plan03_04
id: l-a94fc27d
scope: repo
status: active
src: agent
touches:
  - Cargo.lock
  - Cargo.toml
  - assets/app.css
  - docs/departments.md
  - docs/inter_channel_agent_communication.md
  - docs/transport_architecture.md
  - migrations/20260817000000_init_schema.sql
  - plan/04-root-unit/COORDINATION.md
  - plan/04-root-unit/integration/main-60b9a28b/docs/HANDOFF.md
  - plan/04-root-unit/integration/main-60b9a28b/docs/TASK.md
---

## Decided

Keep the RcDom owner in the caller for the complete form-control traversal. Pass a borrowed document to helpers that collect node handles. The already-locked html5ever/markup5ever_rcdom parser is preferable to reconstructing form controls with regular expressions, and native-browser FormData provides a separate check of successful-control and event behavior.

## Tried and ruled out

Returning only cloned Handles from a helper with a local RcDom type-checks and preserves node attributes, but the document root's destruction drains descendant child lists. Inputs therefore still looked valid while every selected option vanished from its select. Debugging department queries would chase the wrong layer: actual browser markup contained the selected options. Holding the document for the complete traversal fixes the ownership error without weakening the required-selection assertion.

## Watch out

A reference-counted node handle does not by itself preserve the document's traversable subtree in markup5ever_rcdom0.3. Its Node::drop explicitly takes descendants' child vectors. Preserve the DOM owner and distinguish a fixture-parser lifetime failure from a production renderer failure. Browser fixtures must also use the actual page shell when checking computed styles: classic base layout and modern workspace layout have different field token overrides.

In the code: src/adapters/http/routes/channel_classic_form_tests.rs form_nodes/controls/check; src/adapters/http/pages/department_ui_tests.rs desk_access_browser_fixtures; scripts/tests/html-injection-browser.mjs. Historical diagnostics: plan/04-root-unit/phases/p02/docs/logs/B1-F1-rendered-handler-tests{2,3}.txt and B1-F1-browser-tests3.txt.
