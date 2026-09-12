# Phase 1 — `ExternalReplyHandling` configuration (no behaviour change)

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. The line references, the
naming-collision table and the schema conventions there are what this phase edits against.

**Goal.** A company has a reply-handling default and a channel may override it or inherit it live.
The value round-trips through the database, a pure resolver, a port, a settings page, both form
boundaries and the JSON API. **Nothing reads the effective policy to make a decision yet** — Phase 2
is the only phase that changes what happens to a message. Every existing and new company resolves to
`Automatic`, which is exactly today's behaviour.

This is a deliberately separable phase. It mirrors `ExternalResponseReview`
(`src/domain/entities/response_draft.rs:53-85`), whose company column, nullable channel override,
resolution helper, policy page and form/JSON round trip already exist and are the working model to
copy. Landing it first means the hold gate in Phase 2 has a configuration to read instead of
inventing one, and it can be exercised — inheritance, both overrides, migration defaults — before any
message behaviour depends on it.

**Files touched**

| File | Change |
|---|---|
| `migrations/20260817000000_init_schema.sql` | `companies.external_reply_handling`, `channels.external_reply_handling_override`, two `CHECK`s |
| `src/domain/entities/thread_handoff.rs` | **new**: `ExternalReplyHandling`, `ExternalReplyHandlingPolicy`, the pure resolver |
| `src/domain/entities/mod.rs` | register the module |
| `src/application/thread_handoff.rs` | **new**: `ThreadHandoffPolicyPersistence` port |
| `src/application/mod.rs` | register the module |
| `src/application/use_cases/thread_handoff.rs` | **new**: `ThreadHandoffUseCases` façade over the port |
| `src/application/use_cases/mod.rs` | register the module |
| `src/adapters/persistence/thread_handoff.rs` | **new**: the three statements + `effective_reply_handling_on` |
| `src/adapters/persistence/thread_handoff_tests.rs` | **new**: DB tests for the round trip |
| `src/adapters/persistence/mod.rs` | register the module |
| `src/application/use_cases/company.rs` | `CompanyWrite.external_reply_handling` |
| `src/adapters/persistence/company.rs` | the column in the four statements that already carry `external_response_review` |
| `src/adapters/http/routes/company.rs` | `CompanyForm` and `CompanyJsonPayload` fields |
| `src/adapters/http/routes/ui_companies.rs` | the company settings form field |
| `src/adapters/http/routes/ui_thread_handoffs.rs` | **new**: the policy page and its two POSTs |
| `src/adapters/http/pages/thread_handoffs.rs` | **new**: `reply_handling_policy_page`, `reply_handling_policy_saved` |
| `src/adapters/http/pages/mod.rs`, `src/adapters/http/routes/mod.rs` | register page + router |
| `src/adapters/http/app_state.rs` | `Arc<ThreadHandoffUseCases>` and its `FromRef` |

---

## 1.1 Schema

Two columns, both edited into the squashed init migration in place. Recreate both databases
afterwards (see the general file).

**`companies`** (init migration 2055-2072). Add the column at the end of the column list, beside
`external_response_review` at line 2067:

```sql
    external_reply_handling text DEFAULT 'automatic'::text NOT NULL,
```

and the constraint in alphabetical position — `companies_external_reply_handling_check` sorts
**before** `companies_external_response_review_check` at line 2070:

```sql
    CONSTRAINT companies_external_reply_handling_check CHECK ((external_reply_handling = ANY (ARRAY['automatic'::text, 'manual_handoff'::text]))),
```

`DEFAULT 'automatic' NOT NULL` is what satisfies the README's "keep existing automatic-send
behaviour as a migration default": every company that exists and every company created without
naming the field answers automatically, as today.

**`channels`** (init migration 812-841). Add beside `external_response_review_override` at line 835:

```sql
    external_reply_handling_override text,
```

and, alphabetically before `channels_external_response_review_override_check` at line 839:

```sql
    CONSTRAINT channels_external_reply_handling_override_check CHECK (((external_reply_handling_override IS NULL) OR (external_reply_handling_override = ANY (ARRAY['automatic'::text, 'manual_handoff'::text])))),
```

`NULL` means **inherit the company policy live** — not "automatic". A company that switches its
default must move every inheriting channel with it, which is why the effective value is resolved with
`COALESCE` at read time and never copied into the channel row. Add a
`COMMENT ON COLUMN public.channels.external_reply_handling_override` saying exactly that, in the
comments section of the migration.

No index. Both columns are only ever read through a primary-key lookup of the channel and its
company, which is the plan `effective_review_required_on` already gets for the sibling columns
(`src/adapters/persistence/response_review.rs:161-187`). The README's "add indexes only from
representative query plans" applies.

