# Attribution service

This is an independent Cargo project. Run its Cargo commands here; parent-root
checks do not build or test this service. Source, assets, migrations, configuration
and private types belong here. Connect to Openbot only through the published HTTPS
identity contract and external identifiers; never import its crate or query its DB.

Keep business invariants in the responsible application/domain code, HTTP parsing
and responses in adapters, and SQL/transactions in the PostgreSQL adapter. Add a
layer when behavior needs it; do not create empty ports or worker scaffolding.

No backward compatibility. Incompatible schemas require an operator-managed
database reset; never automatically reset a configured database.

Use focused tests for each implementation batch and `scripts/verify.sh` plus the
standalone/container checks for final verification. Database tests must fail when
their required PostgreSQL fixture is unavailable. Only UUID-prefixed test resources
created by the running invocation may be removed. Keep runtime and libtest stack
budgets aligned at 2 MiB; no silent budget increases.
