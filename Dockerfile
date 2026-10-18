# The image has four build stages: the cargo dependencies (cooked by
# cargo-chef into cached layers), the OpenAPI spec (the `specs` binary,
# built from those dependencies and run), the frontend build (its API
# client is generated from the spec), and the compile of the binary that
# embeds the frontend — plus the runtime, which is Alpine with just the
# static binary.
#
# The cargo dependencies are built by cargo-chef into image layers, so
# the layer cache holds them: podman's or docker's locally (cache
# mounts, the alternative, are invisible to build caches). A source
# change recompiles only the crate; a Cargo.lock change rebuilds the
# dependency layer. Everything compiles once, for musl release — the
# specs binary is built from the same cooked dependencies as the final
# binary (it is static, so it runs anywhere).
#
# Build:   mise run build:docker — for the architecture of the machine,
# natively, so an Apple Silicon Mac gets arm64 without emulation.
# CI and the release workflow compile the binary natively on the
# runner, with a warm cargo cache, and package it from this same file,
# by overriding the `bin` stage (see it below) with the artifact
# directory: --build-context bin=<dir>.

# The toolchain, with cargo-chef for the dependency layers. curl is what
# utoipa-swagger-ui's build script downloads its bundle with at compile
# time (the slim image ships no downloader); musl-tools is the C
# toolchain for the bundled SQLite.
FROM rust:1.99-slim AS chef
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates musl-tools \
    && rustup target add "$(uname -m)-unknown-linux-musl" \
    && cargo install --locked --version 0.1.78 cargo-chef

# The recipe: the minimum manifests of the dependency build. It re-runs
# on every source change, but its output changes only when the manifests
# do — that is what keeps the cook layer below cached.
FROM chef AS planner
WORKDIR /repo/backend
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src src/
RUN cargo chef prepare --recipe-path /recipe.json

# The dependencies of the release binary (musl) as a cached layer: the
# heavy step of the build. The target follows the build architecture,
# so one file builds amd64 and arm64 natively.
FROM chef AS deps-musl
WORKDIR /repo/backend
COPY --from=planner /recipe.json recipe.json
RUN cargo chef cook --release --target "$(uname -m)-unknown-linux-musl" --recipe-path recipe.json

# The OpenAPI spec: the frontend's API client is generated from it, so
# the `specs` binary is built first and run. It needs no database
# (src/bin/specs.rs) and no frontend (src/frontend.rs is main-binary
# only); it writes into target/, so the file is copied out to /spec.
FROM deps-musl AS spec
WORKDIR /repo/backend
COPY backend/toasty toasty/
COPY backend/src src/
RUN target="$(uname -m)-unknown-linux-musl" \
    && cargo build --release --target "$target" --bin specs \
    && mkdir /spec \
    && "target/$target/release/specs" \
    && cp target/specs/openapi.json /spec/

# The frontend: the client is generated from the spec (openapi-ts reads
# it at ../../../backend/target/specs/openapi.json, so this stage mirrors
# the repo layout), then Vite builds the app that rust-embed embeds
# next.
FROM node:26-slim AS frontend
WORKDIR /repo/frontend
# Node 26 ships without corepack, so the pnpm of frontend/package.json's
# packageManager field is installed explicitly.
RUN npm install --global pnpm@12.11.2
COPY frontend/package.json frontend/pnpm-workspace.yaml frontend/pnpm-lock.yaml ./
# The pnpmfile belongs with the manifests: pnpm validates its checksum
# (pnpmfileChecksum in the lockfile) when it installs, and the hook gives
# openapi-ts a typescript that still has the JS compiler API.
COPY frontend/.pnpmfile.cjs ./
COPY frontend/packages/client/package.json packages/client/
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY frontend/ ./
COPY --from=spec /spec/openapi.json /repo/backend/target/specs/openapi.json
RUN pnpm --dir packages/client exec openapi-ts && pnpm build

# The compile: a single static binary (musl) with the frontend embedded;
# rust-embed wants ../frontend/dist next to the crate.
FROM deps-musl AS compile
COPY backend/toasty toasty/
COPY backend/src src/
COPY --from=frontend /repo/frontend/dist /repo/frontend/dist
RUN cargo build --release --target "$(uname -m)-unknown-linux-musl" \
    && cp "target/$(uname -m)-unknown-linux-musl/release/woodmouse" /woodmouse

# The binary the runtime packs: the compile stage above by default (the
# self-contained build — mise run build:docker, compose). CI and the
# release workflow swap this stage for a directory holding the prebuilt
# binary: --build-context bin=<dir>.
FROM compile AS bin

# The runtime: Alpine, the static binary, and a writable /data for the
# database (DATABASE_URL) and the uploaded files (DATA_DIR). Nothing in
# woodmouse makes an outbound TLS connection that would need system
# certificate authorities (the mailer's rustls uses webpki-roots).
FROM alpine:3
RUN addgroup -S woodmouse && adduser -S -G woodmouse woodmouse \
  && mkdir /data && chown woodmouse /data
COPY --from=bin /woodmouse /usr/local/bin/woodmouse
ENV PORT=8000 \
    DATABASE_URL=sqlite:/data/woodmouse.db \
    DATA_DIR=/data
VOLUME /data
EXPOSE 8000

# The healthcheck: the backend answers /api/health with 200 OK. Shell
# form, so $PORT expands; wget is busybox's.
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:$PORT/api/health" || exit 1

USER woodmouse
ENTRYPOINT ["/usr/local/bin/woodmouse"]
