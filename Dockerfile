# ---- builder ----
FROM rust:1-bookworm AS builder

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
    --mount=type=cache,target=/app/target \
    cargo build --release --locked \
    && cp target/release/open_bots /app/open_bots

# ---- runtime ----
FROM debian:bookworm-slim

# ca-certificates: outbound TLS (LLM APIs, SMTP relay, DNSBL/rspamd over HTTP)
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system mailagents \
    && useradd --system --gid mailagents --home-dir /app --shell /usr/sbin/nologin mailagents

WORKDIR /app
COPY --from=builder /app/open_bots /app/open_bots
RUN chown mailagents:mailagents /app/open_bots

# 3001 = axum HTTP, 2525 = inbound SMTP listener
EXPOSE 3001 2525

USER mailagents

CMD ["/app/open_bots"]
