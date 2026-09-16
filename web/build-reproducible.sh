#!/usr/bin/env bash
# Rebuild a published artefact and check it against its hash — from anywhere.
#
#     ./web/build-reproducible.sh              # the browser page
#     ./web/build-reproducible.sh --binary     # the Linux executable as well
#
# Why this exists. `web/build-single.sh` builds the page with your compiler,
# on your machine, and two machines do not agree: measured against the v0.1.0
# release, a build on macOS differs from the one GitHub published even with
# the toolchain pinned and every path remapped. Two reasons, neither of them
# a defect in this code:
#
#   * Cargo derives a crate's symbol suffixes from the full `rustc -vV`
#     output, which names the HOST triple. Same wasm32 target, different host,
#     different symbols.
#   * A native binary is linked by the SYSTEM linker. Debian's binutils and
#     Ubuntu's do not emit the same bytes.
#
# So the reproducible build is not "the same source" but "the same source in
# the same environment", and the environment has to be something anyone can
# obtain. A pinned container image is that. Measured, from a clean export of
# the v0.1.0 tag:
#
#   rust:1.93.0-bookworm  → edit-report.html           3db42152…  = published
#   ubuntu:24.04 + rustup → edit-report-linux-x86_64   f44bef0f…  = published
#
# The page is the artefact this matters most for: it is what a reader runs,
# it is one file, and it has one hash. The macOS and Windows executables
# would need the runner's own OS and Xcode/MSVC to reproduce; for those, the
# provenance attestation is what stands — see "Checking a release" in
# README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

WANT_BINARY=0
[ "${1:-}" = "--binary" ] && WANT_BINARY=1

command -v docker >/dev/null || {
  echo "docker is needed: the point is to build somewhere everyone can reach," >&2
  echo "not on this machine." >&2
  exit 1
}

# From a clean export rather than the working tree: an uncommitted change is
# precisely the thing this check must not accidentally include.
REF="${REF:-HEAD}"
SRC="$(mktemp -d)"
trap 'rm -rf "$SRC"' EXIT
git archive "$REF" | tar -x -C "$SRC"
echo "building $REF in a container, at /src"

# --platform: the published binaries are x86_64. On an Apple Silicon machine
# this runs under emulation, which is slow and correct.
docker run --rm --platform linux/amd64 -v "$SRC:/src" -w /src \
  rust:1.93.0-bookworm bash -c './web/build-single.sh' >/dev/null
digest() {
  # The name the release uses, not the temporary path it was built at.
  local sum
  sum=$( { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -d' ' -f1)
  printf '%s  %s\n' "$sum" "$2"
}
echo
digest "$SRC/dist/edit-report.html" edit-report.html

if [ "$WANT_BINARY" = 1 ]; then
  # ubuntu:24.04, because that is what `ubuntu-latest` is on GitHub's runners
  # and the system linker is part of the artefact. rustup rather than the
  # distribution's rustc: the version is pinned by rust-toolchain.toml.
  docker run --rm --platform linux/amd64 -v "$SRC:/src" -w /src ubuntu:24.04 bash -c '
    set -e
    apt-get update -qq && apt-get install -y -qq curl build-essential >/dev/null 2>&1
    curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none >/dev/null 2>&1
    export PATH=$HOME/.cargo/bin:$PATH
    rustup show >/dev/null 2>&1
    export RUSTFLAGS="-D warnings \
      --remap-path-prefix=$HOME/.rustup=/rustup \
      --remap-path-prefix=$HOME/.cargo=/cargo \
      --remap-path-prefix=$PWD=/src"
    cargo build --release --locked --target x86_64-unknown-linux-gnu -q' >/dev/null
  digest "$SRC/target/x86_64-unknown-linux-gnu/release/edit-report" edit-report-linux-x86_64
fi

cat <<'NOTE'

Compare with the release:

    gh release download v0.1.0 --repo forsheur/edit-report --pattern SHA256SUMS
    cat SHA256SUMS

Equal digests mean this source produced that file. They do not mean the
source is honest — nothing published here could establish that.
NOTE
