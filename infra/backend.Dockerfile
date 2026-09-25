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
COPY apps/leptos-web/Cargo.toml ./apps/leptos-web/Cargo.toml
COPY apps/leptos-web/src ./apps/leptos-web/src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --locked --release -p adapter-api -p wallet -p mock-provider \
    && install -D target/release/adapter-api /out/adapter-api \
    && install -D target/release/wallet /out/wallet \
    && install -D target/release/mock-provider /out/mock-provider

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libgcc-s1 libssl3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
USER 65532:65532

FROM runtime AS adapter-api
COPY --from=build /out/adapter-api /usr/local/bin/adapter-api
COPY contracts/openapi.json /app/contracts/openapi.json
ENTRYPOINT ["/usr/local/bin/adapter-api"]

FROM runtime AS wallet
COPY --from=build /out/wallet /usr/local/bin/wallet
ENTRYPOINT ["/usr/local/bin/wallet"]

FROM runtime AS mock-provider
COPY --from=build /out/mock-provider /usr/local/bin/mock-provider
ENTRYPOINT ["/usr/local/bin/mock-provider"]
