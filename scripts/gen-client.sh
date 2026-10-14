#!/usr/bin/env bash
# Regenerate the heyapi client package (frontend/packages/client/src) from
# the backend's utoipa OpenAPI spec. The generated source is never committed;
# this script (plus openapi-ts.config.ts) is the source of truth. CI runs it
# before building the frontend; publish the package from here on releases.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> generating OpenAPI spec (cargo specs)"
(cd backend && cargo specs)

echo "==> installing workspace deps"
pnpm --dir frontend install --no-frozen-lockfile

echo "==> generating client (heyapi openapi-ts)"
pnpm --dir frontend/packages/client exec openapi-ts

echo "==> done: frontend/packages/client/src"
