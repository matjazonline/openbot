One deviation from the step-5 text, deliberately

The plan says email_message_metadata should carry UNIQUE (company_id, rfc_message_id). It cannot: when one channel's agent mails another, the same Message-ID is one outbound message on the sending channel's binding and one inbound message on the receiving channel's, with different bodies, directions and threads. The old schema forced those into one row and then had to demand both writers produce byte-identical content — a coupling whose failure mode is documented in the code it broke. Dedup is therefore (binding_id, external_message_key) only, which is what step 1's architecture contract already states ("the same key text may safely occur in another binding"). I recorded the reasoning in the migration and in docs/transport_architecture.md.

Verification (all clean)

cargo fmt --check, git diff --check, fresh migrations on both databases, sqlx prepare --check, SQLX_OFFLINE=true cargo check --all-targets, cargo test --locked --all-targets (879 passed), cargo clippy --all-targets -- -D warnings, scripts/stack-budget.sh at the stock 2 MiB.

One caveat: an early stack-budget.sh run exited 101 with output suppressed, so I could not identify the test; four subsequent runs passed. It looks like the known shared-database parallelism flake rather than anything in this change, but I could not prove that, so I am flagging it rather than calling it clean. Nothing is committed — say the word if you want a commit.
