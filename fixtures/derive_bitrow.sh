#!/usr/bin/env bash
# Build the machine-readable-strip corpus from two real recordings.
#
# Nothing here is committed. The inputs are somebody's real recording and the
# outputs are hundreds of megabytes; both are derived on demand, which is why
# `/fixtures/derived*/` is ignored. If you need the corpus, run this.
#
# Two families:
#
#   * the EDIT cases — a faithful copy, a removal, an insertion of foreign
#     footage, a reordering. Each is a thing somebody actually does to a video,
#     and `expected.tsv` records what the report has to reach for it.
#
#   * the DEGRADATION ladder — the ordinary fate of a video that circulates.
#     Every one of these must come back as ONE shot with NO cut: they are the
#     cases where a false accusation would be most damaging, because nothing
#     was done to them.
#
# Usage:
#     ./derive_bitrow.sh
#     SEED_SHORT=xxxx OTHER_SHORT=yyyy OUT=derived_mine ./derive_bitrow.sh
set -euo pipefail
cd "$(dirname "$0")"

# Two recordings from 2026-09-12, both carrying the strip at the top of the
# frame: an iPhone portrait one as the seed, an iPhone landscape one to splice
# from. `fetch.sh` knows how to download them.
SEED_SHORT="${SEED_SHORT:-lzvYrVDnmEMQ}"
OTHER_SHORT="${OTHER_SHORT:-ECFgZSmG0Zq1}"
OUT="${OUT:-derived_bitrow_top}"
mkdir -p "$OUT"

root_of() {
  local m
  m="$(find "bundles/$1" -name manifest.json 2>/dev/null | head -1)"
  [ -n "$m" ] && dirname "$m"
}

seed_root="$(root_of "$SEED_SHORT" || true)"
other_root="$(root_of "$OTHER_SHORT" || true)"
SRC="${seed_root:-/nonexistent}/extracted/back.mp4"
ALIEN="${other_root:-/nonexistent}/extracted/back.mp4"

if [ ! -f "$SRC" ]; then
  echo "The seed $SEED_SHORT is not extracted. Run:"
  echo "    ./fetch.sh"
  echo "    (cd bundles/$SEED_SHORT/forsheur-evidence-$SEED_SHORT && python3 verify_bundle.py . --extract)"
  exit 1
fi

ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
say() { printf '  %-32s %s\n' "$1" "$2"; }
: > "$OUT/expected.tsv"
note() { printf '%s\t%s\n' "$1" "$2" >> "$OUT/expected.tsv"; }

# Frame geometry of the seed, needed for the scaled copies: libx264 refuses an
# odd width, and `scale=iw*2/3` produced 853 and failed outright the first time.
W=$(ffprobe -v error -select_streams v:0 -show_entries stream=width -of csv=p=0 "$SRC" | tr -d ',')
even() { echo $((($1 / 2) * 2)); }

echo "Deriving from $SRC (${W}px wide)"

# ── 1 · A faithful copy ───────────────────────────────────────────────────
# Re-encoded, because that is what happens to a file that travels. It is still
# the same recording end to end, and the report must say so with one shot.
ff -i "$SRC" -c:v libx264 -b:v 4000k -preset fast -an "$OUT/case1-faithful.mp4"
say "case1-faithful.mp4" "re-encode at 4 Mbit/s"
note "case1-faithful.mp4" "1 shot, 0 cuts, 0 differing"

# ── 2 · A removal ─────────────────────────────────────────────────────────
# 8s → 14s taken out and the two ends joined. The burned frame numbers jump
# forward by ~180 while the copy's clock does not, which is what a removal is.
ff -i "$SRC" -filter_complex \
  "[0:v]trim=0:8,setpts=PTS-STARTPTS[a];[0:v]trim=14,setpts=PTS-STARTPTS[b];[a][b]concat=n=2:v=1[v]" \
  -map "[v]" -c:v libx264 -b:v 3000k "$OUT/case2-cut.mp4"
