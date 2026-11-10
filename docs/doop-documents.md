# Doop document workflow

Use the pinned private-file build and complete the deployment and release checks in
[Doop testing](doop-testing.md) for your deployment. Use synthetic data until those gates pass.
Complete [connection verification](doop-connections.md) first. The workflow uses the ordinary
format1 compiler, company MCP resources, durable actions, published agents and human decisions.

Publish an agent with slug `doop-document-editor`, instructions from
`integrations/doop/document-editor.md`, a company-approved provider/model and five operations: `get_canvas`, `get_frame`,
`create_frame`, `edit_frame_html`, `get_frame_screenshot`. These are the catalog
`editing_agent` selections except `get_guide`. Select concrete reviewed bindings
from that company's verified connection through normal agent authoring. Attach the three company Skills described below, then publish the agent before
the workflow. Its create/edit operations retain `approval_required: true`.

Create three company Skills using `integrations/doop/document-guide-1.md`,
`document-guide-2.md` and `document-guide-3.md` as their complete instructions, in that order.
Use slugs `doop-document-guide-1` through `-3`, names `Doop document guide 1/3` through
`3/3`, and no required tools. Select all three on the editor draft through normal agent
publication. Together they contain the full pinned guide, with section boundaries preserved;
publication freezes their contents and existing assignment/revocation checks still apply.
The editor instructions restrict this reference to the five granted tools and an unreviewed draft.
Guide examples never grant extra operations or authorize publication or a visual-review claim.

The actual `get_guide` response is one 29,732-character text block (29,947 UTF-8 bytes), exceeding the existing
16,384-character MCP content bound. Each Skill is below the separate 16,384-byte instruction
bound (11,977 / 8,996 / 8,974 bytes). The native model receives every section in its preamble.
The guide callback is a read-only return of static text, with no initialization side effect;
the recipe therefore supplies the reviewed snapshot before editing instead of calling that tool.
The capture and exact provenance are in `src/adapters/mcp/fixtures/doop/guide.json` and
`provenance.json`. When changing the pinned Doop version, recheck the complete guide and replace
all three Skills together, then republish. No limits are raised or guide text silently truncated.
The broader nine-operation catalog and six-operation general editor selection remain available.

Save and publish `integrations/doop/create-document.workflow.json`. Select `get_profile`,
`create_canvas` and `get_canvas` for its `service` MCP resource. Associate the binding with an
explicit company channel that supports email delivery. This is the sender channel for the normal
`message.send` operation; Openbot does not choose a first or default sender. The manual run uses
that same channel association.

Configure and activate the binding with these trusted parameters:

```json
{
  "expected_bot_id": "the verified service bot",
  "expected_workspace_id": "the verified company workspace",
  "doop_origin": "https://doop.example.com",
  "result_destinations": [{"kind": "user", "id": "a current company recipient UUID"}],
  "action_approval": {
    "reviewers": [{"kind": "user", "id": "a current company reviewer UUID"}],
    "deadline": "an ISO8601 UTC deadline for this short-lived binding"
  }
}
```

Set `doop_origin` to the canonical HTTPS origin of the reviewed Doop connection, including its
approved port when needed and excluding `/mcp`, paths, queries, fragments and credentials. The
entry `data.url` step validates and normalizes it before any remote call. An administrator checks
this setting against the saved connection; startup does not perform that remote setup check.
The workflow derives `/c/<verified canvas ID>` from this setting and ignores tool/model URLs.

Select 1–20 explicit user destinations. They retain normal company membership, verified-email,
clearance and egress checks at delivery. For private source material, verify each recipient's
account email and its current company clearance before enabling this binding. Review everyone
who can access the Doop workspace when setting the MCP connection's permitted source level.
Openbot company membership and Doop workspace membership are separate authorization boundaries.

Use an explicit short review window (for example 20 minutes) and refresh the binding's deadline
through normal revision-checked binding configuration for later runs. It is an absolute deadline,
not a rolling timeout. The application validates reviewers and deadline when an action requests
approval. A stale deadline cannot authorize a write. Bind `service` to the same reviewed company
connection used for the published editor; the agent's concrete bindings remain its own authority.

Admit input `{"title":"Project brief","brief":"The bounded document request"}`. Titles are
1–160 characters and briefs 1–8192; extra input properties are rejected. Workspace, canonical origin, recipients, credentials,
approvers and editor selection are unavailable as user input. Classify source content through the
existing knowledge/run owners and configure the connection/model audiences before use; source
classification is never taken from a caller's title or brief. Normal release checks run before
external dispatch and again at final entry, including after approval.

