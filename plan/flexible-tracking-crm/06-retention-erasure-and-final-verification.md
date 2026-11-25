# 06 — Retention, erasure and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-segments-and-activity-analytics.md) and on decisions D3 and D4 in the
[README](README.md). Profile data has explicit lifetimes, a person's data can be exported and
erased, a deleted company's data is removed, and the whole service passes its full gate.

## Batch 06A — Lifetimes, export and erasure

1. Add two required configuration keys, validated at startup like the existing retention keys and
   documented in the service README and `config.env.example`:
   - `ATTRIBUTION_ACTIVITY_RETENTION_DAYS`: an activity is removed this many days after
     `occurred_at`.
   - `ATTRIBUTION_PROFILE_IDLE_RETENTION_DAYS`: a profile with no activity and no update for this
     many days is erased. It must be at least the activity retention.
   Their values are the deployment owner's decision (D3). The verification values are not policy.
2. Extend the existing maintenance worker; do not add a second scheduler. Keep its bounds: a
   limited number of tenants per pass, at most 100 rows per tenant transaction, the existing SQL
   and batch deadlines, and fair scheduling so one large tenant cannot starve the others.
3. Erasing a profile removes its identifiers, tags, enrollments and activities, then the profile,
   in bounded batches. Suppression entries stay. Its unsubscribe tokens are deleted unless the
   address is suppressed, so an old unsubscribe link then shows the not-valid page. Its prospect
   links lose their `profile_id` and become ordinary links of their ad, so their clicks remain
   only as anonymous clicks. Aggregated history shrinks; say so in
   REPORTING.md the way raw click expiry is already described.
4. `DELETE /v1/profiles/{id}` needs `integrations:manage`. It marks the profile unreachable at
   once and lets maintenance finish the removal. An optional flag adds suppression entries for
   the profile's identifiers in the same transaction.
5. `GET /v1/profiles/{id}/export` needs `integrations:manage` and returns everything held about
   the profile as one bounded JSON document, streamed, for an access request.
6. Confirm the tenant purge added in phase 01 covers every table from phases 01–05 in dependency
   order, and that a suspended or deleted company is refused on every new route.

Focused checks: `attribution_profile_retention_` for exact cutoffs, idle detection, erasure that
competes with a resolve, an activity write and an enrollment for the same profile, interruption
and restart mid-erasure, suppression surviving erasure, and a poison tenant not blocking others;
`attribution_profile_lifecycle_` for suspended and deleted companies on every new route and for
purge completeness.

## Batch 06B — Documents and contract

1. Update the service README: the new routes, scopes, configuration keys and the startup
   validation they actually have. Name only keys the application reads.
2. Add a short guide beside WEBSITE.md and REPORTING.md covering the model, the rule language,
   contactability, unsubscribe links and their email headers, prospect links and the scanner
   rule, safe retries with `activity_key`, and the statement that the service records
   outreach and does not send it.
3. Update the OpenAPI contract and examples for every new route, error and bound, and extend
   `contract/test_contracts.py` to cover them, including one example per new scope and a denial.
4. State in operations.md what is personal data here, where an erased contact's identifier
   remains, and that logs carry no identifiers, attributes or properties. Add a test that scans
   captured logs from a representative run for a known identifier and attribute value.

Focused checks: `scripts/check-contracts.sh`; `attribution_profiles_logging_` for the log scan.

## Batch 06C — Final full-suite gate

Run once, after everything above is integrated, from a clean checkout:

```sh
cd services/attribution
scripts/verify.sh prepare          # only if SQL changed since the last regeneration
scripts/verify.sh                  # format, compile, Clippy, boundary, tests, contracts, browser
python3 scripts/extracted-build.py # builds and tests a copy with no parent files
```

Then the service image and container checks used by the attribution CI workflow, and from the
repository root:

```sh
./scripts/check.sh
```

The root runner includes the attribution gates and the plan inventory check. Openbot's own source
is unchanged by this plan, so a failure in an unrelated root gate is reported, not fixed here.

Record every command, the selected and executed test counts, the results and any check that
could not run. Do not claim full verification if a required check was unavailable. After a fix
made during final verification, rerun the affected checks, and repeat the full gate when the fix
touches shared code such as the tenant lock, the maintenance worker or request admission.

Completion: the customer-record area is documented, bounded, erasable and verified as a whole.
Production use still needs decision D4, and each store's prospect links need D6.