## 1.2 The domain enum and the pure resolver (`src/domain/entities/thread_handoff.rs`)

A new module, named for the concept Phase 2 fills in, so that the file itself is the reminder that
this is not `manual_handoffs`. Open it with a module doc saying so.

```rust
//! Whether an outside reply to an existing thread runs the agent or waits for the team.
//!
//! Not to be confused with `manual_handoffs` (see `entities::attention::NewManualHandoff`), which
//! is a generic, human-created work item with no source message and no generation.
```

`ExternalReplyHandling` copies `ExternalResponseReview`'s shape exactly
(`src/domain/entities/response_draft.rs:53-85`): `#[derive(Debug, Clone, Copy, Default, PartialEq,
Eq, Serialize, Deserialize)]`, `#[serde(rename_all = "snake_case")]`, `#[default] Automatic` plus
`ManualHandoff`, a `const fn as_str()` returning `"automatic"` / `"manual_handoff"`, a `FromStr`
whose error is `format!("invalid external reply handling policy '{value}'")`, and one predicate:

```rust
/// Whether an eligible outside reply waits for the team instead of running the agent.
pub const fn holds_outside_replies(self) -> bool {
    matches!(self, Self::ManualHandoff)
}
```

A named predicate rather than `== ManualHandoff` at each call site, for the reason
`ExternalResponseReview::requires_review` exists: the call sites read as the rule they enforce, and
a third variant later is a compile error in one place.

`ExternalReplyHandlingPolicy` mirrors `ResponseReviewPolicy`
(`src/application/use_cases/response_review.rs:358-364`) — `company_default`,
`channel_override: Option<…>`, `effective` — but lives in the **domain** rather than beside the port,
because it carries the resolution rule:

```rust
impl ExternalReplyHandlingPolicy {
    /// `None` inherits the company default live; a stored override wins.
    pub const fn resolve(
        company_default: ExternalReplyHandling,
        channel_override: Option<ExternalReplyHandling>,
    ) -> Self { … }

    pub const fn effective(&self) -> ExternalReplyHandling { self.effective }
    pub const fn is_inherited(&self) -> bool { self.channel_override.is_none() }
}
```

`resolve` is `const`, takes already-loaded values, and has no `self`, no `async` and no persistence —
the "pure decisions, separately" rule in `src/AGENTS.md`. It gets unit tests with no mocks and no
database, and Phase 2's eligibility decision calls it rather than re-deriving `COALESCE` semantics.

Register the module in `src/domain/entities/mod.rs`.

## 1.3 The port (`src/application/thread_handoff.rs`)

A new application-boundary module beside `attention.rs` and `notification.rs`, registered in
`src/application/mod.rs`. Phases 2-4 extend this same trait; Phase 1 declares only the policy
surface:

```rust
#[async_trait]
pub trait ThreadHandoffPolicyPersistence: Send + Sync {
    /// The company default, the channel's override, and the effective value, in one round trip.
    /// `None` when the channel does not exist in that company.
    async fn reply_handling_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ExternalReplyHandlingPolicy>>;

    async fn set_company_reply_handling(
        &self,
        company_id: Uuid,
        policy: ExternalReplyHandling,
    ) -> AppResult<()>;

    /// `None` clears the override back to inheriting the company default.
    async fn set_channel_reply_handling_override(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalReplyHandling>,
    ) -> AppResult<()>;
}
```

No default method bodies: `src/AGENTS.md`, "Preserve dependency direction" — a port trait must not
give correctness operations a silently-successful default, so every implementation and test double
states its behaviour.

`src/application/use_cases/thread_handoff.rs` holds `ThreadHandoffUseCases { persistence:
Arc<dyn ThreadHandoffPolicyPersistence> }` with three thin forwarding methods, exactly as
`ResponseReviewUseCases` does at `src/application/use_cases/response_review.rs:452-495`. Wire
`Arc<ThreadHandoffUseCases>` into `AppState` with a `FromRef` beside the one at
`src/adapters/http/app_state.rs:192`.

## 1.4 Persistence (`src/adapters/persistence/thread_handoff.rs`)

Three runtime statements plus one helper Phase 2 will call from inside a transaction.

`reply_handling_policy` is `review_policy`'s shape
(`src/adapters/persistence/response_review.rs:567-597`) minus the reviewer column:

```sql
SELECT company.external_reply_handling, channel.external_reply_handling_override
  FROM companies AS company
  JOIN channels AS channel ON channel.company_id = company.id
 WHERE company.id = $1 AND channel.id = $2
```

