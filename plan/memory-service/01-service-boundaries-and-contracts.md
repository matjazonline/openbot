# 01 Service boundaries and contracts

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Define a portable memory application. Its public contract must support an independent client
without translating OpenBot concepts or calling OpenBot to authorize ordinary operations.
Implementation continues in [02](02-rust-application-and-storage.md).

## Batch 01A Define ownership and the external contract

1. Give the service an independent Cargo project under `services/memory/`, with its own lockfile,
   migrations, offline SQLx metadata, tests, model artifacts, image, documentation, and CI entrypoint.
   It may use the same maintained crates as OpenBot, but cannot depend on `open_bots`, parent path
   dependencies, private sibling modules, root configuration, or repository-relative runtime files.
2. Give it a dedicated PostgreSQL database and role, even when the PostgreSQL server is shared.
   Neither application reads the other's tables. Share wire contracts and opaque external IDs;
   do not share application models, sessions, cookies, pools, or migration histories.
3. Specify service-owned OpenAPI and MCP contracts for saving observations; search; timeline;
   selected-detail fetch; context packing; session boundaries; generation requests/status; and
   correction, deletion and namespace purge. MCP tools wrap the same authorized use cases as HTTP.
   Do not promise the upstream Claude-Mem payload shapes or endpoints are compatible.
4. Define opaque tenant, namespace, source, session, observation and operation IDs. A namespace
   is an isolation boundary within a tenant, not an encoded OpenBot company/user/agent identifier.
   Source references identify immutable input revisions and carry provenance without requiring
   the service to open the client's filesystem or database.
5. Define explicit bounds for request bytes, text, item counts, timeline depth, candidate scans,
   result bytes, context tokens, concurrent work, provider time, retries and retained data. Specify
   empty results separately from unavailable, unauthorized, invalid and expired operations.
6. Give all writes logical idempotency. Same key and same canonical request return the original
   result; the same key with different content is a conflict. A request retry cannot silently
   create another observation or another paid generation operation.

Focused checks: `memory_contract_` examples cover independent non-OpenBot clients, two tenants,
sibling namespaces, conflicting retries, oversized requests, context exhaustion and explicit
unavailability. Validate both HTTP and MCP examples against their schemas.

## Batch 01B Define authority and information flow

1. Use service-owned scoped machine credentials for the first release. Provision tenants,
   namespaces and credentials through an operator command or management API with separate
   administration authority. Store only credential verifiers; support expiry and revocation.
   The service does not require OpenBot accounts, an OpenBot login, or a second human identity UI.
2. Bind credentials to a tenant, allowed namespaces, allowed operations and permitted read/write
   access levels. Request fields can narrow grants, never enlarge them. Check the same boundary
   on search snippets, timeline neighbors, selected IDs, context, status and mutation operations.
3. Represent classification as an explicit `access_level` and keep it with provenance in the
   service contract and persistence. Each record has one level inside its namespace; levels are
   not separate namespaces or copies at every readable level. Follow the access-level contract
   below for all saves, including explicit notes, generated observations and session summaries.
4. Filter by tenant, namespace, current visibility, expiry and classification before ranking and
   limiting results. Opaque IDs, similarity, prompt instructions and JSON metadata are not access
   controls. Missing narrow-scope identity must never fall back to a wider namespace.
5. OpenBot retains its live actor, membership, capability, audience, ownership and egress checks.
   Its dedicated adapter constructs requests from those checks and holds the service credential.
   Do not expose that credential through an arbitrary agent-controlled HTTP/MCP connection.
   A standalone client follows its own authority policy within its service credential grants.
6. Define tenant disable/delete and credential revocation under concurrent reads, generation and
   saves. A disabled tenant cannot admit new work; deleted namespaces cannot be recreated by a
   late delivery. Retain a minimal deletion barrier independent of removed memory content.

Focused checks: `memory_authority_` proves foreign IDs and caller-supplied tenant/scope/level fields
cannot expand authority; revocation and deletion stop delayed work; and namespace isolation holds
even when one client legitimately has access to more than one namespace.

