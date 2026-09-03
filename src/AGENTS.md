These rules apply to code you write *and* to code you touch. When you edit a function that
already breaks one of them, don't extend the violation — extract the part you're changing into a
well-shaped helper and leave the rest alone. Do not treat the longest function nearby as the house
style: `ingest_normalized_message_with_source` and `execute_agent_and_dispatch_inner` in
`src/application/use_cases/thread.rs` are the anti-examples these rules were written from. Both are
gone — `thread/ingest/` is what the first one became — but the rules they earned are not.

# Newtypes over bare `String`

When a value is a `String`/`&str` that represents a specific domain concept — and especially
when two such strings of *different* meaning are ever passed around together (as sibling struct
fields, or as adjacent function parameters) — wrap it in a newtype instead of leaving it as a bare
`String`. A bare `String` lets the compiler accept a swapped argument order or a value from the
wrong context silently; a newtype turns that mistake into a compile error.

Reach for this especially when:
- Two `String`-typed values of different meaning are passed positionally to the same function
  (e.g. a company slug and a channel slug to the same lookup call) — the classic argument-swap bug.
- A value is used as a `HashMap`/cache key alongside other unrelated strings built the same way
  (e.g. `.trim().to_lowercase()`), where it's easy to build the key from the wrong source field.
- A value has real parsing/comparison rules of its own (e.g. case-insensitive email matching)
  that currently get re-implemented ad hoc at every call site instead of living on the type.

Existing newtypes live in `src/domain/entities/value_objects.rs`
(`CompanySlug`, `ChannelSlug`, `EmailAddress`, `MessageId`, `ThreadIndex`), built on a shared
`string_newtype!` macro. Each one derefs to `str` (so `.trim()`, `.eq_ignore_ascii_case()`, etc.
keep working unchanged) and implements `Display`/`From<String>`/`From<&str>`, so introducing a new
one is a small, low-risk addition — prefer extending that module over hand-rolling a wrapper.

Don't reach for a newtype for a `String` field that's genuinely generic free text (names,
subjects, message bodies) or that never travels alongside a same-typed sibling — that's needless
ceremony, not safety.

Statuses that arrive as strings are the same problem one level up. A field compared with
`eq_ignore_ascii_case("fail")` at three call sites (`spf_status`, `dkim_status`, `dmarc_status`)
should be parsed into an enum once at the adapter boundary, so the *match* is exhaustive and a
new variant is a compile error rather than a silently-passing check.

# Split a function along the phases it already has

Trigger to act, not to debate: a function past **~80 lines**, past **3 levels of nesting**, or
carrying more than **~6 live `let mut` accumulators**. Any of those means it should be split now,
while you're in it.

The seam is almost always already marked. `ingest_normalized_message_with_source` contained
`// Global SPF / DKIM / DMARC ...`, `// Inter-channel hop limit check`, `// ACL & Participant
Restriction Check`, `// Thread Resolution`, `// Thread Turn Limit Check` — each of those comments
was a function that had not been extracted, and each is now one (`guard_ingress`,
`resolve_addresses`, `authorize_channel`, `resolve_thread`, `exceeds_turn_limit`). **A section
comment inside a function body is a naming opportunity: turn it into a `fn` name and delete the
comment.**

The decomposition that fits this codebase, in order:

1. **Guard phase first.** Cheap rejections that need no I/O (auth headers, hop count, auto-reply)
   move into one `fn` returning `Option<InboundIngestResult>` — or better, a small
   `Result<(), RejectReason>`. The caller becomes a handful of `?`-style early returns.
2. **One iteration, one helper.** A `for` loop whose body is 200 lines is a helper taking one item
   and returning an *outcome enum* (`Matched(..) / Skip / Bounce(..) / Reject(..)`), with the loop
   reduced to a `match` that pushes into the right accumulator. Return an enum, not a bool plus
   three out-params.
3. **Pure decisions, separately.** Anything that takes already-loaded data and returns a verdict —
   "is this sender authorized for this channel", "which recipients are third parties", "does this
   spam score pass" — is a free function or a method on the domain entity with no `async`, no
   `self`, no persistence. Those get unit tests with no mocks at all.

Prefer `let ... else` and early `continue` over another `if let` nesting level. If you're at
column 30, the fix is an extraction, not a rustfmt reflow.

