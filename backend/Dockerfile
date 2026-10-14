# Frontend stage: build the CSR bundle that gets embedded into the binary.
# Build from the repo root so the frontend sources are in the context:
#   docker build -f backend/Dockerfile -t woodmouse .
FROM node:26-alpine AS frontend
WORKDIR /app/frontend
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

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
