# Verification contracts

A reads this before planning checks; B reads it before the first review. Keep the
following compact mappings in TASK and link them from assignments and review notes.
Scale detail to changed behavior; do not duplicate original requirements or logs.

## Acceptance mapping

| Criterion/source revision | Production path and case | Decisive assertion | Actual check/evidence | Status |
| --- | --- | --- | --- | --- |

Before implementation, identify the observable outcome and how the assertion
distinguishes it from the demonstrated failure. After implementation, record actual
test names and results. An architecture-gate pass accepts the verification plan,
not execution of its promised tests. Preserve a gap until the required evidence
exists or the owning scope has an explicit agreed disposition.

Use cases that distinguish the relevant contract: real decision submission versus
an event stand-in; an incomplete transport response versus a complete error status;
the last item reachable by scrolling versus a container-size proxy. For changed
variants, bounds or concurrency, select discriminating cases such as a different
selector, zero matches, maximum caller page size, or competing production callers.
Name what a stand-in does not prove. Inspection may establish an inspection claim;
it does not fulfill an explicit behavioral or enduring regression-test obligation.

B independently follows the original criterion through the actual fixture,
production branch and assertion. A test's name, existence or green result alone
does not establish that relation. Keep PASS, FAIL and NOT EXERCISED distinct in
machine-readable evidence and user-facing reports.

## Contract impact

| Changed contract/value | Producers, consumers, wrappers, equivalent surfaces | Inherited obligations/disposition | Focused checks |
| --- | --- | --- | --- |

Use the repository's caller graph and exhaustive literal searches for stored
fields, discriminators and decision outputs. Include SQL readers and UI sibling
views when relevant. For a reused service/component, enumerate inherited side
effects, validation, authorization and container dependencies. Account for each
affected consumer through a change, verification or justified unchanged disposition.
B verifies this against actual code, not only A's inventory.

When moving/deleting tests, save the runner's discovered test names before and
after the change. For Rust use `cargo test --locked --offline --all-targets -- --list`,
or the same focused target both times; other projects use their own runner. Map
each removed name to a replacement present in the
new inventory or to intentionally removed behavior. A syntactically reachable
module does not prove test collection under every cfg or behavioral coverage.
In openbot, `cargo test --locked --offline --test source_modules` also detects
Rust files disconnected from Cargo target module roots.

## Retained evidence

Save a manifest at `<phase-docs>/logs/<round>.evidence.json` for each implementation
review/recheck and final acceptance. A supplies execution entries; B checks them
and records its own review conclusion separately. A manifest summarizes only
acceptance evidence, not every exploratory command. Use this schema:

```json
{
  "version": 1,
  "round": "TOPIC-P01-B1-R1",
  "artifact": "<64 lowercase hex digits from the frozen snapshot>",
  "checks": [
    {
      "criteria": ["AC1"],
      "command": "cargo test --locked --offline --lib named_test",
      "fixture": "owned database/fixture identity, without credentials",
      "status": "PASS",
      "files": [
        {"path": "logs/round-tests.txt", "sha256": "<64 lowercase hex digits>"}
      ]
    }
  ]
}
```

PASS requires an exercised successful assertion; FAIL records an exercised failure.
Both require retained output files. NOT EXERCISED requires a reason and may omit
files. Retain failures as well as their correction evidence. Keep commands free
of secrets; record safe fixture identities instead of credential-bearing URLs.
Use paths relative to this phase's records directory, so archival moves preserve
them. Keep evidence files immutable once their manifest is relied upon; a changed
check gets a new round/manifest. Do not rewrite historical rounds or backfill
archived records.

Before freeze/review and acceptance, resolve the placeholders to literal absolute
paths and prefix each Bash call with the worktree directory:

```sh
cd "<worktree>" && python3 "<skill-dir>/scripts/check-evidence.py" \
  "<phase-docs>/logs/<round>.evidence.json" --expect-artifact "<digest>"
```

The checker verifies schema, file identity, containment and retention eligibility.
It cannot establish that a command ran, that its assertions were sufficient, or
that every criterion is covered: A/B/root retain those obligations. Its success
does not turn FAIL/NOT EXERCISED into acceptance. Root reports unresolved required
checks using the existing acceptance rules.

Tracked evidence is retained even if its extension matches an ignore rule.
Untracked evidence must be nonignored until a user-authorized commit/package
includes it; the checker never stages files or authorizes a commit. Before
archival/commit, and in CI, use `--require-tracked` to verify inclusion:

```sh
cd "<worktree>" && python3 "<skill-dir>/scripts/check-evidence.py" --tracked-manifests --require-tracked
```

Local aggregate checks use `--available-manifests` for tracked and nonignored
untracked manifests. These modes recognize only `logs/*.evidence.json`, leaving
earlier record formats unchanged. Symlinks escaping the selected records tree are
rejected. Validation and fixture tests run in CI and the local verification script.
