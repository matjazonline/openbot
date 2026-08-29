# Hindsight memory provider — remaining work

## Context

Hindsight landed as a second memory provider alongside HydraDB: `src/adapters/memory/hindsight.rs`
plus the shared transport lifted into `src/adapters/memory/http.rs`, `MemoryProviderKind::Hindsight`,
`HINDSIGHT_*` configuration, the widened `CHECK` constraints, and the provider-neutral settings UI.
653 tests pass, `cargo fmt --check` is clean, and `SQLX_OFFLINE=true cargo build` succeeds.

**What is not done is verification against a real server.** Every Hindsight test drives a localhost
`TcpListener` scripted from the published OpenAPI document (`https://hindsight.vectorize.io/openapi.json`,
"Hindsight HTTP API" v0.9.2). That proves the adapter does what the spec was *read* to mean; it
cannot prove the reading was right. Nothing in this repository has ever exchanged a byte with
`api.hindsight.vectorize.io` beyond an unauthenticated `GET /version`.

HydraDB carries an `#[ignore]`d live smoke test for exactly this gap
(`src/adapters/memory/hydradb_tests.rs:435`). Hindsight has no equivalent. That is the first item
below.

## 1. Add the live smoke test — `src/adapters/memory/hindsight_tests.rs`

Mirror `hydradb_tests.rs:435`: `#[ignore = "requires HINDSIGHT_* and HINDSIGHT_LIVE_COMPANY_ID"]`,
reading configuration from the environment and skipping cleanly when it is absent.

It should walk one full lifecycle against a real instance, because the interesting failures are all
in the parts the mocks cannot reach:

```
provision  -> is_ready  -> persist (company scope)
           -> recall    -> delete   -> is_ready == false
```

Assert on the shape the adapter depends on rather than on recall *quality*: that `provision` is
idempotent when repeated, that `is_ready` flips true then false around `delete`, that a persisted
conversation becomes recallable (allowing for asynchronous extraction — poll with a bounded retry
rather than a fixed sleep), and that `delete` removes every bank under the company prefix.

Use a dedicated throwaway company id. `delete` enumerates by prefix, so pointing this at a real
company's namespace would remove that company's memory.

## 2. Answer the four questions the spec did not

Each of these is an assumption compiled into the adapter today. The first can change the code; the
rest confirm or adjust a bound.

### 2.1 Does retain auto-create a missing bank, or 404?

The load-bearing one. `persist_scope` (`hindsight.rs:230-235`) treats a 404 as "bank does not exist
yet", `PUT`s it, and retries the POST exactly once. Agent and user banks are only nameable once a
message arrives, so this is the sole path by which they are ever created.

```sh
curl -s -o /dev/null -w '%{http_code}\n' \
  -X POST "$HINDSIGHT_BASE_URL/banks/mail-agents-probe-does-not-exist/memories" \
  -H "Authorization: Bearer $HINDSIGHT_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{"items":[{"content":"probe"}],"async":true}'
```

- **404** — the branch is load-bearing and correct as written. Record that in a comment at
  `hindsight.rs:230` so nobody deletes it as unreachable.
- **200/201** — the branch is dead code. Leave it (it is three lines and fails safe), but say so in
  the comment, and note that `provision` creating the anchor bank is then a reachability and
  credential check rather than a structural requirement.
- **Anything else** (400, 422) — the create-on-write design does not hold and agent/user scopes need
  a different path; that is a real redesign, not a comment.

Delete the probe bank afterwards if it was created.

### 2.2 Is `scores.final` populated on a plain recall?

`recall_scope` reads `row.pointer("/scores/final")` (`hindsight.rs:174`) and
`merge_scope_results` falls back to a rank-derived value when it is absent. Scope weighting works
either way, but with the fallback the *within-scope* order is the server's rather than a score, and
no test distinguishes the two — a silent quality difference.

```sh
curl -s -X POST "$HINDSIGHT_BASE_URL/banks/<bank>/memories/recall" \
  -H "Authorization: Bearer $HINDSIGHT_API_KEY" -H 'Content-Type: application/json' \
  -d '{"query":"...","budget":"low"}' | jq '.results[0].scores'
```

If `scores` is null by default, find the request field that turns it on and send it — otherwise
`ResolvedMemoryScope::weight` is being applied to a synthetic number on both sides of the
comparison, which is weaker than it looks.

### 2.3 What is the real `bank_id` limit?

`MAX_HINDSIGHT_BANK_ID_BYTES = 128` (`src/domain/entities/memory.rs`) is a guess. Our worst case is
the User scope at **123 bytes**, pinned by
`the_widest_bank_id_we_generate_stays_inside_the_bound`. Five bytes of headroom on a constant we do
not control.