Parse both with `FromStr` and `map_err(AppError::Internal)`, then build the policy through
`ExternalReplyHandlingPolicy::resolve` rather than re-implementing `unwrap_or`. A stored value the
enum does not know is an `Internal` error, not a default — `src/AGENTS.md`, "Don't collapse errors
into defaults on authorization paths": this value decides whether a customer's message is answered.

`effective_reply_handling_on(tx, company_id, channel_id) -> AppResult<ExternalReplyHandling>` is the
transaction-scoped twin of `effective_review_required_on`
(`src/adapters/persistence/response_review.rs:161-187`), a `pub(crate) async fn` taking
`&mut Transaction<'_, Postgres>`:

```sql
SELECT COALESCE(channel.external_reply_handling_override, company.external_reply_handling)
  FROM channels AS channel
  JOIN companies AS company ON company.id = channel.company_id
 WHERE channel.company_id = $1 AND channel.id = $2
```

`None` from the query is `AppError::NotFound("Channel not found.".into())`. It exists in Phase 1 so
the statement is written, prepared and tested once. Add a test that executes it (see Tests) — an
unexecuted runtime statement is an untested one.

**Note the revision in Phase 2 §2.4.** This section originally said Phase 2 would call
`effective_reply_handling_on` from inside the inbound commit. Phase 2 does not, for a reason that
only becomes visible once the hold exists — a commit that re-decided a hold it was handed could only
choose between two worse outcomes. The statement stays, tested here; Phase 2 §2.4 carries the
argument.

The two setters copy `set_company_review_policy` and `set_channel_review_policy`
(`src/adapters/persistence/response_review.rs:599-651`), including `rows_affected() != 1` →
`NotFound`. The channel setter binds `policy_override.map(|policy| policy.as_str())` **directly**:

```sql
UPDATE channels SET external_reply_handling_override = $3 WHERE company_id = $1 AND id = $2
```

Not `COALESCE($3, external_reply_handling_override)`. See §1.5.

Put the tests in a sibling file with `#[cfg(test)] #[path = "thread_handoff_tests.rs"] mod tests;`,
as `attention.rs:1126-1128` does — Phases 2-4 add to this file and it will cross the 500-line rule.

## 1.5 The `COALESCE` trap — do not copy it

`ChannelPersistence::update` writes the sibling override as

```sql
external_response_review_override = COALESCE($12, external_response_review_override)
```

(`src/adapters/persistence/channel.rs:532`). That makes `None` mean "leave it alone", so **the
review override can be set but never cleared** through the channel form: there is no way back to
"inherit". The same pattern on `companies.external_response_review`
(`src/adapters/persistence/company.rs:363`) is correct, because that column is `NOT NULL` and `None`
genuinely means "preserve".

Two consequences for this phase:

1. `external_reply_handling_override` is **not** added to `ChannelWrite` or to
   `ChannelPersistence::{create, update}` at all. It is written only through
   `set_channel_reply_handling_override`, which binds the `Option` directly and can therefore clear
   it. This is also why the field is not on the `Channel` entity: `Channel` is serialized into
   durable `background_tasks` payloads (see its `#[serde(default)]` comments at
   `src/domain/entities/channel.rs:53-68`), and a policy that must be resolved live has no business
   being snapshotted into a queue row.
2. `external_reply_handling` **is** added to `CompanyWrite`
   (`src/application/use_cases/company.rs:104-114`) as
   `Option<ExternalReplyHandling>`, with the same doc comment as its neighbour — "`None` preserves
   the current value on update and chooses automatic on create" — and the same
   `COALESCE($n, external_reply_handling)` in `src/adapters/persistence/company.rs`'s update
   statement, plus the column in the insert and in the four `RETURNING`/`SELECT` column lists that
   already name `external_response_review` (lines 212-219, 270, 289, 308, 331, 363-370).

Per `src/AGENTS.md` — "when you edit a function that already breaks one of them, don't extend the
violation" — this phase does not fix the review override's `COALESCE`, and does not reproduce it.
Leave a one-line comment at `channel.rs:532` pointing at this file so the next person editing that
statement knows the divergence is deliberate.

## 1.6 Settings surface

**The page** (`src/adapters/http/pages/thread_handoffs.rs`). `reply_handling_policy_page(company_id,
channel_id, policy: &ExternalReplyHandlingPolicy)` is
`response_review_policy_page`'s structure (`src/adapters/http/pages/response_reviews.rs:257-311`):
a header naming the effective value, a `#policy-result` slot, a company-default form and a
channel-override form whose `<select>` carries `inherit` / `automatic` / `manual_handoff`. Differences
worth making:

