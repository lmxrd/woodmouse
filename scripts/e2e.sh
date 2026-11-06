#!/bin/sh
# The end-to-end smoke tests (packages/e2e, Playwright) against the container
# image, as it ships: started with a fresh database file in a volume, then
# stopped and removed whatever the result.
#
#   IMAGE      the image to test (default: woodmouse, what `mise run build:docker` makes)
#   ENGINE     podman or docker (default: podman if installed, else docker)
#   E2E_PORT   the port on this machine (default: 8180)
#
# Chromium must be installed for Playwright (`pnpm --filter woodmouse-e2e exec
# playwright install chromium`; the test:e2e task does it). Extra arguments go
# to `playwright test`.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${IMAGE:-woodmouse}"
ENGINE="${ENGINE:-$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)}"
E2E_PORT="${E2E_PORT:-8180}"
name="woodmouse-e2e-$$"
volume="woodmouse-e2e-data-$$"

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    echo "--- server log ---" >&2
    "$ENGINE" logs "$name" >&2 || true
  fi
  "$ENGINE" rm -f "$name" >/dev/null 2>&1 || true
  "$ENGINE" volume rm -f "$volume" >/dev/null 2>&1 || true
  exit "$status"
}
trap cleanup EXIT INT TERM

"$ENGINE" volume create "$volume" >/dev/null
# The image's database is /data/woodmouse.db: a real file in the volume.
"$ENGINE" run -d --name "$name" -p "127.0.0.1:$E2E_PORT:8000" -v "$volume:/data" "$IMAGE" >/dev/null

base="http://localhost:$E2E_PORT"
i=0
until curl -sf "$base/api/health" >/dev/null; do
  i=$((i + 1))
  if [ "$i" -gt 60 ]; then
    echo "the server did not become healthy" >&2
    exit 1
  fi
  sleep 1
done

cd "$root/packages/e2e"
E2E_BASE_URL="$base" pnpm exec playwright test "$@"
