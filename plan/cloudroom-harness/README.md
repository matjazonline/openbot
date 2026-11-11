# Agent harness configuration for Cloudroom

> **No backward compatibility.** Reset the database when schema or persisted payload changes require it. Do not add legacy readers, backfills or compatibility shims.

Implement Openbot-managed agent instructions, skills, MCP tools and permissions for Cloudroom execution. Respect the existing user preference for full access by default. Configure the harness in Openbot, freeze its selected definition for admitted work, and verify that the native provider receives and can use the configured capabilities.

Phase p01 has three cohesive batches. B1 implements full skill directories across storage, copying,
publication, ZIP import/export and authoring. B2 configures native execution from an existing
Openbot agent, while Local Agents profiles choose the machine/project/provider/model. B3 implements actual MCP calls using
existing company connections and their reviewed tools; credentials remain managed by Openbot.
Scripts and binary assets in skill directories must reach the native harness with relative paths
and executable intent preserved. Full native permissions are the default.

Each batch has one independent architecture gate for its persisted ownership and cross-system
contracts, then whole-batch review. Accept B1 before building B2 on its contracts. B1 storage
alone does not complete native delivery, script execution, MCP calls or permissions setup.

Verification: focused domain/wire and plugin tests; persistence create/update/read, company isolation and frozen-run tests; HTTP/browser authoring checks; real pinned Cloudroom configuration and tool-use checks. Final gate includes formatting, offline all-target compilation, Clippy, fresh migrations, SQLx preparation, database-backed suite and repository CI guards. Record unavailable checks and actual failures explicitly.