The workflow validates the configured HTTPS origin, reads the profile, verifies the bot's applied workspace, and prepares one
`create_canvas` action using the trusted workspace. Review its frozen arguments in the normal
decision UI before approving. It validates the receipt and a subsequent authorized canvas read
before invoking the editor. The durable action receipt owns the canvas ID across continuations
and worker restarts. Every editor write requests its own approval under the same trusted policy.

The editor reads its attached guide Skills and the canvas, drafts compact HTML frames, and reads each newly created frame. Identical read arguments in one agent execution replay retained receipts; they do not prove freshness after editing an existing frame. The separate final workflow read supplies a new action execution. If fresh final HTML inspection is required for an existing-frame edit, the editor stops with an explicit limitation. Only
targeted unique `old_str` edits are granted; a stale/missing/ambiguous match is a conflict and must
not trigger a whole-document overwrite. Incremental streaming tools are not selected. Existing
provider, round, time, byte, context, rendering and cancellation limits still apply. The final
workflow checks canvas/workspace identity and 1–8 nonempty frames within 1600 by 2400 pixels.

After a fresh final authenticated canvas/workspace check, the workflow sends a normal notification
to the configured recipients under the existing message policy. Its fixed subject and link label keep the title
and model-authored summary out of notification Markdown. The existing delivery worker owns
rendering, dispatch, receipts and recovery. An accepted notification means normal provider
acceptance or an in-app commit, rather than a read receipt.

A successful result has `status: "draft"`, the durable `canvas_id`, trusted `workspace_id`, the
derived `canvas_url`, the bounded actual `title`, a bounded summary and
`visual_review: "not_performed"`. The recipe's 17-step limit covers the explicit composition;
its CI check fails when a graph change outgrows that reviewed bound. The recipe sets the literal
`mcp_error_policy: "fail_step"` on its editor step: a saved MCP error or ungranted tool stops with
`agent.mcp_tool_failed` before another model turn, final output or replay can claim success. The
raw bounded error receipt remains in the step view. Ordinary workflows can select `observe` to
continue presenting MCP errors to their model; this recipe uses the strict setting. The generic agent receives no screenshot
bytes; requesting a screenshot is not visual inspection. This result does not publish the canvas
or approve its content. An editor's `conflict`/`failed` result reported before any MCP error, or a wrong ID, fails the workflow at
`verify_editor`; its committed editor output preserves the reason in the normal step view.

A failed notification can leave a valid private canvas with a failed or waiting run. Inspect the
saved create receipt and final read through the normal run detail, then use the existing delivery
and reconciliation controls for that run. Current membership and clearance checks still apply at
notification preparation and dispatch; the earlier canvas approval grants no recipient access.

After a possible create with no conclusive receipt, the run waits for reconciliation. Do not
re-admit the request, rerun the workflow or reissue a create to recover it: those are new actions.
Use the existing action reconciliation controls and authorized reads to resolve the specific
effect, or leave it unknown for an operator. A non-unique title or absence from a list proves
neither identity nor non-application. Doop supplies no provider idempotency guarantee here. Rejected
approval, failed, cancelled, conflicted and unknown work must retain their actual state.

For an isolated sample, use `integrations/doop/create-document.sample.json` with the normal sample
endpoint/UI, selecting the three MCP contracts above. Adjust each script's `workflow_id` if the
draft ID changed, and use the current reviewer in the synthetic params. The sample explicitly
simulates approval; it does not contact Doop or the model. It exercises source compilation, identity
guards, editor binding resolution and output shape, not rendering quality or live inference.

Create each human's Doop account through normal sign-up. The Doop workspace owner explicitly
invites that account to the company's workspace. The human signs in normally and opens the
returned canonical link to edit the draft. Workspace members can edit workspace content; this
release does not promise a read-only workspace role. An Openbot notification supplies the link
and leaves these Doop permissions with Doop. If the link reports no access, the workspace owner
checks the signed-in account and its invitation. Keep managed public sharing disabled.

For a PDF, open the authorized canvas in Doop, choose **Export frames as a PDF**, select the pages
and use **Download PDF**. The current Doop session authorizes private images, fonts and the full
multipage download. Keep the bytes in that authenticated download path; normal workflow results
and LLM observations carry the canvas link and honest review status. Current membership removal
revokes editor/media access and subsequent downloads. Retained local exports remain subject to
the company's retention policy.
