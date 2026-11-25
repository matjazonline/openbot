# 05 — Segments and activity analytics

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-prospect-links-and-unsubscribe.md). A saved rule selects profiles by attributes, tags,
activities and stage. Counts, rates and funnels over activities can be grouped by variant and
limited to a segment, and the dashboard shows profiles, timelines and these reports.

## Batch 05A — Segment rules and evaluation

1. Add `attribution_segments`, scoped by company and store: name, the rule, `revision`. Writing
   one needs `definitions:write` or `links:manage`.
2. A rule is a bounded JSON tree. Inner nodes are `all`, `any` and `not`. Leaves are:
   - attribute: a key with `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `contains` or `exists`;
   - tag: has the tag;
   - kind and contact status;
   - activity: had at least `min_count` activities of a type, optionally within the last N days
     and optionally in one campaign;
   - enrollment: is in a campaign, optionally at a stage (`stopped` included) or with a
     variant.
   An activity leaf on `link.clicked` counts only clicks not suspected of being mail scanners.
   Depth is at most 4 and a rule has at most 32 leaves. Reject anything else at write time.
3. Compile a rule to one parameterized SQL statement. Attribute keys, tags, type names and values
   are bound parameters; no caller text is ever concatenated into SQL. Every subquery repeats the
   company and store predicate.
4. Comparison is typed: a number operator on a string value does not match and does not error.
   "Within the last N days" is measured from the database clock at evaluation.
5. `GET /v1/segments/{id}/profiles` returns matching profiles with bounded pagination;
   `GET /v1/segments/{id}/count` returns the number. Both need `profiles:read` or `read`. Segments
   are evaluated on demand and never stored as membership.
6. The existing independence guard checks SQL text for owned tables. Generated SQL must stay
   inside statements it can read; keep the fragments as literals in the PostgreSQL adapter.

Focused checks: `attribution_segments_` with a fixed dataset of two companies and two stores,
checking each leaf and each inner node by hand-computed membership, the bounds, typed
comparisons, and foreign campaign, tag and type references. Include hostile keys and values
(quotes, comment markers, very long strings) and assert they are data. Capture query plans for a
representative three-leaf rule on the pilot volume.

## Batch 05B — Counts, rates and funnels

1. Add `GET /v1/activity-analytics`, needing `analytics:read` or `read`. It follows the existing
   report conventions in [REPORTING.md](../../services/attribution/REPORTING.md): RFC3339 `start`
   and `end` as a half-open interval of at most 93 days, an IANA `timezone`, scope-bound cursors,
   and rejection of unknown query fields. Activities are counted by `occurred_at`.
2. Filters: type, channel, direction, campaign, variant and `segment_id`. Grouping: `day`,
   `type`, `channel`, `campaign` or `variant`. Each group returns the activity count and the
   number of distinct profiles. Groups use frozen identifiers, not current names.
3. A rate takes a numerator type and a denominator type and returns distinct profiles with the
   numerator divided by distinct profiles with the denominator in the same group. It is null when
   the denominator is zero; never return a meaningful-looking zero. Label it as a period rate.
4. A funnel takes 2–8 ordered types. Step *k* counts distinct profiles whose first activity of
   step *k* in the interval is at or after their first activity of step *k−1*. Return each step's
   count; do not return percentages the reader could mistake for significance.
5. Grouping by variant is the A/B comparison. Report counts and rates only. Report outbound
   activities written for suppressed profiles as their own count, and `link.clicked` activities
   suspected of being mail scanners as their own count; rates and funnels use only unsuspected
   clicks.
6. Aggregate before joining, so a profile with many activities cannot multiply another measure.
   Start with indexed queries and inspect plans; add pre-aggregation only on measured need.
7. `GET /v1/activity-analytics/export` returns the same rows as CSV, with the existing 10,000-row
   refusal and spreadsheet-formula protection.

Focused checks: `attribution_activity_analytics_` against a fixed ledger with two variants, a
segment, zero-denominator groups, out-of-order activities, time zone and daylight-saving
boundaries and a foreign segment id. Reconcile every figure with independently computed totals.

## Batch 05C — Dashboard pages

1. Add to the service's own frontend: a profile list with identifier, tag and segment filters; a
   profile page with attributes, tags, enrollments and the activity timeline; activity types;
   segments with their count; and a campaign page with stage counts and the per-variant report.
2. Follow the existing pages: ordinary refresh and filter navigation, CSRF on every write, escaped
   output, keyboard focus, empty and error states. Attribute values, properties and tag text are
   untrusted and must render as text.
3. Hide controls the user's permission does not allow, and enforce the same rule on the server.
4. Add the browser journey to `scripts/verify.py` beside the existing three, and to the CI
   workflow and the root runner wherever they list browser prefixes.

Focused checks: `attribution_profiles_browser_` for role-based controls, a script-bearing
attribute and tag rendered as text, tenant switching, and agreement between the pages and the API
fixtures.

Completion: a person can find a group of profiles, read what happened with each and compare
variants. Record commands, counts, results and query plans here; update the contract and
REPORTING.md. Full-suite gate: [06](06-retention-erasure-and-final-verification.md).
