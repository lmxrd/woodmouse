# The image has four stages: the OpenAPI spec, the frontend build (which
# generates its API client from the spec), the cargo build of the binary
# (which embeds the frontend), and the runtime, which is Alpine with just
# the binary.
#
# Build:   mise run build:docker — for the architecture of the machine,
# natively, so an Apple Silicon Mac gets arm64 without emulation. The
# release builds amd64 and arm64 on native runners.

# The OpenAPI spec: the frontend's API client is generated from it, so the
# backend builds first, without the frontend (the `specs` binary is not part
# of the main binary that embeds it) and only far enough to write the spec.
# It shares the cargo caches with the compile stage. build-base is the C
# toolchain for the bundled SQLite.
FROM rust:1.99-alpine AS spec
RUN apk add --no-cache build-base
WORKDIR /app/backend
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/backend/target \
    cargo run --bin specs \
    && cp target/specs/openapi.json /openapi.json

# The frontend: the client is generated from the spec (it is never
# committed), Vite builds the app, and rust-embed embeds the result. The
# workspace manifests come first so the dependency layer is cached.
FROM node:26-alpine AS frontend
RUN npm install --global pnpm@12.11.2
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
COPY frontend/packages/client/package.json packages/client/
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile
COPY frontend/ ./
# openapi-ts.config.ts reads ../../../backend/target/specs/openapi.json.
COPY --from=spec /openapi.json /app/backend/target/specs/openapi.json
RUN pnpm --dir packages/client exec openapi-ts && pnpm build

# The compile: the release binary on Alpine (musl, so the binary is static
# and the target follows the build architecture). rust-embed wants
# ../frontend/dist next to the crate. The cargo registry and target
# directory are cache mounts: a source change recompiles only the crate.
FROM rust:1.99-alpine AS compile
RUN apk add --no-cache build-base
WORKDIR /app/backend
COPY backend/ ./
COPY --from=frontend /app/frontend/dist /app/frontend/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/backend/target \
    cargo build --release --bin woodmouse \
    && cp target/release/woodmouse /woodmouse

# The runtime: Alpine, the binary, and a writable /data for the database,
# the uploaded files and the backups. ca-certificates is what sending mail
# over TLS needs.
FROM alpine:3
RUN apk add --no-cache ca-certificates \
  && addgroup -S woodmouse && adduser -S -G woodmouse woodmouse \
  && mkdir /data && chown woodmouse /data
COPY --from=compile /woodmouse /usr/local/bin/woodmouse
ENV PORT=8000 \
    DATABASE_URL=sqlite:/data/woodmouse.db \
    DATA_DIR=/data \
    BACKUP_DIR=/data/backups
VOLUME /data
EXPOSE 8000

# The healthcheck: the backend answers /api/health with 200 OK when the
# database is reachable. Shell form, so $PORT expands; wget is busybox's.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:$PORT/api/health" || exit 1

USER woodmouse
ENTRYPOINT ["/usr/local/bin/woodmouse"]
