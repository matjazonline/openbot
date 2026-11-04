# 06 — Integration, verification, and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on every earlier phase. Prove the sales-document journeys end to end and close with the
full-suite gate. This file defines future checks; saving the plan does not mean they have run.

## Batch 06A — End-to-end journeys

Add `knowledge_e2e_` tests. They use real persistence, workflow workers, the message runtime and
the delivery worker, with a scripted LLM, the deterministic embedder and a counted transport.
Set-up: `0_sales.md` and `5_sales.md` uploaded; an agent granted `search_knowledge`; a
public support channel.

1. An outside contact asks for the discount floor. The search observation contains no level-5
   passage, and the reply says the agent doesn't have it (D4).
2. A level-5 member asks on a team channel. The reply cites the level-5 passage, is stamped 5,
   and a level-0 teammate sees the placeholder (P7).
3. The member CCs a customer and asks again. The run's ceiling is 0 and the reply uses only
   level-0 content (D3).
4. In the same thread, "summarise this thread for the customer": the level-5 reply is absent from
   history, and nothing at level 5 is sent.
5. A CC is added while a level-5 run is working. The reply is refused, nothing is delivered, and
   attention shows the typed failure (P6).
6. A level-5 run calls a level-0 MCP tool: the tool error reaches the model and no call is made.
   A level-9 CRM connection is allowed (P5).
7. A level-3 member uploads `5_sales.md` and is refused. They upload `3_sales.md`, which adds a
   layer. A second `3_sales.md` overrides it. `sales.md` without a prefix is rejected (D6).
8. Retrieval-quality fixture: 10–20 realistic sales questions, with the expected passage for each
   (multilingual if P3 confirms it). This calibrates the similarity threshold (04B) and decides
   P9.

Competing claimants (each with `own_database`):

- concurrent uploads of one name and level (02);
- the parallel-branch race between content-level rise and release (03);
- a member's level lowered while their run is mid-reply: the release reads the current audience.

## Focused verification during phases

List each filter first and record a nonzero selected and executed count. Run each with
`cargo test --locked --offline --lib PREFIX` against a disposable database.

| Phase | Prefixes | Existing areas to keep green |
| --- | --- | --- |
| 01 | `access_level_` | participant/channel access, invites, member management |
| 02 | `knowledge_upload_`, `knowledge_chunk_` | upload/body limits, value objects |
| 03 | `knowledge_run_level_`, `knowledge_run_level_race_` | admission, history reader, action authorization, message runtime |
| 04 | `knowledge_search_` | agent driver tool dispatch, provisioning built-ins |
| 05 | `knowledge_view_` | page tests, notifications, run views |
| 06 | `knowledge_e2e_` | workflow message thread journeys |

For every SQL-bearing batch, apply migrations to a fresh disposable database, run
`SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`, and commit the `.sqlx/` changes.
Export `TEST_DATABASE_URL` explicitly. Never point these tests at development or production
data.

Run `scripts/stack-frames.sh` before and after 04, because the tool dispatch adds a level to the
agent chain. Keep the chain within `scripts/stack-budget.sh`. Box the seam into the embedder
rather than raising `STACK_BUDGET_KIB`.

## Batch 06B — Final full-suite gate

Run once after all phases are integrated. Treat `.github/workflows/ci.yml` as the authoritative
inventory and reconcile anything added since this plan was written.

```sh
cargo fmt --all -- --check
git diff --check
cargo check --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
cargo test --locked --offline --all-targets --no-run
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
scripts/tests/deploy.sh
```

Also run CI's browser fixture gates, including HTML injection with the new placeholder markup.

The image build must:

- include the checksummed model files;
- start as non-root;
- fail startup when the model is missing.

Add a `scripts/tests/deploy.sh` assertion for this. Record every command, its result, the test
counts and any unavailable check. Do not claim full verification if a required check could not
run.

## Release notes

- Database reset required (new columns and tables in the init schema).
- Image size and memory grow by the embedding model (P3). State the measured numbers.
- Existing members start at level 0 (P1). Owners must assign levels before restricted documents
  are useful.
