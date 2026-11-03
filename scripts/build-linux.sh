#!/bin/sh
# The static (musl) Linux binary of woodmouse, written to dist/ at the
# repository root, for the image's Dockerfile and the release workflow.
# cargo zigbuild makes zig the C compiler and linker (the bundled SQLite and
# ring included), so the same command works on a Mac and on Linux, for
# either architecture.
#
#   ARCH      amd64 | arm64 (default: this machine's; x86_64/aarch64 work too)
#   PROFILE   cargo profile (default: release)
#   FEATURES  cargo features (default: none)
#
# The result is dist/woodmouse-linux-<arch>, with a -<profile> suffix for any
# profile other than release.
set -eu

BIN=woodmouse
root="$(cd "$(dirname "$0")/.." && pwd)"

ARCH="${ARCH:-$(uname -m)}"
PROFILE="${PROFILE:-release}"
FEATURES="${FEATURES:-}"

case "$ARCH" in
  amd64 | x86_64) arch=amd64 triple=x86_64-unknown-linux-musl ;;
  arm64 | aarch64) arch=arm64 triple=aarch64-unknown-linux-musl ;;
  *)
    echo "unsupported ARCH: $ARCH (amd64 or arm64)" >&2
    exit 1
    ;;
esac

cd "$root/packages/backend"
cargo zigbuild --profile "$PROFILE" --features "$FEATURES" --target "$triple" --bin "$BIN"

# The machine's own target directory, which may be a central one
# (`[build] target-dir` in ~/.cargo/config.toml). Cargo has no stable flag
# that writes one artifact elsewhere (`--artifact-dir` is nightly-only, and
# `--target-dir` would leave the shared cache behind), so this copy is the
# override.
target_dir="$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
# Cargo's output directory for the built-in `dev` profile is `debug`.
profile_dir="$PROFILE"
[ "$PROFILE" = dev ] && profile_dir=debug

out="$root/dist/$BIN-linux-$arch"
[ "$PROFILE" = release ] || out="$out-$PROFILE"
mkdir -p "$root/dist"
cp "$target_dir/$triple/$profile_dir/$BIN" "$out"
echo "$out"