## Access level contract

The service owns an ordered `access_level` in the range 0–9, where 0 is public. Levels restrict
visibility within a permitted namespace; a level never grants access to another tenant or
namespace. The service does not need to understand channel membership to enforce these rules.

- `read_ceiling` is the maximum level authorized for the current operation, bounded by credential
  and namespace grants. Return only records with `access_level <= read_ceiling`, including snippets,
  titles, provenance and timeline neighbors. Counts and omission metadata exclude inaccessible
  records. A broad service credential does not override the narrower execution ceiling.
- `consumed_level` is the highest classification of information already exposed to the context
  producing the new memory. It includes classified current input, instructions/context, history,
  recalled snippets/details, tool results and inherited continuations. Candidates discarded before
  exposure do not count. Once information was exposed, later omission or a claim that the model
  did not use it cannot lower the watermark.
- Use the proven consumed level when precise consumption evidence is complete. Otherwise use the
  producing context's trusted authorized ceiling as the conservative consumption watermark.
  `save_level = max(consumption watermark, known contributing source levels)`. A ceiling-6 context
  with proven consumption at 4 saves at 4; missing precise evidence saves at 6. Freeze the
  producing context's ceiling with its inputs; a later lower request/audience ceiling cannot
  lower this fallback. The trusted client runtime supplies these facts; the service checks its
  own source records and any generation it performs. An authenticated writer must be trusted to
  attest its external context: the service cannot reconstruct another application's model history
  from a request containing text alone. The fallback ceiling is an enforced bound on that
  producing context, not an arbitrary requested number or a substitute for namespace/source
  permissions.
- The model cannot choose these authority fields. A host adapter supplies or overrides them from
  trusted execution state. Reject inconsistent classifications, out-of-range levels, writes
  outside current grants, and any known source above the producing context's authorized ceiling.
  Missing precise consumption evidence uses that trusted ceiling, not level 0; if the ceiling
  itself is unavailable, refuse the save. An explicitly proven public input can save at 0.
- Store one observation at its save level. Do not copy it into levels 0 through that level.
  Summarization, correction, cross-namespace copying, deduplication, embedding/index retries and
  delivery retries cannot declassify its source information. A retry retains the frozen label;
  changing the label under the same logical operation is a conflicting request.

| Read ceiling | Highest consumed level | Saved memory level |
| --- | --- | --- |
| 6 | 4 | 4 |
| 6 | 6 | 6 |
| 6 | Unknown; precise evidence missing | 6, using the producing context's authorized ceiling |
| 6 | 7 | Refuse; the producing context exceeded its authorized ceiling |

A later audience/permission reduction cannot relabel an existing context or save at a lower level.
Reauthorize before further consumption or disclosure and refuse an unsafe continuation. Future
lower-clearance runs may use only their own authorized context. More permissive audiences do not
raise an already admitted run's ceiling automatically.

Focused checks: `memory_access_level_` covers ordered reads across all ten levels, namespace
isolation despite matching levels, ceiling-6/consumed-4 saves, inherited level-6 context with only
level-0 recalled notes, missing precise evidence falling back to 6, forged lower labels, missing
trusted ceilings and replayed labels. Verify
the same denials across HTTP, MCP, generated summaries and correction/copy operations.

## Product decisions before dependent implementation

The defaults above define application separation, not an approved operating budget. Record the
first generation provider/model, embedding artifact/model, deployment region, data retention,
per-tenant spend limits and representative traffic/latency targets before their dependent work.
Choose only the provider needed for the first deployment; a general provider plugin framework is
outside scope. Preserve the ability to save and search explicit memories without LLM generation.

The native service uses explicitly requested server generation over complete bounded activity
windows. It does not claim to resume Claude-Mem's local stateful observer from isolated events.
The lifecycle rules for those server operations are specified in
[04](04-observation-generation.md).
