# Build context: this directory only (services/memory). Same pinned base images as OpenBot.
# ---- verified local model artifacts (build network only; serving never downloads) ----
FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS model
RUN apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /prepare
COPY scripts/prepare-model.py ./scripts/prepare-model.py
COPY assets/model-manifest.json ./assets/model-manifest.json
COPY assets/bundled ./assets/bundled
RUN python3 -I scripts/prepare-model.py --download

FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS builder
WORKDIR /build
ENV SQLX_OFFLINE=true CARGO_BUILD_JOBS=2
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY .sqlx ./.sqlx
COPY migrations ./migrations
COPY assets/model-manifest.json ./assets/model-manifest.json
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
    && cp target/release/memory-service /build/memory-service

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
# The CPU ONNX Runtime needs libstdc++ and libgomp.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libstdc++6 libgomp1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 memory \
    && useradd --system --uid 10001 --gid memory --home-dir /app --shell /usr/sbin/nologin memory
WORKDIR /app
COPY --from=builder --chmod=0555 /build/memory-service /app/memory-service
COPY --chmod=0444 NOTICE /app/NOTICE
# The validated model and pinned runtime, at the default MEMORY_MODEL_DIR (assets/model).
COPY --from=model /prepare/assets/model /app/assets/model
# Unprivileged port; no capability or root needed.
EXPOSE 8085
USER memory:memory
ENTRYPOINT ["/app/memory-service"]
CMD ["serve"]
