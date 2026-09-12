#!/usr/bin/env bash
# Build every copy in the test corpus from the seed recording, with ffmpeg.
#
# Each output is a legitimate thing that happens to a video in the world, or a
# deliberate alteration — the file name says which, and `expected.tsv` records
# the classification the report must reach for it. Nothing here is committed;
# these are derived on demand from the bundle fetch.sh downloads.
set -euo pipefail
cd "$(dirname "$0")"

SEED_SHORT="${SEED_SHORT:-3gvqtL7Y7HZL}"
OTHER_SHORT="${OTHER_SHORT:-olLp8YUjH1kH}"
OUT=derived
mkdir -p "$OUT"

seed_root="$(dirname "$(find "bundles/$SEED_SHORT" -name manifest.json | head -1)")"
SRC="$seed_root/extracted/back.mp4"
if [ ! -f "$SRC" ]; then
  echo "The seed has not been extracted yet. Run, inside $seed_root:"
  echo "    python3 verify_bundle.py . --extract"
  exit 1
fi

ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
say() { printf '  %-34s %s\n' "$1" "$2"; }

: > "$OUT/expected.tsv"
note() { printf '%s\t%s\n' "$1" "$2" >> "$OUT/expected.tsv"; }

echo "Deriving from $SRC"

# ── Re-encoding, three severities ─────────────────────────────────────────
# The ordinary fate of a video that circulates. All three must correspond over
# the whole length; their differences are consistent-with-recompression.
for pair in "light:4000k" "medium:800k" "heavy:150k"; do
  name="${pair%%:*}"; rate="${pair##*:}"
  ff -i "$SRC" -c:v libx264 -b:v "$rate" -preset fast -c:a aac "$OUT/reencode-$name.mp4"
  say "reencode-$name.mp4" "$rate"
  note "reencode-$name.mp4" "full correspondence; consistent-with-recompression"
done

# ── Cropped: the burn-in goes with the top of the frame ───────────────────
ff -i "$SRC" -vf "crop=iw:ih*0.75:0:ih*0.25" -c:v libx264 -b:v 2000k -c:a copy "$OUT/cropped.mp4"
say "cropped.mp4" "top 25% removed — burn-in gone"
note "cropped.mp4" "correspondence after normalisation; no code read"

# ── Letterboxed: the case that breaks first if normalisation is skipped ────
ff -i "$SRC" -vf "scale=iw:ih*0.8,pad=iw:ih/0.8:0:(oh-ih)/2:black" \
   -c:v libx264 -b:v 2000k -c:a copy "$OUT/letterboxed.mp4"
say "letterboxed.mp4" "black bars top and bottom"
note "letterboxed.mp4" "correspondence after normalisation; NOT modified-everywhere"

# ── Burned-in subtitles over the bottom ───────────────────────────────────
# drawtext needs an ffmpeg built with libfreetype, which many are not. Where it
# is missing, the band is filled with glyph-shaped blocks instead: what the
# fixture has to exercise is a persistent, spatially bounded difference in a
# known region, and blocks are that. The name says which was used.
if ffmpeg -hide_banner -filters 2>/dev/null | grep -q " drawtext "; then
  ff -i "$SRC" -vf "drawbox=y=ih-140:w=iw:h=140:color=black@0.65:t=fill,\
drawtext=text='SUBTITLE OVERLAY LINE':fontcolor=white:fontsize=34:x=(w-text_w)/2:y=h-95" \
     -c:v libx264 -b:v 2000k -c:a copy "$OUT/subtitled.mp4"
  say "subtitled.mp4" "opaque caption band with text"
else
  blocks="drawbox=y=ih-140:w=iw:h=140:color=black@0.65:t=fill"
  for x in 90 150 200 275 330 385 455 510; do
    blocks="$blocks,drawbox=x=$x:y=ih-100:w=42:h=30:color=white:t=fill"
  done
  ff -i "$SRC" -vf "$blocks" -c:v libx264 -b:v 2000k -c:a copy "$OUT/subtitled.mp4"
  say "subtitled.mp4" "opaque caption band (no drawtext in this ffmpeg)"
fi
note "subtitled.mp4" "correspondence; localized-difference in the caption band"

# ── One continuous extract ────────────────────────────────────────────────
ff -ss 20 -t 15 -i "$SRC" -c:v libx264 -b:v 2000k -c:a aac "$OUT/extract-single.mp4"
say "extract-single.mp4" "20s → 35s"
note "extract-single.mp4" "one interval, no cuts"

# ── Three non-contiguous segments ─────────────────────────────────────────
for seg in "5:8:a" "40:8:b" "70:8:c"; do
  IFS=: read -r ss t tag <<< "$seg"
  ff -ss "$ss" -t "$t" -i "$SRC" -c:v libx264 -b:v 2000k -c:a aac "$OUT/.seg-$tag.mp4"
