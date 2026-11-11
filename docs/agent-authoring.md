# Configure and publish a reusable agent

Open **Agents** for the selected company. Edit the agent's model, instructions, ordered skills
and tool assignments, then save the draft. Saving does not publish or deactivate the agent.
The publication panel previews the **saved** draft; save pending edits before previewing or
publishing them. A stale save keeps submitted fields for reconciliation with the newer draft.

Assign capabilities on the agent:

- Built-ins provide message send/reply and knowledge search. Message authority still comes from
  the workflow's actual channel and source message. Ordinary edits cannot add Builder provisioning
  privileges; a trusted Builder copy may retain or drop its existing provisioning bindings.
- HTTP choices identify a company connection and approved operation.
- MCP choices identify a company connection, operation, reviewed version and remote tool name.
  Granting the connection, reviewing discovery, approving an operation and assigning it to an agent
  are separate actions. The selector performs no discovery or remote call.
- Workflow tools identify a registered workflow, immutable version and parameter configuration.
  Repointing a registration does not silently change an existing binding's captured version.

Each agent-visible alias appears beside its actual target. Skills may require particular aliases;
preview reports missing aliases. Renaming an alias does not change what a tool can do. A changed
target or alias receives a fresh binding identity. Keep the existing identity to retain an
unchanged assignment, including a historical workflow pin.

Publish with the draft revision, active revision and request key shown by the panel. A successful
publication updates the active agent revision independently of workflow versions. Future root
admissions select it; queued/running runs, retries and children retain their captured manifest.
Editing shared skill text or an agent draft does not replace those frozen definitions.

An explicit revoke affects the exact binding or skill-assignment identity. Publication that removes
an assignment retires its live grant too. Restoring a connection or registration does not revive
a revoked incarnation. To restore an agent tool, remove it from the draft, select it again, save
and publish a fresh binding. For a revoked skill assignment, remove and save it, then add, save
and publish it again. Deactivation stops new root admissions without changing historical runs.
Current account, model, connection, reviewed-operation and assignment authority still applies to
running work; revocation may stop the next guarded call.

In **Workflows**, choose the model step and company agent, then use **Use selected agent in source**.
This changes unsaved source only; save and validate the workflow afterward. The capability panel
shows current information, not a guaranteed revision for every future run. An agent's HTTP/MCP
tools need no duplicated workflow resource slots. Explicit `http.call` and `mcp.call` steps retain
their own resource requirements. `ai.classify` and `decision.agent` use instructions/model only,
with no tools or skills. Agents execute within workflow model steps; publication does not expose
an unrestricted standalone execution endpoint.

Run details show the immutable manifest, selected agent revisions and workflow dependency versions.
Step details show the actual revision and binding targets; tool-call provenance identifies the
saved action invocation or child call. Child rows name their actual workflow/version. These views
use captured definitions and identifiers, so later renames, publication, repinning or revocation
cannot rewrite history. Restricted run content also hides capability provenance. Dependency and
call-origin projections show at most 50 entries per list and explicitly report truncation; revision
history shows 20 entries per cursor page. Catalogs show 50 choices per page and retain selected
identities outside their page.

## Skill directories

Company owners can import a ZIP from **Skills**. Operators have the same controls in the global
skill library. Include `SKILL.md` at the archive root or inside one enclosing directory, with its
supporting `scripts/`, reference files and binary assets. Its YAML frontmatter needs a `name`
matching the skill slug and a nonempty `description`. Preserve executable file modes when
creating the ZIP; an executable script stays executable on export.

Use **Replace directory** to replace the saved instruction text and all supporting files together.
An invalid upload leaves the existing skill intact. Ordinary metadata edits preserve supporting
files. **Download ZIP** exports the saved directory; the file list shows paths, sizes and
executable flags. Copying a global skill into a company creates independent files. If its slug
already exists, the copied frontmatter uses the new slug too. Published agent revisions retain
the original file bytes and executable flags even after the shared skill changes.

