# 03 Retrieval and context

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-rust-application-and-storage.md). An independent HTTP or MCP client can search
authorized memories, inspect nearby context, fetch selected details, and request a bounded context
pack. Observation generation is added in [04](04-observation-generation.md).

## Batch 03A Implement keyword and semantic retrieval

1. Use PostgreSQL's native full-text search for keywords and pgvector for meaning. Combine bounded
   ranked candidates with reciprocal rank fusion, keeping exact identifiers and paraphrases useful.
   A separate vector server or an additional BM25 extension is not required for the first release.
2. Use a pinned local embedding model and tokenizer with explicit model identity, vector width,
   normalization and document/query conventions. Adapt the offline artifact-validation pattern in
   [OpenBot's embedding adapter](../../src/adapters/embedding/mod.rs) without importing its private
   code or requiring its runtime/configuration. Package and validate assets independently.
3. Split long observations into bounded meaningful parts so the model does not silently truncate
   the information being indexed. Rank parts, deduplicate observations, and retain provenance.
   Never compare vectors from different models; incompatible model/schema changes reset the
   service database under this repository's policy.
4. Apply tenant, namespace, classification, deletion and expiry predicates in both retrieval legs
   before candidate limits. A ceiling of 6 reads levels 0–6 within authorized namespaces and cannot
   expose levels 7–9 through snippets, source metadata, counts or timeline expansion. Handle
   approximate-index filtering without silently returning no
   results merely because another tenant filled the candidate window. Bound all expansion work.
5. Keep stored-text success distinct from indexing failure. An explicit save can be durably
   accepted with a visible pending/failed indexing state; keyword recall remains possible.
   Retrying indexing uses only already-saved authorized content, never reruns observation
   generation. Do not silently report successful semantic indexing when it failed.

Focused checks: `memory_search_` uses a fixed corpus with exact IDs, paraphrases, long notes,
duplicate parts, non-English text and empty matches. Include foreign-tenant/namespace/classification
rows nearest to the query, deleted records, invalid vectors, failed indexing and model mismatch.
Assert ceiling-6 recall includes eligible level-0/4/6 records and is unchanged by level-7/9 records.
Run a small real-model retrieval acceptance check in addition to deterministic unit fixtures.

## Batch 03B Implement progressive recall and context packing

1. Search returns a compact index with IDs, bounded titles/snippets, timestamps, classification and
   source references. Timeline returns a bounded authorized neighborhood. Selected-detail fetch
   accepts a bounded batch of IDs and repeats all authority checks, regardless of prior search.
2. Expose equivalent HTTP operations and MCP tools for search, timeline and selected details.
   Both transports call the same use cases, with the same current grants, body/result bounds and
   audit behavior. Write tools, if exposed, require separate write authority.
3. Context packing selects useful observation/summary items and enforces both token and byte
   budgets. Include identity, provenance and classification alongside text. Row limits alone do
   not bound prompts. Define deterministic ordering, selection and omission semantics.
   Return the highest level of all text/metadata exposed in the pack, including compact snippets.
   Do not taint it with candidates discarded before exposure or substitute the requested ceiling
   for the actual returned level. The client combines this with its prior consumed watermark.
4. Return structured data suitable for an arbitrary agent host. Keep system-prompt construction,
   provider-specific message roles, workflow checkpoints and recipient selection out of this
   service. Stored instructions remain ordinary untrusted memory content.
5. Define revision/expiry evidence so a client can freeze a context pack and later check whether
   it is still authorized and valid. Do not promise that deleting a service row can retract text
   a client has already sent to a model; clients own their retained snapshots and model requests.

Focused checks: `memory_recall_`, `memory_context_` and `memory_mcp_` cover progressive disclosure,
cross-scope ID injection, restricted timeline neighbors, exact budget boundaries, escaping,
snippet/metadata classification, filtered candidate counts, deterministic packing and equivalent
transport denials. A second non-OpenBot test client must
complete save, search, selected fetch and context assembly using only the service contract.
