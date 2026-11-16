FROM rust:1.95.0-bookworm@sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1 AS build
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin attribution-service \
    && cp target/release/attribution-service /app/attribution-service

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 attribution \
    && useradd --uid 10001 --gid attribution --no-create-home --shell /usr/sbin/nologin attribution
COPY --from=build /app/attribution-service /usr/local/bin/attribution-service
USER attribution:attribution
ENV ATTRIBUTION_BIND_ADDR=0.0.0.0:8091
EXPOSE 8091
ENTRYPOINT ["/usr/local/bin/attribution-service"]
CMD ["serve"]