Archives accept at most 64 supporting regular files, 256 KiB per file and 512 KiB total decoded
assets. The ZIP itself may be at most 1 MiB and contain 130 entries, including directories.
Instruction text is limited to 16 KiB; plain text
skills receive generated frontmatter on export. Paths must be portable relative ASCII paths,
at most 240 bytes, without traversal, case collisions or file/directory collisions. Symlinks,
special files, encrypted archives and ZIP64 are rejected. Empty directories are not stored.
Importing a script does not execute it on the Openbot server.

Company JSON skill writes under `/api/companies/{company}/skills` and operator writes under
`/api/skill-library` accept `files` entries shaped like
`{"path":"scripts/check.sh","content_base64":"ZWNobyBvawo=","executable":true}`.
`instructions` remains the canonical `SKILL.md` source: do not duplicate it in `files`. Omit
`files` on update to preserve supporting files, or submit `[]` to clear them. List responses
return file counts; load an individual skill to read its file content. These asset JSON routes
allow bodies up to 2 MiB.

## Company API

Use the existing authenticated company owner/admin session and browser Origin/Referer protection.
Replace illustrative UUIDs with identifiers returned by this company. Reads and writes recheck
current authority; a valid identifier from another company grants no access.

`GET /api/companies/{company}/agents/{agent}/authoring` returns the draft revision, active summary,
ordered assignments and first history page. Catalog endpoints under that agent are
`tool-connections?kind=http|mcp`, `tool-options?kind=builtin|http|mcp|workflow` (with `connection` for
HTTP/MCP operations), and `history`. Catalog responses contain `items` and `next`; pass `next` as
`after` for subsequent options/connections. History uses both `before_created_at` and `before_id`.
`GET /api/companies/{company}/agents/directory` lists scoped agent identities.

Update an existing draft with `PUT /api/companies/{company}/agents/{agent}`:

```json
{
  "expected_revision": 3,
  "agent": {
    "name": "Support",
    "slug": "support",
    "provider": "openai",
    "model": "your-enabled-model",
    "system_prompt": "Answer support questions using the approved tools.",
    "description": "Customer support",
    "skill_ids": []
  },
  "tool_assignments": [
    {
      "binding": null,
      "alias": "knowledge.search",
      "choice": {"kind": "builtin", "tool": "search_knowledge"}
    }
  ]
}
```

Read the saved revision from the response's `agent.draft_revision`. Omitting `tool_assignments`
preserves them; `[]` clears the draft list. Omitting `skill_ids` preserves ordered skill assignments;
`[]` clears them. For an unchanged tool, submit its existing `binding` and exact `choice` returned
by authoring. Catalog `choice` values carry exact HTTP/MCP/workflow identifiers; use those values
rather than fabricating operation names, review versions or workflow parameter digests.
Create uses `POST /api/companies/{company}/agents` with flat agent metadata and creates a draft.
The update contract requires the nested payload and expected revision.

`POST .../{agent}/preview` previews the saved draft and returns revision/count metadata.
`POST .../{agent}/publish` accepts:

```json
{
  "expected_draft_revision": 4,
  "expected_active_revision": null,
  "key": "support-publication-4"
}
```

Use the observed active revision UUID instead of `null` when replacing an active publication.
Retain **all three fields** when retrying the same publication. Reusing its key returns the original
receipt, even if a later publication or deactivation has occurred; it does not reactivate that old
revision. Publication receipts contain revision identity, draft revision and digest, without prompts,
skill text, schemas, raw parameters or credentials.

`POST .../{agent}/revoke` accepts exactly one of `{"binding":"UUID"}` or `{"skill":"ASSIGNMENT_UUID"}`.
`POST .../{agent}/deactivate` requires `{"expected_active_revision":"UUID"}`. On a stale revision,
read current state and reconcile before issuing a new command; do not discard submitted edits or
silently replace the request key of a retry.
