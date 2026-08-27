# ---- builder ----
FROM rust:1-bookworm AS builder

# git: the `ai-agents` dependency is a git dependency
RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# sqlx::query! macros are checked at compile time; use the committed .sqlx/
# metadata so the build needs no live database.
ENV SQLX_OFFLINE=true

COPY Cargo.toml Cargo.lock ./
COPY .sqlx ./.sqlx
COPY migrations ./migrations
COPY assets ./assets
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/mail_agents /app/mail_agents

# ---- runtime ----
FROM debian:bookworm-slim

# ca-certificates: outbound TLS (LLM APIs, SMTP relay, DNSBL/rspamd over HTTP)
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system mailagents \
    && useradd --system --gid mailagents --home-dir /app --shell /usr/sbin/nologin mailagents

WORKDIR /app
COPY --from=builder /app/mail_agents /app/mail_agents
RUN chown mailagents:mailagents /app/mail_agents

# 3001 = axum HTTP, 2525 = inbound SMTP listener
EXPOSE 3001 2525

USER mailagents

CMD ["/app/mail_agents"]
