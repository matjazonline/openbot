# 02 — Company MCP connections

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Depends on 01. Configure each company through the existing MCP connection API/UI and encrypted
credential lifecycle. Extend the responsible existing owner only where the compatibility probe
demonstrates a missing behavior; do not add a parallel Doop settings/secret system.

## Batch 02A — Register and verify a company connection

An authorized company administrator creates a connection such as `doop`, using the approved HTTPS
`/mcp` endpoint and that company's bot bearer key. Treat identical slugs in different companies as
independent. Use current endpoint validation, DNS pinning, redirect handling, TLS, secret redaction,
and company-qualified encrypted storage. For an internal origin, use the exact operator-owned
endpoint exception already supported by MCP; do not disable SSRF protection globally.

Use `get_profile` and bounded canvas/workspace probes to verify the expected bot and workspace
before publishing workflows. Authentication failure, unavailable Doop, wrong bot, and missing
workspace are distinct errors. Never switch to another company's key or a broad fallback account.
Setup verification is not a substitute for Doop's continuing access enforcement from 01.

Keep workspace selection and endpoint changes under existing connection/publication revisions.
Changing the workspace requires a new reviewed workflow configuration/binding; endpoint identity
changes follow the current connection replacement rules. Never expose token bytes in catalog
responses, HTML, logs, examples, workflow source, model prompts, or stored tool-result fixtures.

**Focused checks:** existing MCP credential/connection/redaction and endpoint-policy tests plus
new `doop_connection_` cases. Use real PostgreSQL to prove company A cannot read, update, rotate,
bind, or dispatch company B's connection, including when both are named `doop`.

## Batch 02B — Review the smallest operation catalog

Discover tools through the ordinary MCP path and register only operations used by the document
workflow. Freeze the actual reviewed schemas, retain schema-drift rejection, and grant tools
explicitly. Include the Doop guide/profile and necessary canvas/frame reads, writes, and bounded
review screenshots. Keep canvas creation in the deterministic workflow action; do not grant it
to the editing agent. Do not grant deletion, arbitrary imports, external integrations, unrelated
uploads, or public publishing merely because the server advertises them.

Preserve existing write classification, approval requirements, operation revisions, connection
access levels, and revocation checks. Tool names/descriptions and `readOnlyHint` are untrusted
metadata, not authority. Make declared result shapes account for Doop's text/structured/error
envelopes without weakening generic MCP validation for other vendors.

**Focused checks:** `doop_catalog_` fixtures for missing/changed schemas, unknown tools, rejected
grants, error envelopes, secret echoes, and oversized discovery/results. Run affected existing MCP
publication/dispatch tests, including revocation or key replacement between preparation and send.

## Exit criteria

Two companies can independently configure the same endpoint without sharing identity or secret
scope. Existing MCP authoring and transport protections remain in force; no Doop-specific transport
branch or unimplemented vault dependency has been introduced.