- the effective line spells out where the value came from — "Manual handoff (channel override)" or
  "Automatic (inherited from company)" — using `ExternalReplyHandlingPolicy::is_inherited`, because
  the whole support question this page answers is "why is this channel behaving like that";
- each option carries one sentence of consequence: `automatic` → "Outside replies run the channel's
  agent immediately.", `manual_handoff` → "Outside replies to existing threads are filed and wait for
  the team.";
- `escape_html_text` / `escape_html_attr` on every interpolation, as the sibling page does.

`reply_handling_policy_saved(company_id, channel_id)` mirrors
`response_review_policy_saved` (`src/adapters/http/pages/response_reviews.rs:316-322`).

**The routes** (`src/adapters/http/routes/ui_thread_handoffs.rs`), a new router merged in
`src/adapters/http/routes/mod.rs` beside `ui_response_reviews::router()` at line 101:

| Route | Handler |
|---|---|
| `GET /ui/reply-handling?company_id&channel_id` | render the page |
| `POST /ui/reply-handling/company` | set the company default |
| `POST /ui/reply-handling/channel` | set or clear the channel override |

All three authorize with the same rule as the review policy pages — a private
`authorize_policy_manager` copying `src/adapters/http/routes/ui_response_reviews.rs:115-128`:
`company_access(user.id, company_id)` filtered on `membership.manages_company_operations()`, and a
`company_not_found()` error otherwise, so a non-manager cannot tell a channel apart from a
non-existent one. The `GET` additionally requires the channel to be readable by the viewer, through
`ChannelUseCases::get_readable_channel` as the mailbox streams do
(`src/adapters/http/routes/ui.rs:850-856`), so a restricted channel is `NotFound` rather than
rendered.

The channel form parses `"inherit"` to `None` and anything else through `FromStr` with
`map_err(AppError::BadRequest)`, exactly as `update_channel_policy` does at
`src/adapters/http/routes/ui_response_reviews.rs:166-170`. Note the contrast with
`parse_review_override` (`src/adapters/http/routes/channel.rs:165-178`), which maps an unrecognised
string to `None` — silently turning a typo into "inherit". Do not copy that; a bad value is a
`BadRequest`.

## 1.7 Form and JSON round trip

**Company.** Add `external_reply_handling: Option<ExternalReplyHandling>` to `CompanyForm`
(`src/adapters/http/routes/company.rs:49-67`) beside `external_response_review`, copy it into the
`CompanyWrite` built at line 94, and add the identical field to `CompanyJsonPayload` and to the
`/ui` company settings form struct at `src/adapters/http/routes/ui_companies.rs:109` and its write at
line 947. `Option` with no `#[serde(default)]`-style coercion, so an omitted field preserves rather
than resets — the neighbouring field's documented contract.

