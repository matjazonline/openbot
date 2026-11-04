# 04 — Knowledge search tool

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

An agent that is granted `search_knowledge` can look up company knowledge up to its run's
ceiling, and nothing above it. When nothing relevant is visible, it says "I don't have that"
(D4). Search combines meaning with exact terms, so product codes and exact model names are found
reliably (P9). This covers P3 and P9.

## Batch 04A — Built-in tool

- Add `search_knowledge` as a built-in workflow tool, following the provisioning built-ins
  (`src/application/workflow/provisioning.rs:25-89`: reserved name, trusted snapshot, schemas).
  Add an `AgentToolTarget::Knowledge` variant (`src/application/workflow/agent.rs:35`).
  - It is read-only, so it dispatches locally, without `ActionService`.
  - It is granted on the agent like every other tool: agent capabilities live on the agent and
    are frozen at publication.
- Input schema: `{ "query": string 1..=1000 chars, "exact_terms"?: [string 1..=64 chars] at most 8 }`.
  There is **no level parameter** (invariant 2), and extra fields are rejected.
  - `query` is the question in natural language, used for meaning.
  - `exact_terms` are the product codes, SKUs, order numbers and exact product names the user
    mentioned, used for exact matching.
- Tool description, written for the model:
  - put every code, SKU, order number or exact product name the user mentions into
    `exact_terms`;
  - answer only from the returned passages, and cite the file name;
  - if the passages do not contain the answer, say you don't have that information;
  - never say that information is restricted or hidden.

## Batch 04B — Search

- Exact terms are the model's `exact_terms`, plus any token in `query` that contains a digit and
  is at least 3 characters long (a pure extraction function). The second source is a safety net
  for when the model forgets to fill `exact_terms`. Each term becomes
  `phraseto_tsquery('simple', term)`, and the terms are OR-combined into one `tsquery`.
- Persistence port: `KnowledgeChunks::visible(company, ceiling, model, exact) -> Vec<StoredChunk>`.
  It runs a **single** query, so the level filter exists exactly once:
  ```sql
  SELECT id, document_name, heading, body, access_level, embedding,
         CASE WHEN $4::tsquery IS NOT NULL AND search_text @@ $4 THEN ts_rank_cd(search_text, $4) END
           AS keyword_rank
    FROM knowledge_chunks
   WHERE company_id = $1 AND embedding_model = $2 AND access_level <= $3
  ```
  (`document_name` comes from a join to `knowledge_documents` on the same company.) It is bounded
  by the per-company cap from 02B, so the scan stays small.
- Ranking is a pure function in Rust:
  1. **Meaning list:** a dot product over normalised vectors, keeping chunks at or above the
     similarity threshold (calibrated in 06A).
  2. **Exact list:** chunks with a `keyword_rank`, ordered by that rank. An exact hit is kept even
     when its similarity is below the threshold, because a code like `AX-4410` looks weak to the
     embedding model.
  3. **Merge** with reciprocal-rank fusion: score `1/(60 + rank)` from each list a chunk is in.
     Return the top `k = 5` (configurable up to 8, enforced).

  A chunk in neither list is dropped, so a weak match becomes "nothing found" rather than a
  misleading passage.
- The result is the document name *without* its level prefix (`sales.md`, not `5_sales.md`),
  the heading path and the passage text, bounded by `MAX_TOOL_RESULT_BYTES`
  (`src/application/workflow/tools.rs:20`). Levels and document ids are not returned: the prefix
  is the level, the model does not need either, and leaving them out keeps the result format the
  same at every ceiling. Citations in replies then never show level numbers.
- Dispatch reads the ceiling from the run row and records the tool observation. In the same
  transaction it calls `raise_content_level` with the highest level among the returned chunks
  (03B). An empty result raises nothing.
- Query embedding goes through the same `TextEmbedder` and semaphore as uploads (02C). A query
  timeout is a retryable tool error, not an empty result.

## Focused checks

Prefix `knowledge_search_`:

- a query that exactly matches a level-5 chunk at ceiling 0 returns nothing from it;
- ceiling 5 returns both layers of `sales.md` (`0_sales.md` and `5_sales.md`), cited as
  `sales.md`;
- another company's chunks are never returned;
- chunks from a different `embedding_model` are ignored;
- the threshold drops weak matches;
- `exact_terms: ["AX-4410"]` returns the AX-4410 chunk above the AX-4401 chunk, even when the
  meaning scores are tied;
- a digit-bearing token in `query` is matched with empty `exact_terms`;
- an exact hit below the similarity threshold is still returned;
- a level-5 chunk containing the exact term is not returned at ceiling 0 (the level filter covers
  both lists);
- more than 8 exact terms, or one over 64 characters, is rejected by the schema;
- non-English text with codes tokenises as expected under `simple`;
- the result bound;
- the observation and the content-level rise commit together (crash between them leaves neither);
- the schema rejects a `level` argument;
- the tool description contains the "don't have that" instruction.

Use the deterministic test embedder from 02C.
