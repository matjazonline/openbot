# 03 — Stages and enrollments

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-activity-types-and-activities.md). A campaign has an ordered list of stages. A
profile enrolled in it holds one variant and one stage, and the stage moves forward in the same
transaction as the activity that causes it.

## Batch 03A — Campaign stages and enrollment

1. Add to `attribution_campaigns` an ordered `stages` list (at most 32 distinct tokens matching
   the attribute key pattern, empty by default) and a `final_stages` subset of it, for outcomes
   such as `won` and `lost`. Every campaign also has an implicit final stage `stopped`, which
   cannot be declared and is set by a do-not-contact entry or by hand. A campaign with no stages
   still accepts enrollments; they have no stage until they are stopped.
2. Changing the list needs `definitions:write` or `links:manage`. A stage may be appended or
   reordered, and marked final or not final at any time. Removing a stage is refused while an
   enrollment holds it.
3. Add `attribution_enrollments`, scoped by company and store: campaign, profile, optional
   `ad_id` (the variant), optional `stage`, `stage_changed_at`, `entered_at` and `revision`.
   Unique on campaign and profile.
4. `POST /v1/enrollments` needs `profiles:write` or `links:manage`. It is idempotent on campaign
   and profile: a repeat returns the existing enrollment unchanged. Enrolling a profile whose
   contact status is suppressed creates the enrollment at `stopped`.
5. The caller either names the variant or asks the service to assign one. Assignment picks
   uniformly at random among the campaign's enabled ads and is frozen: a later enable, disable or
   repeat request never changes it. A campaign with no enabled ad yields no variant.
6. `GET /v1/enrollments` filters by campaign, `stage` (where the enrollment is now),
   `entered_stage` (a stage it has ever entered, from the history in 03B), variant and profile,
   with bounded pagination.

Focused checks: `attribution_enrollments_` for stage-list validation, final stages, refusal to
declare `stopped`, refused removal, foreign campaign, profile and ad, idempotent repeats, a
suppressed profile enrolled at `stopped`, and at least eight competing enrollments of one
profile in one campaign ending with one enrollment and one variant. Check the assignment is
close to uniform over a large sample with a fixed test seed, and that it never picks a disabled
ad.

## Batch 03B — Move the stage

1. When an activity names a campaign and its type carries a `stage` that is in that campaign's
   list, set the enrollment's stage in the transaction that stores the activity. If the profile
   is not enrolled, enroll it without a variant in the same transaction.
2. Movement by activity is forward only: the stage changes when the new stage's position in the
   list is later than the current one. An earlier or equal stage, a late-arriving older activity
   and a replayed activity leave the stage unchanged. Activities never move an enrollment out of
   a final stage, `stopped` included, and never into `stopped`.
3. Lock the enrollment row while deciding, so two activities for one profile cannot each read the
   old stage and write a regression.
4. `PATCH /v1/enrollments/{id}` with the current `revision` lets a permitted actor set any stage,
   a final one or `stopped` included, forwards or backwards: for example `lost`, or reopening a
   stopped enrollment after its do-not-contact entry was lifted. It records an audit row.
5. A type whose stage is not in the campaign's list has no stage effect; the activity is still
   stored.
6. A do-not-contact entry stops campaigns. When an entry matches one identifier exactly, set that
   profile's enrollments that are not in a final stage to `stopped` in the transaction that adds
   the entry. When a domain entry matches the email domain of many profiles, the existing
   maintenance worker does the same in bounded batches. Contactability is correct at once in
   both cases; the stage only keeps stopped profiles out of "who to contact next" lists. Lifting
   the entry reopens nothing.
7. Keep a stage history. Add `attribution_enrollment_stage_changes`, scoped by company and store:
   the enrollment, its campaign and profile, `from_stage` and `to_stage` (either may be empty, for
   an enrollment with no stage), `changed_at` from the database clock, and `cause`: `enrollment`,
   `activity` (with the activity id), `manual` (with the actor) or `suppression`. Write one row in
   the same transaction as every stage change, under the enrollment lock: on creation, on
   movement by an activity, on a manual change, and when a do-not-contact entry or maintenance
   stops an enrollment. A replayed, earlier or refused move writes nothing; decide that by
   comparing the locked row's old and new stage, not from an upsert's affected-row count, which
   reports a row even when nothing changed. Rows are append-only
   and leave only through retention or erasure
   ([06](06-retention-erasure-and-final-verification.md)). Index them by campaign and time for
   the campaign flow in [05](05-segments-and-activity-analytics.md).

Focused checks: `attribution_enrollments_stage_` for forward-only movement, final stages and
`stopped` that activities cannot leave, out-of-order and replayed activities, implicit
enrollment, the manual override and its stale-revision conflict, an exact do-not-contact entry
stopping enrollments in its own transaction, and a domain entry stopped by maintenance across
many profiles. With competing claimants: activities for one enrollment that carry different
stages, asserting the result is the latest in list order regardless of arrival order; and a
do-not-contact entry racing an activity for the same enrollment, ending at `stopped` unless the
activity reached a final stage first. And `attribution_enrollments_history_`: every path that
changes a stage writes exactly one row, an unchanged stage writes none, and each enrollment's rows
form an unbroken chain (each `from_stage` equals the previous `to_stage`) ending at its current
stage, including after the competing cases above.

Completion: campaign funnels can be declared, and each profile's place in them is consistent
under competing writers, with a complete history of how it got there. Record commands, counts and results here; regenerate SQLx metadata;
update the contract. Full-suite gate: [06](06-retention-erasure-and-final-verification.md).