# Keep `async fn` chains shallow

An unoptimized `async fn` poll frame materialises the child future it constructs, so every level of
`await` nesting costs real stack. `process_claimed_task_until_shutdown` was one line —
`self.process_claimed_task_inner(task, Some(shutdown)).await` — and cost **200 KiB**. The
task-worker → dispatch → agent-runner chain reached 1,997 KiB of a 2,080 KiB thread stack and
aborted the process inside serde_yaml, parsing an agent config that was nothing unusual. Optimized
builds cost several times less, so this is a rule about the builds tests and development run on —
which is where it will bite you.

**This qualifies the rule above: splitting one `async fn` into two that call each other adds a
level and makes the stack worse.** What actually reduces it, in order:

1. **Extract a synchronous helper.** A non-`async fn` contributes no future and no frame at all —
   the "pure decisions, separately" rule paying off a second way. `build_agent` went 292 KiB → 174
   KiB by moving everything but its two `await`s into `builder_with_provider` and
   `build_with_tools`.
2. **Delete a level.** An `async fn` that only forwards, or only adapts an argument, is pure cost;
   never add one.
3. **`Box::pin` the seam.** At a call descending into the agent, a provider, or any external
   runtime, the parent then holds a pointer rather than the whole state machine. Say why in a
   comment — these read as noise and get tidied away otherwise.

The chain above is now 347 KiB. `scripts/stack-frames.sh` prints what each frame costs: measure
before and after rather than reasoning about it.

# One decision, one place

Before writing a lookup or derivation, grep the file for it — this codebase's large functions
re-implement the same three things repeatedly:

- building the `Vec<MessageId>` lookup key from `in_reply_to` + `references` (4+ copies),
- deciding `is_trusted_participant` from `channel.participant_emails` + `@public` + team
  membership (3+ copies, in two different functions),
- the memoize-in-a-local-`HashMap` dance for company / channel / membership lookups.

Each copy is a place where a rule can drift out of sync with the others, and the participant one
is an *authorization* rule. Extract to a named helper the first time you'd write the second copy;
when the logic belongs to an entity (participant rules → `Channel`, thread references →
`ParsedEmail`), put it there rather than in a free function in the use case.

Inline memoization specifically: `if let Some(cached) = cache.get(&k) { .. } else { let loaded =
...; cache.insert(k, loaded.clone()); loaded }` should never appear twice. Give it a small struct
(e.g. a `LookupCache` holding the persistence handles and its maps) with `async fn company(&mut
self, slug) -> Option<Company>` methods. That also stops the "which of my five `HashMap`s does
this key belong to" class of bug.

# Name your tuples

`Vec<(Company, Channel, RecipientRole, usize, usize)>` and `Vec<(&ChannelMatch, String,
Option<String>, Option<serde_json::Value>)>` are unreadable at the point of use and force
`|(company, _, _, _, _)|` patterns that break on every field addition. Any tuple with **3+
elements**, or with two same-typed elements (`usize, usize` — step index and total steps, in that
order, hopefully), becomes a struct with named fields. `ChannelMatch` in this same file is the
model to copy.

# No flag parameters

`execute_agent_and_dispatch_inner(&ingest, send_email, task_already_claimed,
Some(worker_id))` is unreadable at the call site and couples two behaviours in one body. When a
bool parameter selects *behaviour* rather than carrying data:

- if the variants share little, keep them as separate public functions with the shared middle
  extracted — not a private `_inner` with a bool matrix;
- if they share a lot, pass one enum (`TaskClaim::AlreadyClaimed(worker_id) |
  TaskClaim::ClaimHere`) that makes illegal combinations unrepresentable — `task_already_claimed:
  false` with `claimed_worker_id: Some(..)` currently compiles fine and means nothing.

Never add a fourth positional parameter to a function that already takes two bools.

# Don't collapse errors into defaults on authorization paths

`.await.ok().flatten()` and `.await.unwrap_or(false)` turn a database outage into a *decision*.
`is_company_team_member(...).await.unwrap_or(false)` reads "on error, treat the sender as an
outsider", and the sibling `get_by_slug(...).ok().flatten()` reads "on error, this company does
not exist" — one of those fails closed by luck and the other silently bounces legitimate mail.

