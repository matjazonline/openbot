# Flexible tracking and customer records: profiles, activities, tags and segments

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-25. This is an implementation plan; nothing in it is implemented.
File anchors are on `main` at `a1cdb2d5`. Re-trace every anchor with Trail before implementation.

All work is inside the independent attribution service, [services/attribution](../../services/attribution/README.md),
and follows its [AGENTS.md](../../services/attribution/AGENTS.md). Openbot's source, schema and
routes are not changed by this plan. The companion plan
[attribution-cookie](../attribution-cookie/README.md) is independent; neither needs the other.
Both edit the service's one migration and its README, so implement them one after the other.

## Why this lives in the attribution service

Agents inside and outside Openbot run multi-step outreach and sales funnels: find a business,
review its website, build a preview, send a tailored message, record replies and appointments,
compare message variants, and segment the audience. They need one place to read and write who
the prospects and customers are, what happened with each, and where each one stands.

Decided with the owner on 2026-11-24:

- **One store for records, actions and status.** A prospect's status follows from the actions,
  and segment reports need attributes and actions in one query. Two services would drift, and
  the attribution boundary forbids reading another application's database.
- **The attribution service, as a separate area.** It already has tenants, sign-in, scoped
  server keys, campaigns, ads, reports, retention and deployment. The new area adds its own
  tables and routes and leaves anonymous clicks, checkouts, orders and refunds as they are.
- **Not Openbot.** Outside agents need a network API with their own keys, and prospects exist
  before any Openbot conversation does.

## Intended behavior

A local-business outreach run, with every write made through a store-scoped server key:

1. A discovery agent resolves a profile by identifier (`domain: bistro-ana.example`,
   `external: google_place:…`). The service returns the existing profile or creates one; two
   agents racing on the same business end with one profile.
2. A review agent writes attributes (`city`, `industry`, `website_score`) and tags (`no-booking`),
   and records an activity `site.reviewed`.
3. An enrollment places the profile in a campaign and assigns one of the campaign's variants.
4. A sender checks contactability, asks for the address's unsubscribe link and, if the store
   allows them, a prospect link, sends the message itself, and records `email.sent`. Later
   activities (`link.clicked`, `email.replied`, `appointment.booked`) move the enrollment's stage
   forward in the same transaction that stores them. A recipient who unsubscribes is stopped for
   every agent at once.