done
printf "file '.seg-a.mp4'\nfile '.seg-b.mp4'\nfile '.seg-c.mp4'\n" > "$OUT/.montage.txt"
ff -f concat -safe 0 -i "$OUT/.montage.txt" -c copy "$OUT/montage-three.mp4"
rm -f "$OUT/.seg-"*.mp4 "$OUT/.montage.txt"
say "montage-three.mp4" "3 segments, 2 cuts"
note "montage-three.mp4" "three intervals, two cuts, timestamps in both videos"

# ── A retouched region, persistent across many frames ─────────────────────
ff -i "$SRC" -vf "drawbox=x=200:y=600:w=180:h=180:color=black:t=fill:enable='between(t,30,40)'" \
   -c:v libx264 -b:v 3000k -c:a copy "$OUT/retouched-region.mp4"
say "retouched-region.mp4" "180×180 patch, 30s → 40s"
note "retouched-region.mp4" "localized-difference with coordinates and duration"

# ── ONE substituted frame ─────────────────────────────────────────────────
# The sharpest version of the substitution attack: a single frame from another
# recording, spliced into an otherwise identical copy. Everything around it
# aligns perfectly, so the tool has exactly one frame in which to notice.
other_root="$(dirname "$(find "bundles/$OTHER_SHORT" -name manifest.json 2>/dev/null | head -1)" 2>/dev/null || true)"
if [ -n "$other_root" ] && [ -f "$other_root/extracted/back.mp4" ]; then
  ff -ss 60 -i "$other_root/extracted/back.mp4" -frames:v 1 "$OUT/.alien.png"
  # Frame 1200 of the copy is replaced; the overlay-shaped inputs stay put so
  # the only thing that changed is the picture.
  ff -i "$SRC" -i "$OUT/.alien.png" \
     -filter_complex "[1:v]scale=720:1280,setsar=1[a];[0:v][a]overlay=0:0:enable='between(n,1200,1200)'" \
     -c:v libx264 -b:v 3000k -c:a copy "$OUT/substituted-one-frame.mp4"
  rm -f "$OUT/.alien.png"
  say "substituted-one-frame.mp4" "frame 1200 replaced"
  note "substituted-one-frame.mp4" "one-frame discontinuity or localized-difference on that frame"

  # ── An unrelated recording carrying a copied reference ────────────────
  # A genuine Forsheur recording of something else, wearing the seed's
  # reference. The code reads; the pictures have nothing in common. The report
  # must establish NO correspondence and accuse nobody.
  #
  # The code is RENDERED from the URL, not lifted out of the seed. A crop of
  # the real burn-in does not survive: it is composited at 80 % opacity and
  # H.264-compressed, and stretching its contrast back turns compression
  # artefacts into module errors — tried, measured, does not decode. Rendering
  # is also the more faithful attack: someone forging a reference reads the URL
  # and makes a clean code.
  #
  # Placed at 4,4 — the offset the phone uses — so the geometry matches a real
  # recording rather than being a slightly different problem.
  ( cd .. && cargo run -q -p edit-report-core --example make_qr -- \
      "preprod.forsheur.com/v/$SEED_SHORT" 87 "fixtures/$OUT/.qr.pgm" )
  ff -i "$other_root/extracted/back.mp4" -i "$OUT/.qr.pgm" \
     -filter_complex "[1:v]format=gray[q];[0:v][q]overlay=4:4" \
     -c:v libx264 -b:v 2000k -c:a copy "$OUT/unrelated-with-copied-qr.mp4"
  rm -f "$OUT/.qr.pgm"
  say "unrelated-with-copied-qr.mp4" "other recording, seed's QR"
  note "unrelated-with-copied-qr.mp4" "code read; NO correspondence established; no accusatory word"
else
  echo "  (skipping the two cases that need $OTHER_SHORT — fetch and extract it first)"
fi

# ── No audio ──────────────────────────────────────────────────────────────
ff -i "$SRC" -an -c:v libx264 -b:v 2000k "$OUT/no-audio.mp4"
say "no-audio.mp4" "video only"
note "no-audio.mp4" "image-only path; correspondence still established"

# ── No QR: the corner painted out ─────────────────────────────────────────
ff -i "$SRC" -vf "drawbox=x=0:y=0:w=100:h=100:color=black:t=fill" \
   -c:v libx264 -b:v 2000k -c:a copy "$OUT/no-qr.mp4"
say "no-qr.mp4" "burn-in corner masked"
note "no-qr.mp4" "no code read; correspondence still established"

echo
echo "Wrote $(ls -1 "$OUT"/*.mp4 | wc -l | tr -d ' ') fixtures to $OUT/ (expectations in $OUT/expected.tsv)"
