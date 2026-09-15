# syntax=docker/dockerfile:1
FROM rust:1.96.1-bookworm AS build
WORKDIR /workspace
ENV RUSTUP_TOOLCHAIN=1.96.1
RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-bindgen-cli --version 0.2.128 --locked
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Cargo workspace resolution requires manifests and targets, not backend builds.
COPY services ./services
COPY apps/leptos-web ./apps/leptos-web
WORKDIR /workspace/apps/leptos-web
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --package leptos-web --bin leptos-web --target wasm32-unknown-unknown --release --locked \
    && wasm-bindgen --target web --out-dir /dist --out-name leptos_web \
        /workspace/target/wasm32-unknown-unknown/release/leptos-web.wasm \
    && install -m 0644 index.direct.html /dist/index.html \
    && install -m 0644 style.css /dist/style.css

FROM nginx:1.28.3-alpine AS frontend
COPY infra/nginx/frontend.conf /etc/nginx/nginx.conf
COPY infra/nginx/config.js /etc/kunveno/config.js
COPY --from=build /dist /usr/share/nginx/html
USER 101:101
ENTRYPOINT ["nginx"]
CMD ["-g", "daemon off;"]