Probe by `PUT`ting bank ids of increasing length until one is refused. If the real limit is under
123, the fallback is hashing the collection segment to 32 hex — it costs only readability, since
scope attribution comes from which call returned a row, never from parsing the id back.

Also confirm the accepted charset. `validate_identifier` restricts ids to `[A-Za-z0-9._~-]` because
they land in a URL path segment; that is our guard and stays regardless, but a *narrower* server
rule (e.g. no `.`) would matter.

### 2.4 Does `q` filter as documented, and is the org segment always `default`?

`company_banks` (`hindsight.rs:269`) narrows with `q` and then re-filters by the `{database_id}--`
prefix client-side. The prefix check is the safety property and is tested; `q` is only an
optimisation. Confirm it is a case-insensitive substring match as the spec says — if it is a prefix
match or ignored entirely, the pagination bound (`MAX_DELETED_BANKS = 1_000`) becomes the thing
standing between `delete` and a full tenant listing.

Separately, confirm whether the `/v1/default/` segment is fixed or an organization slug. It is
configuration today (it lives in `HINDSIGHT_BASE_URL`), so either answer is already handled — but
the deploy documentation states it as an organization segment and should not be wrong.

## 3. Run the end-to-end pass

With `.env` set to a real instance — cloud, or self-hosted for a run that costs nothing:

```
HINDSIGHT_BASE_URL=https://api.hindsight.vectorize.io/v1/default
HINDSIGHT_API_KEY=…
HINDSIGHT_FAST_TIMEOUT_SECS=15
HINDSIGHT_THINKING_TIMEOUT_SECS=60
```

1. Company settings → Long-term memory → **Hindsight**. The badge should reach `ready` on the first
   worker poll (~1-2s): `provision` is a `PUT` plus a `GET`, with no remote build to wait for.
2. Channel settings → enable retrieve + persist for the Company and User scopes.
3. Send a message through the channel. Wait for extraction, then send a follow-up that depends on
   the first, and confirm the reply uses the recalled context.
4. `GET /banks?q=mail-agents-company-` — expect `…--company` and `…--user_<sha256>`, and **no**
   cross-company ids.
5. Recall directly against `…--company` to see what was actually stored and how it scores.
6. Switch the company back to HydraDB and confirm a `memory_cleanup_jobs` row appears for
   `hindsight` and the banks are gone after the 180s quiescence window.

Step 3 is the one that exercises the whole path — coordinator, scope resolution, bank creation on
write, extraction latency, and recall attribution — and it is the step the mocks approximate least
well.

## 4. Decide on extraction-failure visibility

`persist` sends `async: true`, so a 422 surfaces synchronously but an extraction failure after the
queue does not. `RejectedItem` coverage is therefore thinner than HydraDB's, where the per-item
result is in the response.

`GET /banks/{id}/operations` can report the outcome of the returned `operation_id`. Wiring that into
the durable job tables is a real piece of work and was scoped out; persistence is best-effort for
every provider today. Revisit if the end-to-end pass shows memories silently not landing — that is
the symptom this would diagnose.

## Not remaining (recorded so it is not re-litigated)

- **The two persistence bugs in `plan/shodh_memory_provider.md` §6 are already fixed.**
  `memory_connection()` joins on the selected provider, and `select_provider()` retires the
  abandoned connection and its non-leased provisioning job inside the transaction. Covered by
  `switching_providers_retires_the_previous_connection_and_enqueues_its_cleanup`. Strike that
  section when Shodh is picked up; §§1-5 and 7-10 of that plan remain accurate and are now largely
  already done.
- **Adding Shodh** is one `MemoryProviderKind` variant, one adapter file, one word in the five
  `CHECK` constraints, and one block in `infra/setup.rs`. No route, page, or config-shape edits —
  the provider `<select>`s render from `MemoryProviderKind::ALL` and the string parsing goes through
  `MemoryProviderKind::parse`.
- **`reflect`, mental models, entities, knowledge pages and webhooks** are deliberately unused.
  `reflect` returns an LLM-authored answer rather than chunks and does not fit the port's
  `recall -> Vec<MemoryChunk>` contract.
- **Per-provider runtime metrics.** `runtime_metric_samples.hydradb_*` is a machine-level aggregate
  spanning every provider; the column names are kept deliberately, with a schema comment saying why.

## A note on the shared test database

`switching_providers_retires_the_previous_connection_and_enqueues_its_cleanup` deletes the
provisioning job it queues before returning. It has to: these tests share one database with
`memory_worker`'s, whose workers claim *unscoped*, and a left-behind pending job gets claimed by
whichever worker test runs next — whose registry carries only the provider it is exercising, so the
claim resolves to no provider and that test's own job never runs. It hangs rather than fails.

Cleanup jobs need no such care; the 180-second quiescence window keeps them unclaimable well past
the end of a run. Any future test that selects a provider needs the same cleanup.