Rule: any fallible call whose result feeds an authorization, authentication, or dedup decision
propagates with `?`. Use `.unwrap_or_default()` only for genuinely optional enrichment (display
names, suggestion lists, telemetry). If you deliberately want a fallback, write the `match` with
an explicit `Err(e) => { warn!(...); <fallback> }` arm so the choice is visible and logged.

# Keep the file and its test module navigable

A 5,000-line module is a merge-conflict magnet and blows past what can be held in context at once.
When a file crosses **~1,000 lines**, split it into a directory module (`thread/mod.rs` +
`thread/ingest/` + `thread/dispatch.rs`) along the phase boundaries above.

Inline `#[cfg(test)] mod tests` is the convention here — keep it. But when the test module itself
crosses **~500 lines**, move it to a sibling file (`#[cfg(test)] #[path = "thread_tests.rs"] mod
tests;`), and hoist hand-written mocks (`MockCompanyPersistence`, `MockThreadPersistence`, …) into
one shared `test_support` module instead of re-declaring them per file. The 700 lines of mocks in
`thread.rs` exist because the decision logic isn't pure — following the extraction rule above
deletes most of the need for them.

# Regenerate sqlx query metadata after touching SQL

**Whenever you add or change a SQL query, regenerate it and commit the result:**

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

This refreshes the `.sqlx/` offline query cache that `cargo build` relies on in environments
without a live `DATABASE_URL` (Fly.io deploy included — see `docs/deploy.md`). A stale `.sqlx/`
cache after a query edit either fails the build there or, worse, silently keeps building against
the old query shape. Requires `sqlx-cli` (`cargo install sqlx-cli --no-default-features --features
postgres,rustls`) and a local Postgres reachable at that URL.

# Bound work at every external boundary

Anything driven by a client, peer, provider, queue row, or stored payload needs an explicit bound.
Choose and enforce the bound at the boundary: body/message and line size, recipient/item count,
connection and worker concurrency, operation and idle timeout, retry/attempt count, prompt-token
budget, pagination, and data retention. Advertising or documenting a limit without rejecting input
that exceeds it is not enforcement.

Make overload produce a controlled rejection, backoff, or terminal state. Do not let the default
failure mode be unbounded allocation, an immortal task, a hot retry loop, or one tenant consuming
the entire worker pool. Defaults exposed on public listeners must be safe for production rather
than relying on every deployment to override them.

# Own background work through shutdown

Do not detach correctness-critical work with an untracked `tokio::spawn`. Keep its `JoinHandle` (or
put it in a supervised `JoinSet`), propagate cancellation into the actual work, and await it during
bounded graceful shutdown. Dropping a `JoinHandle` does not cancel the task it represents.

When shutdown or lease loss interrupts durable work, either release ownership safely or leave a
short, recoverable lease whose expiry consumes an attempt. A deploy must not silently discard a
notification, keep making side effects after ownership is lost, or strand work for a full lease
period without retry accounting.

# Preserve dependency direction

The domain remains independent of application and adapters. The application layer describes the
ports it needs; adapters implement them. Application code must not import transport, framework, or
database implementation types such as `lettre`, `axum`, or `sqlx`, and an abstraction must not live
inside the outer adapter it is intended to abstract.

When touching an inverted dependency, move or introduce the port in the consuming inner layer and
adapt the implementation rather than adding another upward import. Port traits must not use
silently-successful default methods for correctness operations such as authorization, lease
renewal, completion, or durable writes; require every implementation and test double to state its
behavior.

# Make operations traceable without leaking data

Initialize structured tracing before configuration validation, migrations, storage checks, or any
other fallible startup work. Accept or create one correlation id at ingress, echo it where the
protocol permits, and carry it as structured data through the message, task, agent, and outbox
stages. Log identifiers and state transitions as fields, not as opaque interpolated sentences.

Never record secrets, credentials, session or approval tokens, raw authorization headers, full
request structs, or full message bodies in spans. Log addresses and other PII only where the
operational need is explicit and the configured level is appropriate. Metrics needed for alerts
must distinguish retries, terminal failures, lease loss, and stuck work; in-process counters alone
are not durable or multi-instance observability.
