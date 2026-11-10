# Review p6a-code-r2: P6a recheck (P5 minors m1–m3 + batch 04A up to durable acceptance), reviewer B gen 3

## Artifact identity

- Reviewer: role B gen 3, nonce `run=08e8e605 role=B gen=3`, agent `afbba0fc2ed0e068e`.
- Frozen full diff `tmp/memory-service-run/frozen/p6a-code-r2.diff`: sha256 recomputed
  `a1113cbc40848db94c02f7c5692abf2a9ab44791587de27f533f285c876f9726`, matches.
- Incremental r1→r2 `p6a-r2-incremental-from-r1.diff`: sha256 recomputed
  `e460e780b7a5f1e355fec39e6e84db9b92a9c4364d8ff85d102a0a464a7153cf`, matches (12 files).
- Worktree tree (temporary index, `tmp/` excluded) `54270382f93f726d4fd566b7798c17d66c4d6381`,
  identical to the frozen tree before and after my verify run.
- Originals read: 04 (04A in full, focused-check list), D4, D5, D8, D9, D13, the P6a
  implementation notes (round-1 resolutions, P6b handoff), review `p6a-code-r1`, the claude-api
  skill bundled with Claude Code 2.1.319 (SKILL.md thinking table, `shared/model-migration.md`
  § Migrating to Claude Sonnet 5.5, `shared/error-codes.md`).
- Code read in full: `domain/generation.rs`, `application/generation.rs`,
  `application/observation_tests.rs`, `adapters/anthropic/mod.rs`, the adapter error test,
  `application/memory.rs` (canonical hash, `authorize_write`, the `sources.record` arm),
  `adapters/postgres/memory.rs::record_sources_once`, the m2 path in `application/recall.rs::pack`
  and `adapters/postgres/recall.rs` (`load_rows`, `candidate`, `pack_bodies_once`), and the
  r1→r2 incremental diff.

## Checks I ran

