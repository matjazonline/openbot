# 07 — Application layering

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). Starts after [04](04-authorization-in-application.md) merges. Both
plans reshape the `use_cases` ports, and 04 removes the `ThreadUseCases` accessors that this plan
would otherwise move.

## Outcome

Application code depends only on ports it owns, as `src/AGENTS.md` ("Preserve dependency
direction") and `src/application/AGENTS.md` require:
- no infra configuration or framework clients in application;
- no module cycles between application submodules;
- the composition root returns HTTP state and workers separately.

There are no schema changes.

## Batches

### 1 — The spam scanner becomes an adapter

**Today:**
- `src/application/services/spam_scanner.rs` imports `crate::infra::config::AppConfig` (`:1`), and
  holds a `reqwest::Client` and a tokio `TcpStream` (`:142-151`).
- The SMTP server builds a new scanner, and so a new HTTP client, for every inbound message
  (`src/adapters/smtp/server.rs:668`).

**Change:**
- An application port, for example `SpamScanner::scan(…) -> SpamScanResult`, with the
  implementation moved under `src/adapters/`.
- Build it once at startup with one client, and inject it into the SMTP server.
- Keep `SpamScanResult` in application.

**Tests:**
- The scanner's own tests, moved with it.
- An SMTP test with a stub scanner.

### 2 — Narrow configuration

**Today:**
- Application use cases hold `Arc<AppConfig>`, for example `use_cases/channel.rs:20`, `:165`,
  `:174`. Run `grep -rn AppConfig src/application` for the full list.
- `ThreadUseCases::config()` (`thread/mod.rs:413`) hands the whole configuration on to HTTP and
  webhook code (`routes/channel.rs:929`, `:983`; `sendgrid.rs:42`).

**Change:**
- Each use case takes the small settings it reads, such as the mail domain, the public base URL and
  the spam settings. Infra builds them from `AppConfig`.
- Remove `config()`. HTTP gets what it needs from its own state.

**Tests:** existing use-case tests. Their `AppConfig { .. }` literals become the narrow structs.

### 3 — A dedicated key for confirmation codes

**Today:**
- Account-change and confirmation codes are hashed as `SHA-256(jwt_secret ‖ ":" ‖ …)`
  (`src/application/use_cases/user.rs:1165-1191`).
- Rotating the JWT secret therefore voids every pending code, and secret-prefix SHA-256 is not an
  HMAC.

**Change:**
- Hash codes through the keyed helper the attempt keys already use (`auth_attempt_key`, with
  `attempt_secret`). Give each kind of hash its own domain label.
- User decision I006/r1: add required `AUTH_ATTEMPT_SECRET`, independent of `JWT_SECRET`, for
  attempts and confirmation codes. Keep it stable during JWT rotation; use no JWT fallback.
- Read that helper first, and confirm it is an HMAC.

**Tests:**
- A code issued before a JWT-secret change still confirms.
- A code for one kind never matches another.

### 4 — Shared ports break the module cycles

**Today:**
- `use_cases ↔ workflow`: `workflow/authorization.rs:3-5` imports `ChannelPersistence`,
  `PrincipalAccessPersistence` and `ThreadPersistence` from `use_cases`. Meanwhile
  `use_cases/schedule.rs:16` and `builtin_agent_library.rs:9` import `application::workflow`.
- `use_cases ↔ transport`: `transport/compose.rs:33` imports
  `use_cases::integration::ChannelBindingPersistence`, while `use_cases/thread` imports `transport`.
- The root cause is that ports are defined inside the use-case modules that implement behaviour.

**Change:**
- Move exactly the ports that participate in a cycle into a shared application module, for example
  `application::ports`.
- Leave every other port where it is.

**Tests:** compilation, plus the existing suites of the moved ports' consumers.

### 5 — Workers out of HTTP state

**Today:**
- `init_app_state` (`src/infra/setup.rs:50`, 254 lines) returns `AppState`
  (`src/adapters/http/app_state.rs:38-101`), which has 37 public fields and 23 `FromRef` impls.
- `main.rs:208-275` pulls the workers back out of it.
- `delivery_queue` is documented as "Not used by any handler".

**Change:**
- Return `{ http: AppState, workers }`, and remove the worker-only fields from `AppState`.
- Split `init_app_state` along its phases (persistence, use cases, workers, HTTP state), using
  synchronous helpers where possible.
- Keep the startup and shutdown ordering in `main.rs` as it is.

**Tests:**
- The lifecycle tests in `main.rs` and `src/infra/`.
- `scripts/stack-budget.sh`, since setup and worker construction change.

### 6 — One derivation in one place

- **The channel address.** It is re-`format!`ed in `pages/simulation.rs:170`, `:1073` and
  `pages/channels.rs:368`, although `Channel::inbound_address`
  (`src/domain/entities/channel.rs:220-223`) exists so pages don't. Use it.
- **The Message-ID.** It is built in application (`thread/mod.rs:877-880`), although
  `egress.rs:224-230` says only the email adapter should. Move it behind the egress port.
- **Reply headers (verify first).** `message_runtime.rs:254-259` stores reply headers without the
  renderer's "add the parent to References" step (`egress.rs:259`). If the stored row differs from
  what was sent, store what the renderer produces.

### Size hotspots

Split a file listed here only when this plan already edits it, along the seam shown
(`src/AGENTS.md`, "Keep the file and its test module navigable"). Production lines exclude inline
tests.

| File | Prod lines | Seam |
|---|---|---|
| `adapters/http/pages/mailbox.rs` | 2940 | shell, top bar and rail · thread column · message pane and composers |
| `application/use_cases/thread/mod.rs` | 1383 | `ThreadPersistence` port · bounce and help notices · canonical and relay ingest into `ingest/` |
| `adapters/http/routes/channel.rs` | 1373 | channel CRUD · simulation and SSE |
| `adapters/http/pages/simulation.rs` | 1236 | compose and routing report · outcome cards · live view |
| `application/use_cases/user.rs` | 1192 | port · confirmation and code hashing · external identity · profile; move the 1,221-line test module to a sibling file |
| `adapters/http/routes/ui.rs` | 1180 | page handlers · SSE streams · compose and reply |
| `adapters/http/pages/dashboard.rs` | 1127 | panels · scope and range pickers |
| `adapters/persistence/workflow/provisioning.rs` | 1003 | authority and preparation · `provision_on` writes · `AssistantChannelService` |

## Focused checks

There are no schema changes, so a database is needed only for the suites that use one:

```sh
cargo check --locked --offline --all-targets
createdb mail_agents_fix_app_07
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_07" \
  cargo test --locked --offline --lib spam
# repeat with smtp::, use_cases::channel, use_cases::user, use_cases::thread, transport::, infra::
dropdb mail_agents_fix_app_07
```

Run `cargo check` after each batch, since every batch moves types. Confirm a nonzero test count for
each filter.

## Final gate

Run `scripts/check.sh` once (it includes the stack budget). Record the commands and results, and
name any gate that could not run.
