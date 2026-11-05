# HTTP UI source clarification during G1r1 review

Exhaustive current-source inventories http-port-consumers.txt and http-create-exhaustive.txt show HttpConnections only consumed by the production persistence implementation and tests, with no production connection edit/list forms. MCP has existing ui_mcp.rs panes and mcp.rs API. Original P5 still requires company administrators to manage destination levels.

Routine implementation clarification: the independent level-only control uses the existing MCP pane; HTTP needs a minimal bounded company-authorized connection-level list/pane/API, with current joint Owner/Admin facts, expected revisions, strict level-only payloads and the same final atomic cap check. List/detail projections omit credentials. No separate connection registry or execution engine is introduced. Test every new HTTP and MCP endpoint with valid foreign targets, allowed owner/admin changes and unchanged-data/cap refusals.

D1–D4 and D5's gated authorization/write policy do not change. The immutable G1-r1 decision remains the requested costly-contract proposal; this records the corrected routine UI assumption for whole-batch integration review. B was informed before verdict and may flag it if it changes a gated contract. No source implementation has started.
