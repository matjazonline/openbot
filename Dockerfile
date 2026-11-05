# ---- verified local model artifacts (build network only) ----
FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS model
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /prepare
COPY scripts/prepare-knowledge-model.py ./scripts/prepare-knowledge-model.py
COPY assets/knowledge ./assets/knowledge
RUN python3 scripts/prepare-knowledge-model.py --destination /knowledge-model

# ---- builder ----
FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder
WORKDIR /app
ENV SQLX_OFFLINE=true CARGO_BUILD_JOBS=2
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
FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
# CPU ONNX Runtime needs libstdc++; ca-certificates supports application outbound TLS.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libstdc++6 libgomp1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system mailagents \
    && useradd --system --gid mailagents --home-dir /app --shell /usr/sbin/nologin mailagents
WORKDIR /app
COPY --from=builder --chmod=0555 /app/open_bots /app/open_bots
COPY --from=model /knowledge-model /app/models/multilingual-e5-small
ENV KNOWLEDGE_MODEL_DIR=/app/models/multilingual-e5-small
EXPOSE 3001 2525
USER mailagents
CMD ["/app/open_bots"]
