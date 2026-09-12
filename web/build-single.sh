#!/usr/bin/env bash
# Build the whole tool into ONE html file that runs from file://.
#
# Why one file. A page assembled from several parts can be tampered with a
# part at a time, and a hash published for it is a hash of whichever part you
# happened to check. One file has one hash: the user runs `shasum -a 256` on
# the thing actually sitting on their disk and compares it with the release —
# no devtools, no chain of subresources, no server in the equation at all.
#
# Why it cannot just be index.html renamed. From `file://` a document has an
# opaque origin, and both of the ways this page normally gets its code are
# network requests subject to that:
#
#   * `import` in a `<script type="module">` fetches, and is refused;
#   * `WebAssembly.instantiateStreaming(fetch(...))` fetches, and is refused.
#
# So the modules are concatenated into one classic script and the wasm is
# carried as base64 in the page. Nothing else changes: the same index.html,
# the same JavaScript, the same wasm bytes. This script only removes the
# module plumbing.
#
# What survives file:// unchanged, and was measured rather than assumed: the
# two video players. They read `blob:` URLs the document makes for itself, and
# a document can always read its own blobs — full seeking included.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="${OUT:-dist}"
mkdir -p "$OUT"

# ── The wasm ──────────────────────────────────────────────────────────────
# Paths are remapped out of the binary. Without this the build embeds the
# builder's home directory — measured: 7 occurrences of /Users/<name> — so two
# people building the same commit get different bytes and a published hash is
# a number nobody can check.
#
# This is necessary and not yet sufficient: the toolchain version is not
# pinned, so a different rustc still produces different bytes. Pinning it is
# the remaining step before the hash below means anything across machines.
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
RUSTFLAGS="--remap-path-prefix=$HOME/.rustup=/rustup \
           --remap-path-prefix=$HOME/.cargo=/cargo \
           --remap-path-prefix=$PWD=/src" \
  cargo build --release -p edit-report-wasm --target wasm32-unknown-unknown

WASM=target/wasm32-unknown-unknown/release/edit_report_wasm.wasm
if strings -a "$WASM" 2>/dev/null | grep -q "$HOME"; then
  echo "refusing to build: the wasm still embeds $HOME" >&2
  exit 1
fi

# ── Assemble ──────────────────────────────────────────────────────────────
python3 - "$WASM" "$OUT/edit-report.html" <<'PY'
import base64, re, sys, pathlib

wasm_path, out_path = sys.argv[1], sys.argv[2]
web = pathlib.Path(__file__).parent / "web" if False else pathlib.Path("web")

def strip_module(src: str) -> str:
    """Remove the module plumbing, and nothing else.

    Deliberately blunt: drop `import` lines and the `export ` keyword. Anything
    cleverer would be a transform whose output nobody has read, in a tool whose
    whole argument is that its code can be read.
    """
    out = []
    for line in src.splitlines():
        if re.match(r"\s*import\s.*from\s+['\"]\./", line):
            continue
        out.append(re.sub(r"^export\s+", "", line))
    return "\n".join(out)

mp4 = strip_module((web / "mp4.js").read_text())
glue = strip_module((web / "edit-report.js").read_text())
page = (web / "index.html").read_text()

b64 = base64.b64encode(pathlib.Path(wasm_path).read_bytes()).decode()

# The page's own bootstrap is a module that imports; it becomes a classic
# script with the imports gone and everything in one scope.
inline = re.search(r'<script type="module">(.*?)</script>', page, re.S)
if not inline:
    raise SystemExit("index.html no longer has its module bootstrap")
bootstrap = strip_module(inline.group(1))

banner = """
// ─────────────────────────────────────────────────────────────────────────
// edit-report — single file. Everything below is the same source as the
// served build, with the module plumbing removed so it runs from file://.
// The wasm module is carried as base64 a few lines down and decoded in
// memory; nothing here reaches the network, ever.
// ─────────────────────────────────────────────────────────────────────────
"""

combined = (
    "<script>\n(function () {\n"
    + banner
    + "\n// The measuring module, as bytes. `atob` gives a binary string; the\n"
      "// map turns it into the array WebAssembly.instantiate wants.\n"
      f'const __WASM_B64 = "{b64}";\n'
    + "window.__EDIT_REPORT_WASM__ = Uint8Array.from(atob(__WASM_B64), c => c.charCodeAt(0));\n"
    + "\n// ── web/mp4.js ──\n" + mp4
    + "\n// ── web/edit-report.js ──\n" + glue
    + "\n// ── the page's own bootstrap ──\n" + bootstrap
    + "\n})();\n</script>"
)

page = page[: inline.start()] + combined + page[inline.end() :]

# Say on the page what it is, so someone who opens it in a year knows what
# they are holding and how to check it.
page = page.replace(
    "</h1>",
    "</h1>\n<p class=\"caveat\">Single file, no network. Check it against the "
    "published hash with <code>shasum -a 256</code> on this file.</p>",
    1,
)

pathlib.Path(out_path).write_text(page)
print(f"{out_path}  {len(page) / 1024:.0f} kB")
PY

if command -v sha256sum >/dev/null; then
  sha256sum "$OUT/edit-report.html" | tee "$OUT/edit-report.html.sha256"
else
  shasum -a 256 "$OUT/edit-report.html" | tee "$OUT/edit-report.html.sha256"
fi

echo
echo "Open it directly — no server:"
echo "    open $OUT/edit-report.html"
