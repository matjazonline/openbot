# 04 — Authorization in the application layer

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). Starts after [01](01-identity-and-ownership.md) merges: owner checks
move onto `company_access` here, which is only correct once 01 makes ownership one enforced fact.
[07](07-application-layering.md) starts after this one.

## Outcome

- Every authorization decision is made once, in an application use case, by a method that takes
  the caller. HTTP handlers and the SMTP adapter only translate.
- No adapter reads a tenant row by bare id.
- Responses are consistent:

| Case | Response |
|---|---|
| Unknown, or someone else's | 404, revealing nothing |
| Visible, but not allowed | 403 |
| Database error | An error, never a decision |

## What happens today

### Unscoped reads are exposed

- `CompanyUseCases::get_company(id)` (`src/application/use_cases/company.rs:372`) is called from 26
  adapter sites.
- `ThreadUseCases::get_thread(id)` (`src/application/use_cases/thread/mod.rs:664`) is called from 3.

### Handlers make their own checks

- **The agent pages leak and swallow.** `list_agents_page`
  (`src/adapters/http/routes/agent.rs:254-271`) renders any company's name to any logged-in user,
  and turns the agent list's permission error into an empty list. `create_agent_handler` (`:275`)
  starts the same way.
- **Database errors become "not found".** Pages check ownership with `Ok(Some(c)) if c.user_id ==
  user.id => …, _ => "not found"` (`routes/channel.rs:561`, `:1066`; `routes/company.rs:135`,
  `:151`), so a database error reads as "not found".
- **"This thread is in this channel" is written three times:**
  - `load_channel_thread` (`routes/ui.rs:248-255`);
  - `routes/channel.rs:648-654`, which answers "Thread does not belong to this channel" and so
    confirms that the id exists;
  - `routes/channel.rs:923`.
- **Two owner checks disagree.** `ui_team.rs:541-556` decides "owner" from `company.user_id ==
  user_id` and fails with `AppError::Internal` (a 500). `ui_companies.rs:442`, `:505` use
  `membership.is_owner()`.
- **The operator check is copied by hand.** `AppConfig::is_operator` (`src/infra/config.rs:402`) is
  applied in `monitoring.rs:28`, `agent_library.rs:99`, `ui_dashboard.rs:169` and `ui.rs:1164`. The
  library-agent use cases do not check it themselves.
- **Attention gating lives in HTTP.** The attention routes compute team and manager gating
  themselves and call `AttentionPersistence` directly (`routes/attention.rs:80-95`, `:117`,
  `:196`).

### Access checks run through trait defaults

- **`company_access` loads a capped list.** `CompanyPersistence::company_access`
  (`use_cases/company.rs:168-178`) is a default method that scans `list_accessible_by_user_id`.
  Postgres caps that list at the 200 newest companies (`src/adapters/persistence/company.rs:195-207`).
  Every check loads up to 200 rows, and beyond 200 companies the oldest are silently denied.
- **Other correctness methods have defaults too:**
  - `update_for_user` and `delete_for_user` (`:133-149`);
  - `UserPersistence::has_login_method` and `activate_password` (`use_cases/user.rs:209-213`);
  - `ThreadPersistence::latest_replyable_email_context` (`thread/mod.rs:294`).
- **The mocks hide it.** `MockCompanyPersistence` is declared six times, and every copy inherits
  "every owned company is Owner", so the Admin and Member paths go untested.

### Onboarding commits halfway

`routes/onboarding.rs:311-345` creates N agents and then N channels inside the handler:
- the agent rollback ignores delete errors;
- a channel error leaves the agents behind.

### SMTP decides sender trust itself

- `src/adapters/smtp/server.rs:612-665` loads the company, channel and participant context through
  `ThreadUseCases`' raw persistence accessors (`thread/mod.rs:397-411`).
- That repeats the loading in `thread/ingest/routing.rs`.

## Decisions

- **`company_access(user, company)` is the one membership read.** It becomes a required trait
  method backed by a one-row query. Owner means membership `Owner`, which 01 makes agree with
  `companies.user_id`.
- **Scoped use-case methods replace bare-id reads.** For example, a company read returns the company
  together with the caller's membership, and a thread read takes viewer, company, channel and
  thread. Pick names that fit the existing modules. `get_company` stays only for internal callers,
  `pub(crate)`.
- **Operator is a property of the caller.** The auth extractor decides it once, and use cases that
  need it check it.
- **Onboarding becomes one use case** with a purpose-specific port. That port runs the library
  copies and the channel provisioning in one transaction; `provision_on` already takes a connection
  (`src/adapters/persistence/workflow/provisioning.rs:536`).
  - A provisioning *refusal* stays a result shown on the page, as it is today.
  - An *error* leaves nothing behind.
  - First trace whether `create_agent_from_library` can run on that same transaction.
- **SMTP asks the application.** It calls a use-case method, such as
  `sender_trust(recipient, sender)`, and the persistence accessors are removed.

## Batches

### 1 — `company_access` is a required one-row query

- Remove the defaults from `company_access`, `update_for_user`, `delete_for_user`,
  `has_login_method`, `activate_password` and `latest_replyable_email_context`.
- Implement them in Postgres. Make every mock state its own behaviour, and hoist the six
  `MockCompanyPersistence` copies into shared test support.
- Tests, `company_access_*` (DB-backed):
  - a user with 201 companies can access the oldest one;
  - owner, admin, member and no-relation each return the right membership.

### 2 — Scoped company reads

- Replace the handler uses of `get_company(id)` and the `c.user_id == user.id` checks.
- Fix `list_agents_page` and `create_agent_handler`.
- Route tests:
  - another tenant's company id returns 404, with no company name in the body;
  - a member attempting an owner action gets 403;
  - a failing persistence mock gives 500, not "not found".

### 3 — One thread-in-channel read

- Add one use-case method and delete the three copies.
- Tests: a thread from another channel, or another tenant, returns the same 404 as an unknown id.

### 4 — Owner, operator and attention checks

- `ui_team` takes its role from membership, and a non-owner gets 403.
- Operator becomes a caller property, and the library-agent use cases check it.
- Attention gating moves into a use case.
- Tests: existing `ui_team`, `agent_library`, `monitoring` and `attention` route tests, plus a
  non-operator calling a library-agent use case directly.

### 5 — Onboarding as one use case

- Tests, `onboarding_*`:
  - a channel error after two agents were created leaves no agents;
  - a provisioning refusal keeps the agent and shows the refusal;
  - a retry with the same selection is idempotent.

### 6 — SMTP sender trust in the application

- Add the use-case method, switch `smtp/server.rs` to it, and delete the accessors at
  `thread/mod.rs:397-411`.
- Tests: the existing SMTP trust tests, plus a lookup failure that is logged and treated as a
  stranger, as today.

## Focused checks

```sh
createdb mail_agents_fix_app_04
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_04" \
  cargo test --locked --offline --lib company_access_
# repeat with routes::agent, routes::channel, routes::company, ui_team, attention, onboarding_, smtp::
dropdb mail_agents_fix_app_04
```

Batch 1 changes a widely implemented trait, so run `cargo check --locked --offline --all-targets`
straight after it. Confirm a nonzero test count for each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
