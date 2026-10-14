# Spec stage: generate the OpenAPI spec from the backend. The embedded
# frontend lives in the main binary only, so this needs no frontend bundle.
# Build from the repo root so all sources are in the context:
#   docker build -f backend/Dockerfile -t woodmouse .
FROM rust:1-alpine AS spec
WORKDIR /app

# C toolchain for rusqlite's bundled SQLite (toasty's SQLite driver).
RUN apk add --no-cache build-base

COPY backend/ ./backend/
WORKDIR /app/backend
RUN cargo build --release --bin specs && ./target/release/specs

# Frontend stage: generate the heyapi client from the spec, then build the
# bundle. The generated client (frontend/packages/client/src) is never
# committed, so it must be produced here.
FROM node:26-alpine AS frontend
RUN corepack enable
WORKDIR /app

# Workspace manifests first so pnpm install can resolve woodmouse-client.
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./frontend/
COPY frontend/packages/client/package.json ./frontend/packages/client/
COPY frontend/packages/client/openapi-ts.config.ts ./frontend/packages/client/

# The spec lands where openapi-ts.config.ts expects it
# (../../../backend/target/specs from frontend/packages/client).
COPY --from=spec /app/backend/target/specs/openapi.json ./backend/target/specs/openapi.json

WORKDIR /app/frontend
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm --dir packages/client exec openapi-ts && pnpm build

# Build stage: compile the release binary on Alpine (musl).
# rust-embed embeds ../frontend/dist, so the bundle must be in place first.
FROM rust:1-alpine AS build
WORKDIR /app

# C toolchain for rusqlite's bundled SQLite (toasty's SQLite driver).
RUN apk add --no-cache build-base

COPY backend/ ./backend/
COPY --from=frontend /app/frontend/dist ./frontend/dist

WORKDIR /app/backend
RUN cargo build --release --bin woodmouse

# Runtime stage: slim Alpine with just the binary.
FROM alpine:3
RUN apk add --no-cache ca-certificates

COPY --from=build /app/backend/target/release/woodmouse /usr/local/bin/app

# The SQLite database lives on a mounted volume.
ENV PORT=8000
ENV DATABASE_URL=sqlite:/data/woodmouse.db
VOLUME /data

EXPOSE 8000
CMD ["app"]