say "case2-cut.mp4" "6 s removed at 8 s"
note "case2-cut.mp4" "2 shots, 1 join, ~180 frames of the original absent, 0 differing"

# ── 4 · A reordering ──────────────────────────────────────────────────────
# The second half placed first. The frame numbers go BACKWARDS at the join,
# which no capture ever does.
ff -i "$SRC" -filter_complex \
  "[0:v]trim=10,setpts=PTS-STARTPTS[a];[0:v]trim=0:10,setpts=PTS-STARTPTS[b];[a][b]concat=n=2:v=1[v]" \
  -map "[v]" -c:v libx264 -b:v 3000k "$OUT/case4-reordered.mp4"
say "case4-reordered.mp4" "halves swapped"
note "case4-reordered.mp4" "2 shots, 1 backwards join, 0 differing"

# ── 3 · An insertion, and 5 · a swapped recording ─────────────────────────
if [ -f "$ALIEN" ]; then
  # A second of the OTHER recording, turned into the seed's geometry so the
  # splice is not given away by a shape change. Rotating and padding destroys
  # its strip, which is the honest situation: inserted footage usually carries
  # no readable number, and what proves the insertion is the time the copy
  # spends where the original does not.
  ff -i "$ALIEN" -t 1 -vf "transpose=1,scale=720:1280:force_original_aspect_ratio=decrease,pad=720:1280:(ow-iw)/2:(oh-ih)/2" \
     -c:v libx264 -b:v 3000k -an "$OUT/.foreign-snippet.mp4"
  ff -i "$SRC" -i "$OUT/.foreign-snippet.mp4" -filter_complex \
    "[0:v]trim=0:9,setpts=PTS-STARTPTS[a];[1:v]setpts=PTS-STARTPTS[m];[0:v]trim=9,setpts=PTS-STARTPTS[b];[a][m][b]concat=n=3:v=1[v]" \
    -map "[v]" -c:v libx264 -b:v 3000k "$OUT/case3-foreign.mp4"
  rm -f "$OUT/.foreign-snippet.mp4"
  say "case3-foreign.mp4" "1 s of another recording at 9 s"
  note "case3-foreign.mp4" "2 shots, 1 join reported as ~1 s inserted, a stretch corresponding to nothing"

  # Case 5 needs no file: it is the OTHER recording presented against the
  # SEED's bundle. Its own strip carries a different signature, and that
  # settles it before a single picture is compared.
  say "case5-swap" "no file — run $OTHER_SHORT against $SEED_SHORT's bundle"
  note "case5-swap" "0 shots; every frame carries a foreign signature"
else
  echo "  (skipping cases 3 and 5 — $OTHER_SHORT is not fetched and extracted)"
fi

# ── The degradation ladder ────────────────────────────────────────────────
# None of these is an edit. Every one must come back as one shot, no cut, and
# nothing in the "differs" list — refusals belong in the inconclusive list.
for pair in "reencode:4000k:$W" "150k:150k:$W" \
            "two-thirds:2000k:$(even $((W * 2 / 3)))" "half:2000k:$(even $((W / 2)))"; do
  IFS=: read -r name rate width <<< "$pair"
  if [ "$width" = "$W" ]; then
    ff -i "$SRC" -c:v libx264 -b:v "$rate" -preset fast -an "$OUT/$SEED_SHORT-$name.mp4"
  else
    ff -i "$SRC" -vf "scale=$width:-2" -c:v libx264 -b:v "$rate" -an "$OUT/$SEED_SHORT-$name.mp4"
  fi
  say "$SEED_SHORT-$name.mp4" "${width}px at $rate"
  note "$SEED_SHORT-$name.mp4" "1 shot, 0 cuts, 0 differing; refusals are inconclusive, never differing"
done

echo
echo "Wrote $(ls -1 "$OUT"/*.mp4 | wc -l | tr -d ' ') file(s) to $OUT/, and $OUT/expected.tsv"
echo "None of it is committed: /fixtures/derived*/ is ignored."
