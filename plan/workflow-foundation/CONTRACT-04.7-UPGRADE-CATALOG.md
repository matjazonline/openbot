# 04.7 upgrade catalog comparison

Architecture proposal; requires Sol's independent original-criteria/source check
before implementation. Root alone records acceptance. This is test-only scope;
full 04.7 remains incomplete and 04.8 is excluded.

## Original guarantee and evidence

[CONTRACT-04.7-CLAIM-BUDGET.md](CONTRACT-04.7-CLAIM-BUDGET.md):152–169,204–205
requires authentic ambiguous retained pending provenance to block upgrade with
the named preflight error and zero mutation; old command/evidence/audit/attempt/
accounting history must survive. The accepted preparation at
`/private/tmp/workflow-04.7-upgrade-01a0f971/PREPARATION.md` additionally specifies
two comparisons of every public table and complete pg_class/attribute/constraint/
trigger/function rows. This proposal explicitly amends that derived catalog
comparison; it does not claim raw equality is unchanged.

Actual source: `action_reconciliation_upgrade_tests.rs`:234–282 captures those
five catalogs in one query, invokes CURRENT twice on owned connections closed
after each expected error, checks version/23514/exact message and every public
table including migration metadata, then catalog and historical checksums.
Preserve that sequence, baseline, two attempts and connection lifecycle.

The broad run recorded in `/private/tmp/workflow-control-finish-20261003b/CHECKS.md`
was 385 PASS / 1 FAIL at raw catalog equality after named preflight and public-row
checks passed. Its delta was lost to host truncation. **Cause remains unknown.**
The later focused pass is noncausal. Independently measured controls at
`/private/tmp/workflow-catalog-fix-20261003b/controls/` show on PostgreSQL 180006:
ANALYZE changes only relpages 0→1 and reltuples −1→100 with all 100 rows unchanged;
ADD COLUMN changes attributes/relnatts/reltoastrelid; a same-OID function replacement
changes prosrc. Both DDL controls restore full raw equality after rollback.

## Selected boundary

Keep the existing raw snapshot query. Add one small test-only pure comparison
projection that disregards **only values of pg_class.relpages and
pg_class.reltuples in the `relations` group**. Preserve field presence, every
object and object identity, group/array membership and order, and every other
value. Canonicalizing these two existing values to a fixed sentinel is sufficient;
do not rebuild rows from an allowlist or globally remove keys with these names.
New PostgreSQL catalog fields therefore remain strict automatically. Keep raw
snapshots unmodified and use the same pure decision for real-test and control
assertions. Do not derive pass/fail from the bounded diagnostic summary.

| Field | Reason for this individual exception |
|---|---|
| pg_class.relpages | Planner page estimate; ordinary ANALYZE changed it without changing rows or logical schema in the measured control. |
| pg_class.reltuples | Planner row estimate; ordinary ANALYZE replaced the initial unknown estimate with 100 in the same control. |

Both are documented planner estimates updated by maintenance and some DDL in the
[PostgreSQL 18 pg_class reference](https://www.postgresql.org/docs/18/catalog-pg-class.html).
They are not themselves an authoritative row count or schema definition. Their
exclusion cannot excuse a DDL/object change: every remaining catalog value and
every public data row must still match. This is a correction to a demonstrated
false-positive mechanism, not a diagnosis of the lost historical failure.

In particular keep relfilenode, reltoastrelid, relam, reltablespace, OIDs,
membership, ownership/ACL/options, relation kind/persistence, counts, policies,
security/replication/partition fields and all booleans exact. Also keep
relallvisible, relallfrozen, relfrozenxid and relminmxid exact: this bounded decision
does not establish their exclusion. All attribute fields (including statistics
targets/storage/compression), constraints, triggers and function bodies/security/
configuration remain exact. An unexplained or additional maintenance delta fails
and is investigated with complete evidence; it never expands exceptions at runtime.

Do not suppress maintenance in the fixture. That alternative adds preparation and
coordination for already-running/future workers, does not cover every forced
maintenance path, and risks changing the lock environment of the genuine migrator.
No cluster/table autovacuum settings, system-catalog writes, pre-analysis campaign,
held repeatable-read snapshot, migration guards or applied SQL change is needed.

## Diagnostics and implementation scope

Reuse the diagnostic module at
`src/adapters/persistence/workflow/action_reconciliation_upgrade_catalog_diagnostics.rs`.
On **any raw difference**, including one accepted by the projection, retain full
before/after JSON and SHA256 manifest in a unique disposable artifact directory,
with attempt/fixture/server identity, supplementary maintenance context, bounded
keyed raw delta and whether the projected comparison passed. Report its path.
Artifact-write failure must fail explicitly; never silently accept an unrecorded
raw difference. Fail on a projected difference with bounded output, not megabytes
of catalog JSON. Keep diagnostic key uniqueness checks and total/truncated counts.
Supplementary activity/statistics are explanatory only, never an acceptance gate.

Implement within these two existing test files; avoid a general catalog framework
or changes to shared fixture ownership. Fix the seven existing strict-Clippy
diagnostic findings as part of this work. Source remains frozen until independent
preparation passes. No production, migration, dependency, resource-bound, retained
data or queue-acceptance changes are authorized by this document.

## Discriminatory acceptance

1. The existing disposable ANALYZE control must establish a raw difference in both
   named fields, unchanged rows, and equality using the exact projection used by
   the genuine upgrade assertion. Preserve its complete raw artifacts.
2. Both genuine transactional DDL controls must be unequal under that same
   projection: ADD COLUMN (new attribute/shape) and CREATE OR REPLACE body at the
   same function OID (prosrc). Read uncommitted catalogs on the owning transaction.
   After each rollback assert full **raw**, not merely projected, baseline equality
   and unchanged rows, preserving the original rollback-positive discriminator.
3. A small pure guard check must reject added/removed catalog objects, a removed
   exempt field, and an unreviewed changed relation field (e.g. relfilenode) even
   alongside changed estimates. It protects the exact exception boundary; no broad
   synthetic mutation matrix or new DB fixture is required.
4. Run the authentic ambiguous historical upgrade test with both named failures,
   every-public-row/migration-metadata equality and original checksum checks.
   Run the fresh/populated upgrade tests as affected coverage. Synthetic controls
   cannot replace genuine CURRENT migration behavior.
5. Complete required affected/broader checks from HANDOFF-20261003.md: admission
   suite and ten application reconciliation tests, formatting/diff, locked offline
   all-target compilation, strict Clippy, SQLx prepare/check and migration/schema
   evidence with preservation checks. Use stock 2 MiB/default parallelism, explicit
   retained test URLs, complete direct logs and existing owned disposable fixtures.
   Reuse unchanged schema evidence only with recorded identity/checksum agreement.
   Refresh graft after implementation. No limit increase or repeat-until-green.

Sol independently checks these choices against original requirements and actual
code before edits; Astra subsequently reviews the implementation and challenges
the projection against the originals. Any non-exempt runtime delta keeps the gate
failed and requires inspection. Green final evidence can accept this explicitly
amended test mechanism; it cannot retrospectively explain the original failure
or accept other outstanding 04.7 criteria.
