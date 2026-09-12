#!/usr/bin/env bash
# Fetch the seed bundles. They are somebody's real recording and tens of
# megabytes each, so they are downloaded rather than committed.
set -euo pipefail
cd "$(dirname "$0")"

SERVER="${FORSHEUR_SERVER:-https://preprod.forsheur.com}"
mkdir -p bundles

# short_id:session_id — the seed, and the unrelated recording used as a
# negative case.
SEEDS=(
  "3gvqtL7Y7HZL:d3153f66-fb43-4663-b3dc-0a867b66312e"
  "olLp8YUjH1kH:e399b548-f3cd-4e2c-a10b-e46a5ab4e7bf"
  # 2026-09-12, the first four recordings carrying the machine-readable strip
  # at the top of the frame — iOS and Android, portrait and landscape. The
  # strip reads on 100 % of frames on all four, which is why they are the
  # reference material for `derive_bitrow.sh`.
  "lzvYrVDnmEMQ:584b1f92-cfab-46f9-bf79-5edca345335a"   # iOS, portrait
  "ECFgZSmG0Zq1:90b267f2-03db-4a9f-93af-e425af071e75"   # iOS, landscape
  "cDPqjF4rgKHX:83b33937-f0e0-41e9-bf78-2327ef97c7c1"   # Android, portrait
  "vYvhiLEQ3pUm:446f9acf-f6c8-441e-ab7b-e48322970fba"   # Android, landscape
)

for pair in "${SEEDS[@]}"; do
  short="${pair%%:*}"; uuid="${pair##*:}"
  dir="bundles/$short"
  if [ -d "$dir" ]; then echo "$short: already present"; continue; fi
  echo "$short: downloading…"
  # evidence.zip is a public endpoint; no credential is used or wanted.
  curl -fsS --max-time 600 -o "bundles/$short.zip" \
    "$SERVER/api/v2/sessions/$uuid/evidence.zip"
  mkdir -p "$dir" && (cd "$dir" && unzip -q "../$short.zip")
  rm "bundles/$short.zip"

  # A bundle carries the verifier that was compiled into the server which
  # generated it. `--json` is live on the dev server (verified 2026-09-10: a
  # bundle fetched from it needs nothing below) but not yet on preprod, and the
  # seed recordings live on preprod. So refresh the script from the working
  # tree, which is exactly what a newly generated bundle already contains.
  #
  # Delete this block once preprod is redeployed. The `grep` guard means it is
  # already a no-op for any bundle that arrives with the flag.
  root="$(dirname "$(find "$dir" -name manifest.json | head -1)")"
  repo_verifier="../../server/verifier/verify_bundle.py"
  if [ -f "$repo_verifier" ] && ! grep -q -- '--json' "$root/verify_bundle.py"; then
    echo "$short: refreshing verify_bundle.py from the working tree (pre-deploy)"
    cp "$repo_verifier" "$root/verify_bundle.py"
  fi
  # Extract the media. Everything downstream reads `extracted/back.mp4`, and
  # the extraction is the verifier's own job — the tool never opens a chunk
  # itself, so that the picture it compares is the one the chain covers.
  (cd "$root" && python3 verify_bundle.py . --extract >/dev/null) \
    || echo "$short: verify/extract did not complete — see $root"
  echo "$short: ready at $root"
done

echo
echo "Now build the corpora (neither is committed):"
echo "    ./derive.sh          # the blind-comparison corpus"
echo "    ./derive_bitrow.sh   # the machine-readable-strip corpus"