**Channel.** Add `external_reply_handling_override: Option<String>` to `ChannelForm`
(`src/adapters/http/routes/channel.rs:108-118`) and
`external_reply_handling_override: Option<ExternalReplyHandling>` to `ChannelJsonPayload`
(line 231-232's neighbour). Because §1.5 keeps the field off `ChannelWrite`, both handlers apply it
with a second call to `ThreadHandoffUseCases::set_channel_reply_handling_override` after the channel
write succeeds, in the same request. State in a comment that this is two writes rather than one
because the override must be clearable, and that a failure between them leaves the channel saved with
its previous policy — acceptable because the policy is idempotent and re-submittable, and because the
alternative is reproducing the bug in §1.5.

For the channel form, `"inherit"` and the empty string both mean `None`; an unrecognised value is a
`BadRequest`.

**Read-back.** `ChannelResponse` serializes `Channel`, which by §1.5 does not carry the override, so
add a JSON policy pair on the channel API router
(`src/adapters/http/routes/channel.rs:76-96`):

| Route | Body |
|---|---|
| `GET /api/companies/{company_id}/channels/{channel_id}/reply-handling` | `{ "company_default": …, "channel_override": …, "effective": …, "inherited": bool }` |
| `PUT /api/companies/{company_id}/channels/{channel_id}/reply-handling` | `{ "channel_override": "automatic" \| "manual_handoff" \| null }` |

This closes the gap the review policy still has, and it is what makes the round-trip tests below
assertable through the API rather than only through persistence. The sibling review policy is
deliberately **not** given the same pair in this phase (see the general file's out-of-scope list).

---

## Tests

Unit, no database, no mocks — in `src/domain/entities/thread_handoff.rs`:

- `resolve` returns the company default when the override is `None`, for both defaults, and reports
  `is_inherited()`.
- `resolve` returns each override regardless of the company default, and reports
  `!is_inherited()` — including the case where the override equals the default, which is still an
  override and must not be reported as inherited.
- `as_str` / `FromStr` round-trip both variants; `"review_all_external"`, `""` and `"Automatic"`
  are all errors — the enum must not accidentally accept the sibling policy's vocabulary.
- `Default` is `Automatic`, and `ExternalReplyHandling::default().holds_outside_replies()` is false.
- serde round-trips both variants as `"automatic"` / `"manual_handoff"`, and an unknown string fails
  to deserialize.

DB tests in `src/adapters/persistence/thread_handoff_tests.rs`, each scoped to a company it creates:

- **Migration default.** A company created through `CompanyPersistence` with
  `external_reply_handling: None` reads back `Automatic`, and a channel created under it has a
  `NULL` override, so `reply_handling_policy` reports `Automatic` / `None` / `Automatic` /
  inherited.
- **Company default changes, inheriting channel follows.** Set the company to `ManualHandoff`;
  the channel's effective value becomes `ManualHandoff` **without** the channel row changing —
  assert the override column is still `NULL`.
- **Both channel overrides.** Set the override to `Automatic` while the company is `ManualHandoff`
  and to `ManualHandoff` while the company is `Automatic`; the effective value follows the override
  in both directions.
- **Clearing the override.** After setting an override, `set_channel_reply_handling_override(…, None)`
  restores inheritance. **This is the test the `COALESCE` bug in §1.5 would fail**, so it is the
  regression guard for that decision.
- **`effective_reply_handling_on` executes** inside a transaction and agrees with
  `reply_handling_policy` for all four company/override combinations.
- `effective_reply_handling_on` and `reply_handling_policy` for an unknown channel id are `NotFound`
  and `None` respectively; for a channel belonging to **another** company they are the same, proving
  the `company_id` predicate is load-bearing.
- A direct `UPDATE channels SET external_reply_handling_override = 'nonsense'` is rejected by the
  `CHECK`, and a company `UPDATE … = NULL` is rejected by `NOT NULL`.
- Setting the policy on a nonexistent company is `NotFound` (`rows_affected() != 1`).

Route tests, beside the existing channel/company route tests:

- The `/ui/reply-handling` page renders the effective value and marks the right `<option>` as
  `selected` for each of the six company/override combinations.
- `POST /ui/reply-handling/channel` with `policy_override=inherit` clears; with
  `policy_override=manual_handoff` sets; with `policy_override=nonsense` is a `BadRequest` and
  changes nothing.
- A member who does not `manages_company_operations()` gets the company-not-found error from all
  three routes.
- A manager whose viewer has no `view` grant on a restricted channel gets `NotFound` from the `GET`.
- `PUT /api/…/reply-handling` for a channel in another company is `NotFound`; the `GET` after a
  successful `PUT` returns the value written, and a `PUT` with `null` returns to inherited.
- `POST /companies/{id}` and `PUT /api/companies/{id}` omitting `external_reply_handling` leave the
  stored value alone; sending it changes it.
- `PUT /api/companies/{id}/channels/{id}` sending `external_reply_handling_override` applies it, and
  sending `null` clears it.

No existing assertion changes in this phase. If one does, the phase has changed behaviour and
something is wrong.

## Done when

- [ ] Both columns and both `CHECK`s are in the init migration, in `pg_dump` order, the column
      comment is present, both databases are recreated, and `.sqlx/` is regenerated and committed.
- [ ] `ExternalReplyHandling`, `ExternalReplyHandlingPolicy` and the `const fn resolve` exist in
      `src/domain/entities/thread_handoff.rs` with the "not `manual_handoffs`" module doc.
- [ ] The port, the use-case façade and the Postgres adapter exist and are wired into `AppState`;
      `effective_reply_handling_on` is present and executed by a test.
- [ ] The override is **not** on `Channel`, not on `ChannelWrite`, and not written through
      `ChannelPersistence::update`; the clearing test passes.
- [ ] `/ui/reply-handling` renders and both POSTs work, manager-only, with `inherit` handled and a
      bad value refused.
- [ ] Company form and JSON, channel form and JSON, and the `GET`/`PUT` reply-handling pair all
      round-trip.
- [ ] Nothing reads the effective policy to decide anything: `grep` for
      `holds_outside_replies` finds only the enum, its unit tests and the settings page.
- [ ] `cargo fmt --check`, `SQLX_OFFLINE=true cargo build --all-targets`, `cargo test` and
      `cargo clippy --all-targets -- -D warnings` are green.