5. A person or agent reads a segment ("Ljubljana, score under 50, visited the preview in the
   last 14 days, not booked"), the per-variant rates for the campaign, and its flow from stage to
   stage.

The service records and reports. It does not run the steps, send messages, or host preview sites.

## The model

| Thing | What it is | The flexible part |
| --- | --- | --- |
| Profile | One prospect or customer in a store: a person or an organization | A flat bag of attributes |
| Identifier | An email, phone, domain or external reference that points at exactly one profile | The external namespace |
| Tag | A label on a profile | Any token |
| Activity type | A declared kind of happening: its name, channel, direction and property types | Everything; a store declares its own |
| Activity | One append-only row: this type happened to this profile at this time | The properties |
| Enrollment | A profile's place in a campaign: variant and stage | The campaign's stage list and its final stages |
| Stage change | One append-only row per stage move: from, to, when and why | — |
| Segment | A saved rule over attributes, tags, activities and enrollments, evaluated on demand | The rule |
| Suppression | A do-not-contact entry keyed by identifier; it outlives the profile | — |
| Unsubscribe link | A link for one email address or phone number that records a do-not-contact entry | — |
| Prospect link | A tracked link made for one organization profile; a click is recorded on that profile | Off unless the store turns it on |

A new channel (phone, chat, email, ads, post) or a new measure is a new activity type; it needs
no schema change. The word *activity* is used because `/v1/events` already means browser
observations and `attribution_merchant_events` already means merchant deliveries.

A metric is a count of activities of one type, or of the distinct profiles that had one, grouped
by day, campaign, variant, channel or type and optionally limited to a segment. A rate is one
such count divided by another. A funnel is an ordered list of types. An A/B comparison is the
same metric grouped by variant.

## Rules that do not bend

- **Tenant and store scope.** Every row carries `company_id` and `store_id`, like every existing
  attribution table. A key reaches only its own store. No caller-supplied company or store field
  grants authority.
- **Declared types only.** An activity with an undeclared type, an undeclared property or a
  property of the wrong type is refused. Reports can then trust names and types.
- **Activities are append-only.** They are never edited. They leave only through retention or the
  erasure of their profile.
- **One identifier, one profile.** Uniqueness is a database constraint. A write that would join
  two existing profiles is a conflict naming both; nothing merges automatically.
- **Do-not-contact outlives the profile.** Suppression is keyed by identifier, checked on every
  profile read and contactability check, and kept when the profile is erased.
- **The strict parts stay strict.** Clicks, observations, checkouts, orders, refunds and their
  money and attribution rules are untouched. An anonymous click, observation or browser is never
  linked to a profile. The only click tied to a profile is a click on a prospect link, and those
  are tracked only while the store's setting allows it.
- **No profile data on public routes.** Profiles are reachable only with a user session or a
  server key. The public redirect and unsubscribe pages show no address and no profile data.
  Logs never carry identifiers, attributes or properties.

## Access

Server keys gain four scopes; the `attribution_credentials` scope list and its cardinality bound
grow to match.

| Scope | Allows |
| --- | --- |
| `profiles:read` | Read profiles, identifiers, tags, timelines, enrollments; evaluate segments; check contactability |
| `profiles:write` | Resolve, create and update profiles; set tags; enroll; set stage; add a suppression |
| `activities:write` | Record activities |
| `definitions:write` | Declare activity types, campaign stage lists and segments |

Two existing scopes gain one use each: `links:write` also creates prospect links, when the store
allows them, and `activities:write` also creates unsubscribe links, since every sender holds it.

Activity analytics use the existing `analytics:read`. For signed-in users the existing
permissions map as: `read` views everything here; `links:manage` edits profiles, tags, types,
stages, segments and enrollments; `integrations:manage` also removes suppressions, exports and
erases profiles, and issues keys.

## Out of scope, with the reason

- **Email open pixels.** Reading whether a recipient opened a message falls under the
  terminal-equipment consent rule (ZEKom-2 Article 225, the EU cookie rule). Unlike the
  marketing-email rule in Article 226, it is not limited to individuals, so it covers company
  addresses too; France's regulator says so explicitly in its 2026 recommendation on email
  pixels. Mail apps also preload images, so open counts are unreliable anyway. Prospect links
  fall under the same rule, which is why they are an opt-in store setting with a recorded legal
  decision (D6) rather than a default.
- **Linking browsers or anonymous clicks to profiles.** That needs the visitor cookie and a
  separate, explicit consent. See [attribution-cookie](../attribution-cookie/README.md).
- **Merging two profiles.** Conflicts are surfaced; a merge operation is a later plan once real
  duplicates show what it must preserve.
- **An MCP surface and Openbot agent tools.** The contract is OpenAPI over HTTPS. Openbot would
  hold a store key in its backend, as the attribution plan already provides for.
- **Permission to contact.** A profile written through a key is a record, not a permission.
  Openbot's outreach allowlist ([outreach_allowlist_plan.md](../outreach_allowlist_plan.md)) is
  unchanged and is not fed from this store.
- **Bulk import, significance testing, pre-aggregated reports.** Add each on measured need.

## Decisions to confirm before the dependent phase

| # | Decision | Default in this plan | Needed by |
| --- | --- | --- | --- |
| D1 | Profiles belong to one store, not to the whole company | Store | 01 |
| D2 | Suppression entries keep the readable identifier, visible only to integration administrators | Readable | 01 |
| D3 | Retention: days an activity is kept, and days an idle profile is kept | No default; the deployment owner sets both | 06 |
| D4 | Lawful basis and notice for holding prospect data in each target country | Owner's legal decision | Before any production data |
| D5 | Prospect links bind to organization profiles only, never to a person profile | Organization only | 04 |
| D6 | Legal decision, per store, to turn prospect links on (Article 225 consent for per-recipient tracking) | Off; on only with a recorded acknowledgement | Before a store turns it on |

Record each answer, its date and who gave it in this file. An unanswered item stays open; do not
present a default as approved policy.

## Phases

Implement in order. Each phase depends on the ones before it.

| Phase | Deliverable |
| --- | --- |
| [01 — Profiles, identifiers and tags](01-profiles-identifiers-and-tags.md) | Records, resolve-or-create, tags, suppression, key scopes |
| [02 — Activity types and activities](02-activity-types-and-activities.md) | Type registry, append-only activity log, timeline |
| [03 — Stages and enrollments](03-stages-and-enrollments.md) | Campaign stage lists, final stages, enrollment, variant assignment, stage movement, stopping, stage history |
| [04 — Prospect links and unsubscribe](04-prospect-links-and-unsubscribe.md) | Unsubscribe links, the opt-in store setting, prospect links and scanner filtering |
| [05 — Segments and activity analytics](05-segments-and-activity-analytics.md) | Rule language, evaluation, metrics, funnels, campaign flow, dashboard pages |
| [06 — Retention, erasure and final verification](06-retention-erasure-and-final-verification.md) | Lifetimes, export, erasure, tenant purge, docs, full gate |

## Verification discipline

Each batch names focused checks; run only those while implementing it. List a filtered test set
before running it and confirm a nonzero count:

```sh
cd services/attribution
cargo test --locked --offline --all-targets PREFIX -- --list
cargo test --locked --offline --all-targets PREFIX
```

Database tests use the disposable fixture and fail when it is unavailable. After SQL changes run
`scripts/verify.sh prepare`. Record commands, selected and executed counts, results and any check
that could not run in the phase file. The one full-suite gate is in
[06](06-retention-erasure-and-final-verification.md); do not rerun the full suite per batch.

No resource limit is raised by this plan: the 256 KiB body limit, the 2 MiB stack budgets, the
connection and request budgets and the SQL deadlines stay as they are. If implementation finds a
limit must rise, name what fails early instead and keep that check in CI.

## Anchors

- Schema: [one migration, edited in place](../../services/attribution/migrations/202611150001_foundation.sql).
- Key scopes: `KeyScope` in [application/setup.rs](../../services/attribution/src/application/setup.rs)
  and the `attribution_credentials` check constraint.
- User permissions: `Permission` in [application/identity.rs](../../services/attribution/src/application/identity.rs).
- Campaigns and ads: [application/catalog.rs](../../services/attribution/src/application/catalog.rs),
  [adapters/catalog](../../services/attribution/src/adapters/catalog/postgres.rs).
- Redirect, where prospect links are recorded: `redirect` in
  [adapters/catalog/http.rs](../../services/attribution/src/adapters/catalog/http.rs) and the
  click insert in [adapters/catalog/postgres.rs](../../services/attribution/src/adapters/catalog/postgres.rs);
  the bot and prefetch classifier is `classify` in `application/catalog.rs`.
- Reports: [REPORTING.md](../../services/attribution/REPORTING.md),
  [application/analytics.rs](../../services/attribution/src/application/analytics.rs).
- Retention and tenant purge: [operations.md](../../services/attribution/operations.md),
  [application/retention.rs](../../services/attribution/src/application/retention.rs).
- Contract: [contract/attribution.openapi.json](../../services/attribution/contract/attribution.openapi.json).
- Independence guard: [scripts/boundary.py](../../services/attribution/scripts/boundary.py); every
  new table must be created by the service's own migration.
