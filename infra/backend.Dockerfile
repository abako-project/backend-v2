# syntax=docker/dockerfile:1
FROM rust:1.96.1-bookworm AS build
WORKDIR /workspace
ENV RUSTUP_TOOLCHAIN=1.96.1
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
# Cargo resolves every workspace member; this does not build frontend assets.
COPY apps/example-web/Cargo.toml ./apps/example-web/Cargo.toml
COPY apps/example-web/src ./apps/example-web/src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --locked --release -p api -p wallet -p native-provider \
    && install -D target/release/api /out/api \
    && install -D target/release/wallet /out/wallet \
    && install -D target/release/native-provider /out/native-provider

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libgcc-s1 libssl3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
USER 65532:65532

FROM runtime AS api
COPY --from=build /out/api /usr/local/bin/api
COPY contracts/openapi.json /app/contracts/openapi.json
ENTRYPOINT ["/usr/local/bin/api"]

FROM runtime AS wallet
COPY --from=build /out/wallet /usr/local/bin/wallet
ENTRYPOINT ["/usr/local/bin/wallet"]

FROM runtime AS native-provider
COPY --from=build /out/native-provider /usr/local/bin/native-provider
ENTRYPOINT ["/usr/local/bin/native-provider"]
