These rules apply to code you write *and* to code you touch. When you edit a function that
already breaks one of them, don't extend the violation — extract the part you're changing into a
well-shaped helper and leave the rest alone. Do not treat the longest function nearby as the house
style: `ingest_normalized_message_with_source` and `execute_agent_and_dispatch_inner` in
`src/application/use_cases/thread.rs` are the anti-examples these rules were written from.

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

The seam is almost always already marked. `ingest_normalized_message_with_source` contains
`// Global SPF / DKIM / DMARC ...`, `// Inter-channel hop limit check`, `// ACL & Participant
Restriction Check`, `// Thread Resolution`, `// Thread Turn Limit Check` — each of those comments
is a function that wasn't extracted. **A section comment inside a function body is a naming
opportunity: turn it into a `fn` name and delete the comment.**

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
`thread/ingest.rs` + `thread/dispatch.rs`) along the phase boundaries above.

Inline `#[cfg(test)] mod tests` is the convention here — keep it. But when the test module itself
crosses **~500 lines**, move it to a sibling file (`#[cfg(test)] #[path = "thread_tests.rs"] mod
tests;`), and hoist hand-written mocks (`MockCompanyPersistence`, `MockThreadPersistence`, …) into
one shared `test_support` module instead of re-declaring them per file. The 700 lines of mocks in
`thread.rs` exist because the decision logic isn't pure — following the extraction rule above
deletes most of the need for them.