- `MEMORY_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:55432/postgres services/memory/scripts/verify.sh`
  (root's private cluster, shared target): **all gates passed, 135/135** under macOS network
  isolation (formatting, whitespace, boundary probe, locked fetch, offline check, clippy,
  migrations as owner, `prepare --check`, model manifest and artifacts, tests). All 13
  `memory_observation_` tests ran. Log `tmp/memory-service-run/logs/b3-p6a-verify.log`.
- Redaction probes: a Python model of the pattern set over prose, identifiers and env-style
  secrets, with the decisive cases re-run against the locked `regex =1.13.1` in a throwaway crate
  (built under `services/memory/target/b3-scratch`, then removed). No project file was edited.

## Thinking parameter (m1) — verified

The claude-api skill states for `claude-sonnet-5-5`: omitting `thinking` runs adaptive thinking;
`{type: "disabled"}` is a 400 ("Use thinking.type.between_tools for the lowest thinking
setting"); `{type: "between_tools"}` is "the lowest thinking setting: the model does no extended
thinking", takes no other field (`display`/`budget_tokens`/`block_binding` → 400), is a 400 at
effort `xhigh`/`max`, is Sonnet-5.5-only, and forbids per-message effort changes. The adapter
sends exactly `{"type":"between_tools"}`, refuses efforts other than low/medium/high (and `""`)
at construction, sends no `fallbacks` and no sampling parameters. Claim confirmed. The skill's
default advice (fallbacks, adaptive at low effort first) is deliberately not followed, with
reasons recorded in D9; I agree.

## Recheck of round-1 findings

| Item | Status |
| --- | --- |
| F1 hash over raw request | **Fixed.** `memory.rs` `sources.record`: identifiers checked with `holds_secret`, content redacted, then `authorize_write` hashes the redacted request. Test asserts the stored `request_sha256` equals the redacted canonical hash, a replay across different `<private>`/`password=` text, a conflict for other visible text, and refusal of a secret-shaped ref with nothing stored. Remaining raw-derived persistence: none found (sources hold redacted content and its digest; the idempotency result holds ids/refs/revisions, now refused if secret-shaped; audit details are counts; HTTP logs codes; the DB log is the driver message). |
| F1 cross-effect: byte-exact replay | **Safe.** Requests differing only inside removed text produce identical stored rows, so replaying the stored result is exactly what a fresh execution would yield; D5 states the rule. A redaction-pattern change inside a 7-day window turns a raw-identical retry into a visible 409, never a silent wrong result (acceptable under no-backward-compatibility). The 400 `secret_in_identifier` precedes authorization but is a pure function of the request, so it is no oracle. |
| m1 thinking | **Fixed** (above). |
| m2 bearer/basic prose | **Fixed for the cited cases**; residual prose hits in n1. |
| m3 marker substring | **Fixed**: only an exact `^\[REDACTED:[a-z_]+\]$` value is skipped; `token=abc[REDACTED:x]def` is redacted. |
| m4 identifiers + description | **Fixed**: event ids, refs and revisions refused in generation windows (`generation.rs:170-178`) and refs/revisions in `sources.record`; description level stated in D9 and the field doc. False refusals: UUIDs, SHA-1/SHA-256 hex, git refs, `msg_…`/`toolu_…` ids, `doc:42`, `chat:run-7/event-3` all pass; only keyword-shaped ids are refused (n2). |

## Areas the predecessor did not read

- `observation_tests.rs`: covers every 04A focused-check item for P6a scope — useful window
  (exact request sent, no levels/namespace/refs/key in it), trivial window (valid skip), privacy-only
  input (no call), boundary re-redaction of event text and description, empty/malformed/valid
  skip, truncation, refusal, every provider error class with one call each, model-selected lower
  label (`access_level` field → malformed), ceiling 6/consumed 4 → 4, unknown → ceiling 6, an
  incoming level-6 answer summarized from public text stays 6, excluded events not counted but
  still refusing above the ceiling, missing ceiling, consumed above ceiling, write grant, a
  destination below the level (no call), ordering, in-window revision conflicts, context
  exhaustion (513 events, 600 KiB, allowance), expiry, summary requires a session, cites the
  whole window at the floor, and a summary request answered without one is malformed. Gap (nit):
  a secret-shaped `revision` is not exercised in either path.
- m2 rerun in `recall.rs::pack`: first selection on exact served sizes (`bytes - 2 + body JSON
  size`, correct for the empty `"text":""`), bodies loaded only at the selected `(id, revision)`
  under ceiling/namespace/expiry filters, then `assemble` reselects on loaded items. A corrected
  body bumps the revision, so it is absent rather than resized; the rerun can only shrink the set,
  so both budgets hold. A dropped item counts as `omitted.budget` and skipped candidates are not
  pulled back in: a smaller but valid pack, as D8 says. Correct.
- Outcome tail of `application/generation.rs`: no call when privacy left nothing, destination
  check before the call, `max_tokens` → `output_truncated`, `refusal` → `provider_refused`,
  `end_turn`/other → local validation, `NotSent`/`Rejected` → retryable, `Terminal` → failed,
  `Ambiguous` → unknown; labels from `prepared.label` only; provenance from cited ids via the
  validated subset (the index cannot panic). Correct.

## Findings

No blocking finding.

### Minor

- **m1, `password=`-style pairs with a prefixed key are not redacted**
  (`services/memory/src/domain/generation.rs:112`). The leading `\b` cannot match after `_`, so
  `DB_PASSWORD=hunter2`, `POSTGRES_PASSWORD: s3cret`, `client_secret=…`, `refresh_token=…`,
  `id_token=eyJ…`, `GITHUB_TOKEN=…` (non-`ghp_` value), `MY_API_KEY=…`, `export OPENAI_API_KEY=…`
  and `AWS_SECRET_ACCESS_KEY=…` all pass through unchanged (confirmed with `regex 1.13.1`); URL
  userinfo (`postgres://user:hunter2@db`) is also not covered. Env/config text is common in the
  evidence this service captures, and these are exactly "password=-style pairs" in D9's set. Not a
  regression of r2. Required: let the key take a `[A-Za-z0-9]+[_-]` prefix (keep the whole key in
  the replacement), add a userinfo pattern, and test the forms above. `holds_secret` inherits the
  change (ids like `db_password:1` become refused, consistent with n2). Fix in the next
  correction or the first P6b batch, before acceptance stores windows.

### Nits

- **n1, residual prose redaction** (`domain/generation.rs:56-62, 100`): the credential shape
  counts sentence-final `.` and hyphens, so "the basic responsibilities.", "Basic
  internationalization." and "bearer instrumentation-heavy" are redacted, and `Authorization:`
  takes any 8-character word ("Authorization: required for admins"). Conforms to D9's wording;
  consider ignoring a trailing `.`/`-` and requiring a digit or `+ / =` for Basic/Bearer, and add
  these to the prose test.
- **n2, keyword-shaped ids refused**: `secret:42`, `token:7`, `cmd:pwd:3`, `api-key:prod-rotation`
  and `sk-` + 16 characters get 400 `secret_in_identifier`. Visible and recoverable; say in the
  `event_id`/`external_ref` docs that keyword-prefixed ids are refused.
- **n3, idempotence overstated**: `password="a b"x` → `password=[REDACTED:secret]x` → a second pass
  gives `password=[REDACTED:secret]`. It only ever removes more, so harmless, but the "idempotent"
  doc claim is not strict.
- **n4, P6b**: `between_tools` is Sonnet-5.5-only. `AnthropicSettings.model` is a pub field; if
  P6b makes the model configurable, refuse other models at construction (or send no `thinking`
  for them), and drop the field from any re-send to another model.
- **n5, tests**: no test of a secret-shaped `revision` (generation window or `sources.record`).

## Coverage of 04A (P6a scope)

| 04A item | Status |
| --- | --- |
| 1. Provider-neutral envelope | **Met.** |
| 2. Evidence/save/inference separated; request carries window, namespace, ceiling, consumed exact/unknown, expiry, allowance | **Met for P6a**; acceptance is 04B; `generations.*` stay 501, `MEMORY_ANTHROPIC_*` unread and undocumented. |
| 3. Complete-window generation | **Met.** |
| 4. Typed bounded output, explicit nothing-to-save, empty ≠ malformed ≠ valid skip | **Met.** |
| 5. Privacy before capture/provider, recheck at the boundary, no raw logs, trusted labels, summary scope, destination check, ceiling fallback, higher-source refusal | **Met**, with m1 as a pattern-coverage gap in the redaction set. |

Focused-check list: every named case is covered by the 13 `memory_observation_` tests through
the counted local provider, and the Anthropic adapter only against a loopback fake. No paid call.

## Remaining checks

1. m1 fix and tests (with n1/n3/n5 if taken), then focused `memory_observation_` and
   `memory_storage_`, then `verify.sh`.
2. Carried, CI only: the first `memory-service.yml` run (Linux isolation, model cache, extracted
   copy, `aws-lc-sys`) and the image job. No paid provider smoke (06's open decision).

## Verdict

**Pass for P6a.** F1 and round-1 m1–m4 are resolved; the thinking claim is verified against the
claude-api skill; the three unread areas are correct; 04A items 1–5 and the focused-check list
are met within P6a's scope; verify.sh 135/135 on the frozen tree. One new minor (m1, prefixed
secret keys) and nits n1–n5 are carried and should be fixed before or in the first P6b batch.
