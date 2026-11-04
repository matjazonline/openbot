# 04 — Knowledge search tool

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

An agent that is granted `search_knowledge` can look up company knowledge up to its run's
ceiling, and nothing above it. When nothing relevant is visible, it says "I don't have that"
(D4). This covers P3 and P9.

## Batch 04A — Built-in tool

- Add `search_knowledge` as a built-in workflow tool, following the provisioning built-ins
  (`src/application/workflow/provisioning.rs:25-89`: reserved name, trusted snapshot, schemas).
  Add an `AgentToolTarget::Knowledge` variant (`src/application/workflow/agent.rs:35`).
  - It is read-only, so it dispatches locally, without `ActionService`.
  - It is granted on the agent like every other tool: agent capabilities live on the agent and
    are frozen at publication.
- Input schema: `{ "query": string, 1..=1000 chars }`. There is **no level parameter**
  (invariant 2), and extra fields are rejected.
- Tool description, written for the model: answer only from the returned passages, cite the file
  name, and if the passages do not contain the answer, say you don't have that information.
  Never say that information is restricted or hidden.

## Batch 04B — Search

- Persistence port: `KnowledgeChunks::visible(company, ceiling, model) -> Vec<StoredChunk>`, which
  runs
  `SELECT … FROM knowledge_chunks WHERE company_id=$1 AND embedding_model=$2 AND access_level <= $3`.
  It is bounded by the per-company cap from 02B, so the scan stays small.
- Ranking happens in Rust, as a dot product over normalised vectors. A pure function returns the
  top `k = 5` (configurable up to 8, enforced) at or above a minimum similarity threshold. The
  threshold is calibrated in 06A. Results below it are dropped, so a weak match becomes "nothing
  found" rather than a misleading passage.
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
- the result bound;
- the observation and the content-level rise commit together (crash between them leaves neither);
- the schema rejects a `level` argument;
- the tool description contains the "don't have that" instruction.

Use the deterministic test embedder from 02C.
