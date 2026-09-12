#!/usr/bin/env bash
# Build the browser module. One command, no toolchain to install:
# `wasm/` exports a flat C interface and the glue in edit-report.js is written
# by hand, so there is no wasm-bindgen and no generated code.
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
cargo build --release -p edit-report-wasm --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/edit_report_wasm.wasm web/

echo "web/edit_report_wasm.wasm  $(du -h web/edit_report_wasm.wasm | cut -f1)"
echo
echo "Serve it — a module cannot be fetched from a file:// URL:"
echo "    python3 -m http.server --directory web 8080"
echo
echo "Any static server will do here. The two players read the files you pick"
echo "through blob: URLs, which seek without needing range requests — unlike"
echo "the standalone report the native binary writes, which points at files on"
echo "disk and does need a server that answers 206."
