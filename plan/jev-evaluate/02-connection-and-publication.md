# 02 — TypeSafe connection, publication, and binding

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Give a company an explicit TypeSafe connection that a workflow binding can select. Its secret
never enters source, compiled identity, publication bundles, run snapshots, logs, or samples.
Do not add Jev to the agent-selectable `company_model_connections` or the Rig provider list.

## Batches and checks

1. Add a company-scoped TypeSafe connection with stable ID, enabled state, revision, encrypted
   bearer key, and an explicit set of allowed versioned Jev model IDs. Use the credential
   envelope and rotation conventions in `src/adapters/persistence/credentials/`; never accept a
   user-supplied arbitrary API origin. Fix the TypeSafe API origin in the adapter, with a
   loopback-only test endpoint. Add owner/admin create, update, rotate, disable, and read views
   that expose readiness and model IDs but never secret bytes. Change the single init migration
   for a fresh database and regenerate `.sqlx/` if SQL queries change.
2. Extend `ResourceDirectory`, binding readiness, admission, publication preflight, and current
   execution authorization for `kind: typesafe`. Publication freezes the step's model and
   question contract; binding stores only the selected resource ID. Verify same-company
   ownership and selected-model availability at publication/admission, then recheck enabled
   state, actor authority, model allowance, and current credential just before provider use.
   A disabled or removed connection is a terminal refusal, not a route to a replacement
   credential. Credential rotation keeps the resource identity but affects the next authorized
   use. Follow existing authority-before-local lock order.

Focused commands after batch 1: migrate a fresh database with
`SQLX_OFFLINE=false cargo sqlx migrate run`, regenerate with
`SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`, then run
`cargo test --locked --offline --lib jev_connection_` and relevant credential rotation tests.
After batch 2 run `cargo test --locked --offline --lib jev_binding_`, plus existing
`workflow_binding_` and `workflow_publication_` filters. List filters first and record actual
counts/results. Include database tests for cross-company IDs, stale binding revisions, disabled
connections, model removal, rotation, and two claimants competing with a revocation/update.

Gate: a run uses exactly the selected company's current authorized connection; no secret is
serialized into any workflow artifact or shown in an authoring response.
