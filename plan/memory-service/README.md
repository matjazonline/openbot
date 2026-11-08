# Standalone Rust memory service

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Build a native Rust memory application in `services/memory/`. It runs beside OpenBot initially,
but owns its package, database, credentials, model configuration, API, build, and deployment.
Another application must be able to use it without OpenBot being present. Moving the service to
another host or repository must not require extracting code from OpenBot.

Claude-Mem supplies the behavioral reference: turn useful activity into concise observations and
session summaries; preserve their sources; retrieve a compact index; fetch selected details; and
assemble bounded context. Reimplement those behaviors in Rust rather than translating its entire
TypeScript application. Its coding-agent installers, local filesystem conventions, viewer,
commercial integrations, and wire compatibility are outside this plan.

This is an implementation plan. The service and OpenBot integration are not implemented by these
documents. Execute the numbered phases in order, with focused checks for each batch and the final
full-suite gate in [06](06-operations-and-verification.md).

| Phase | Outcome |
| --- | --- |
| [01](01-service-boundaries-and-contracts.md) | Independent ownership, portable API, authorization and lifecycle contracts |
| [02](02-rust-application-and-storage.md) | Standalone Rust package, PostgreSQL schema, credentials, reproducible build |
| [03](03-retrieval-and-context.md) | Keyword and semantic retrieval, progressive recall, bounded context, MCP |
| [04](04-observation-generation.md) | Explicitly authorized observation and summary generation with bounded cost |
| [05](05-openbot-integration.md) | OpenBot adapter, workflow handlers, agent tools, automatic context |
| [06](06-operations-and-verification.md) | Retention, deletion, standalone deployment proof and final verification |

## Architecture decisions

- Rust/Axum/Tokio/SQLx with PostgreSQL. Use pgvector in the service database for semantic search;
  PostgreSQL full-text search supplies the keyword leg. No Chroma or separate vector database.
- One service binary with HTTP and supervised background work initially. PostgreSQL owns durable
  job state and claims; no Redis, Valkey, BullMQ, or additional queue broker is required.
- The core speaks in tenants, namespaces, access levels, sources, observations, sessions, and
  scoped credentials.
  It does not contain OpenBot company, channel, agent-publication, workflow, or membership models.
- OpenBot is one authenticated client. Its adapter maps company identity to tenant identity and
  authorized company/agent/user memory scopes to namespaces. Other clients choose their own maps.
- Access levels 0–9 filter records within a namespace. Read through the authorized ceiling and
  save at the highest level actually consumed by the context producing the memory. A ceiling of
  6 with proven consumption at 4 saves at 4; consuming level 6 saves at 6. Missing precise
  consumption evidence falls back to the producing context's authorized ceiling, so it saves at
  6 in this example. Summarization cannot lower the label.
  [01](01-service-boundaries-and-contracts.md) defines the portable contract; phase 05 reuses
  OpenBot's existing lowest-human-reader audience and monotone content-level rules.
- Explicit saves work without a generation provider. Automatic extraction is a separate operation
  with explicit authorization, source boundaries, expiry, and a spend allowance.
- Agents receive a small authorized memory pack as untrusted context and may use tools for more
  detail. Stored memory text never becomes system instructions.
- Use a fresh service database. Do not import local SQLite/Chroma state or restore removed
  HydraDB/Hindsight/Shodh integrations as part of this work.

## Reference behavior and current integration anchors

The checkout under [tmp/claude-mem](../../tmp/claude-mem/README.md) is research material, not a
runtime or build dependency. Before implementation, record the source revision being used and
preserve applicable Apache-2.0 attribution for any adapted material. Useful references are its
[server routes](../../tmp/claude-mem/src/server/routes/v1/ServerV1PostgresRoutes.ts),
[PostgreSQL storage](../../tmp/claude-mem/src/storage/postgres/schema.ts),
[hybrid retrieval](../../tmp/claude-mem/src/storage/postgres/observations.ts), and
[observer lifecycle](../../tmp/claude-mem/docs/architecture/observer-lifecycle.md).

OpenBot's [memory contracts](../../src/application/workflow/registry/families.rs) are currently
unavailable at publication/runtime, as recorded in
[workflow composition](../../src/infra/workflow.rs) and
[catalogue help](../../src/application/workflow/registry/help.rs). They are integration seams,
not an implemented memory backend. Its
[agent preparation](../../src/application/workflow/agent.rs) already delimits untrusted context;
[knowledge consumption](../../src/adapters/persistence/workflow/knowledge_checkpoint.rs) shows
how retrieved information must affect run classification before the agent can consume it.
The existing [access-level domain](../../src/domain/entities/access_level.rs) and
[run admission](../../src/adapters/persistence/workflow/knowledge.rs) already distinguish the
read ceiling from the consumed content level. Reuse those semantics in the adapter; implement
the same generic ordered-level contract independently in the service.

Follow the independent application boundary in
[attribution-service](../attribution-service/01-application-boundaries-and-contracts.md).
Historical provider plans describe removed infrastructure and are not current implementation
instructions. Keep this service useful on its own; OpenBot-specific work belongs only in phase 05.

## Verification discipline

Each batch defines behavioral checks. Before running a focused test filter, list it and confirm
a nonzero selection. Record commands, counts, results and unavailable checks in the phase being
implemented. Do not claim a pass when a database or model was absent and the test skipped itself.
Run the complete service and affected OpenBot gates once after integration, as required in phase
06. Every file in this directory carries the database-policy note and must be listed in
`NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`.
