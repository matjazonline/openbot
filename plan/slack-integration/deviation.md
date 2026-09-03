## Step 5 — one deviation, deliberately

The plan says email_message_metadata should carry UNIQUE (company_id, rfc_message_id). It cannot: when one channel's agent mails another, the same Message-ID is one outbound message on the sending channel's binding and one inbound message on the receiving channel's, with different bodies, directions and threads. The old schema forced those into one row and then had to demand both writers produce byte-identical content — a coupling whose failure mode is documented in the code it broke. Dedup is therefore (binding_id, external_message_key) only, which is what step 1's architecture contract already states ("the same key text may safely occur in another binding"). I recorded the reasoning in the migration and in docs/transport_architecture.md.

Verification (all clean)

cargo fmt --check, git diff --check, fresh migrations on both databases, sqlx prepare --check, SQLX_OFFLINE=true cargo check --all-targets, cargo test --locked --all-targets (879 passed), cargo clippy --all-targets -- -D warnings, scripts/stack-budget.sh at the stock 2 MiB.

One caveat: an early stack-budget.sh run exited 101 with output suppressed, so I could not identify the test; four subsequent runs passed. It looks like the known shared-database parallelism flake rather than anything in this change, but I could not prove that, so I am flagging it rather than calling it clean. Nothing is committed — say the word if you want a commit.

## Step 9 — three deviations, deliberately

**1. `message_deliveries` requires a channel, an interface and a canonical message, so three
notices stay off the queue.**

The plan's column list makes a delivery "one canonical message through one destination binding",
and the composite foreign keys that prove tenancy need `company_id`, `channel_id`, `message_id`,
`source_binding_id` and `destination_binding_id` all `NOT NULL`. A bounce, a reply from a reserved
`_` address, and an account confirmation code have none of them: the first two answer a message
this deployment *refused* — nothing was stored and no channel was matched, which is what a bounce
is — and all three go out from the deployment's own mailbox rather than any channel's interface.
They stay on the direct `OutboundDispatcher` path, which is fire-and-forget by design: a relay that
is down must not turn an undeliverable message into retried work. The plan already sanctions this
for confirmation mail; I extended it to the other two rather than making four columns nullable for
three messages that have no channel to be attributed to.

Everything *with* a channel behind it is on the queue, including the notices that used to be
fire-and-forget. An approval request and a stop notice are now written as system-authored canonical
messages in the thread they concern, in the transaction that queues their delivery — so a task
parked on an approval nobody was told about is no longer reachable, and the conversation shows that
a human was asked. `ApprovalSubject.thread_id` became a `Uuid` for the same reason.

**2. `retryable` is a claimable status, not a way back to `pending`.**

The plan lists both. A failed attempt lands in `retryable` with its backoff on `available_at`
rather than being reset to `pending`, and the claim takes both. They are one predicate —
`DeliveryStatus::is_claimable`, which the partial index and a test are both derived from — but two
different things for a reader and for the stuck-work census: "queued and never tried" is not "tried
and failed".

**3. Parts are frozen by the producer, not by the worker.**

The plan puts the freeze "before the first provider call" (step 16). I moved it earlier: the
producer renders inside the transaction that creates the state the delivery answers for. That is
what lets the queue row hold identifiers only — the render context is never persisted — and what
lets a producer record *what it sent* before it is sent: `TransportRenderer::predicted_provider_key`
gives mail's `Message-ID` up front, which is how an outreach's question is findable by the reply
that quotes it. `InboundCommitRequest.deliveries` is therefore `Vec<NewDelivery>` rather than
`Vec<DeliveryIntent>`.

The internal channel relay stays inside `EmailSender`, reached before SMTP through the new
`InternalMailRelay` port, rather than becoming a binding-destination delivery. That is a step-16
question (it is the same decision as an email-to-Slack mirror), and moving it now would have meant
resolving `foo@bar.domain` to a binding at plan time with no other mirror to justify the machinery.

### Verification

`cargo fmt --check`, `git diff --check`, fresh migrations on both databases, `cargo sqlx prepare
--check`, `SQLX_OFFLINE=true cargo check --all-targets`, `cargo test --locked --all-targets` (980
passed), `cargo clippy --all-targets -- -D warnings`, `scripts/stack-budget.sh` at the stock 2 MiB,
and a real `cargo run` boot: the delivery worker starts with the email transport registered, polls
and sweeps for a minute with no errors, and `/ui/deliveries` serves.
