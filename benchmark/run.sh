#!/usr/bin/env bash
# Comparative bench: what other open-source tools report on the same corpus.
#
# Every row of the table in BENCHMARK.md comes from a command in this file.
# Nothing is written from memory, and a tool that will not install is recorded
# as a tool that would not install rather than quietly dropped.
#
# Three outcome states, and the third is not optional:
#
#   reported         the tool produced a reading that locates the thing
#   silent           the tool ran and produced no reading about it
#   cannot-express   the question cannot be put to this tool at all — asking a
#                    single 64-bit whole-video hash WHERE a cut is has no
#                    answer, and calling that "silent" would read as a
#                    clearance the tool never gave
#
# Nothing here scores a tool, ranks them, or totals anything. A tool that says
# less is not a worse tool; it is a tool that was asked a different question.
#
# Usage:
#     ./benchmark/run.sh                  # everything
#     ./benchmark/run.sh --short-only     # skip the 86 s corpus
#     ./benchmark/run.sh --no-install     # never touch pip
#
# Outputs, none of them committed (fixtures/derived*/ is ignored):
#     fixtures/derived_bench/bench/       short-corpus artefacts and logs
#     fixtures/derived_bench_long/bench/  long-corpus artefacts and logs
#     benchmark/results.tsv               one line per tool per case
#     benchmark/results.md                the table BENCHMARK.md quotes

set -uo pipefail   # deliberately not -e: a competitor that fails is a result

# Every number below goes through awk or bc. Under a locale whose decimal
# separator is a comma, awk reads "10.093539" as 10 and the bench silently
# reports different timings on a French machine than on an English one. A
# benchmark that is not locale-independent is not reproducible.
export LC_ALL=C

cd "$(dirname "$0")/.."
ROOT="$PWD"

SHORT_ONLY=0
NO_INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --short-only) SHORT_ONLY=1 ;;
    --no-install) NO_INSTALL=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

FIX="$ROOT/fixtures"
SHORT="$FIX/derived_bench"
LONG="$FIX/derived_bench_long"
OUT="$ROOT/benchmark"
TSV="$OUT/results.tsv"
VENV="$SHORT/venv"

SEED_SHORT_ID=vXcAFFLUBpCa
OTHER_SHORT_ID=vYvhiLEQ3pUm
SEED_LONG_ID=3gvqtL7Y7HZL
OTHER_LONG_ID=olLp8YUjH1kH

# ── plumbing ──────────────────────────────────────────────────────────────
say()  { printf '\n\033[1m== %s\033[0m\n' "$*"; }
note() { printf '   %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

: > "$TSV"
printf 'tool\tcorpus\tcase\tstate\treading\tseconds\n' >> "$TSV"

# record <tool> <corpus> <case> <state> <reading> <seconds>
record() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "$6" >> "$TSV"
}

# Wall-clock seconds of a command, without depending on GNU time.
secs_since() { echo "$(date +%s.%N) - $1" | bc | awk '{printf "%.1f", $0}'; }

bundle_dir() {
  local m
  m="$(find "$FIX/bundles/$1" -name manifest.json 2>/dev/null | head -1)"
  [ -n "$m" ] && dirname "$m"
}

media_of() {
  local d
  d="$(bundle_dir "$1")" || return 1
  [ -n "$d" ] && [ -f "$d/extracted/back.mp4" ] && echo "$d/extracted/back.mp4"
}

frames_of() {
  ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames \
          -of csv=p=0 "$1" 2>/dev/null | tr -d ','
}

# ── prerequisites ─────────────────────────────────────────────────────────
say "Prerequisites"
for tool in ffmpeg ffprobe python3 bc; do
  if have "$tool"; then note "$tool  $(command -v "$tool")"
  else echo "   MISSING: $tool — this bench cannot run without it" >&2; exit 1; fi
done
note "ffmpeg $(ffmpeg -version 2>/dev/null | head -1 | cut -d' ' -f3)"

# Not `ffmpeg -filters | grep -q`: grep -q closes the pipe on its first match,
# ffmpeg dies of SIGPIPE, and under `pipefail` the pipeline then reports
# failure precisely when the filter IS present. The filter list is captured
# first so nothing is judged by an exit status that says the opposite.
FILTERS="$(ffmpeg -hide_banner -filters 2>/dev/null)"
if printf '%s\n' "$FILTERS" | grep -q ' signature '; then
  HAVE_SIGNATURE=1
else
  note "this ffmpeg has no 'signature' filter — that family will be skipped"
  HAVE_SIGNATURE=0
fi

ORIG_SHORT="$(media_of "$SEED_SHORT_ID")"
OTHER_SHORT_MEDIA="$(media_of "$OTHER_SHORT_ID")"
if [ -z "${ORIG_SHORT:-}" ]; then
  cat >&2 <<MSG
   The seed bundle is not extracted. Run:
       cd fixtures && ./fetch.sh
       (cd bundles/$SEED_SHORT_ID/forsheur-evidence-$SEED_SHORT_ID \\
          && python3 verify_bundle.py . --extract)
MSG
  exit 1
fi
note "short seed   $ORIG_SHORT ($(frames_of "$ORIG_SHORT") frames)"

ORIG_LONG="$(media_of "$SEED_LONG_ID")"
OTHER_LONG_MEDIA="$(media_of "$OTHER_LONG_ID")"
if [ "$SHORT_ONLY" = 0 ] && [ -z "${ORIG_LONG:-}" ]; then
  note "long seed $SEED_LONG_ID not extracted — the long corpus will be skipped"
  SHORT_ONLY=1
fi
[ "$SHORT_ONLY" = 0 ] && note "long seed    $ORIG_LONG ($(frames_of "$ORIG_LONG") frames)"

# ── the short corpus, 18.7 s ──────────────────────────────────────────────
say "Short corpus (18.7 s, 562 frames)"
if [ -f "$SHORT/expected.tsv" ] && [ -f "$SHORT/case7-one-frame.mp4" ]; then
  note "already built — delete $SHORT to rebuild"
else
  ( cd "$FIX" && SEED_SHORT="$SEED_SHORT_ID" OTHER_SHORT="$OTHER_SHORT_ID" \
      OUT=derived_bench ./derive_bitrow.sh ) || exit 1
fi
mkdir -p "$SHORT/bench/logs" "$SHORT/bench/ssim" "$SHORT/bench/psd"

# ── the long corpus, 85.9 s ───────────────────────────────────────────────
# Its reason for existing is measured, not assumed: ffmpeg's MPEG-7 signature
# filter reports "no matching" between a file and ITSELF below roughly 875
# frames, so judging it on an 18.7 s corpus judges it where it cannot work.
# The manipulations are the same seven, scaled to the length.
build_long() {
  local src="$1" alien="$2" o="$3"
  local ff=(ffmpeg -hide_banner -nostats -loglevel error -y)
  mkdir -p "$o"
  local w h
  read -r w h <<< "$(ffprobe -v error -select_streams v:0 \
      -show_entries stream=width,height -of csv=p=0:nk=1 "$src" | tr '\n,' '  ')"
  : > "$o/expected.tsv"
  lnote() { printf '%s\t%s\n' "$1" "$2" >> "$o/expected.tsv"; }

  "${ff[@]}" -i "$src" -c:v libx264 -b:v 4000k -preset fast -an "$o/L1-faithful.mp4"
  lnote L1-faithful.mp4 "re-encode at 4 Mbit/s; one shot, no cut"

  "${ff[@]}" -i "$src" -filter_complex \
    "[0:v]trim=0:30,setpts=PTS-STARTPTS[a];[0:v]trim=50,setpts=PTS-STARTPTS[b];[a][b]concat=n=2:v=1[v]" \
    -map "[v]" -c:v libx264 -b:v 3000k "$o/L2-cut.mp4"
  lnote L2-cut.mp4 "20 s removed at 30 s; two shots, one join"

  "${ff[@]}" -i "$src" -filter_complex \
    "[0:v]trim=43,setpts=PTS-STARTPTS[a];[0:v]trim=0:43,setpts=PTS-STARTPTS[b];[a][b]concat=n=2:v=1[v]" \
    -map "[v]" -c:v libx264 -b:v 3000k "$o/L4-reordered.mp4"
  lnote L4-reordered.mp4 "halves swapped at 43 s; one backwards join"

  if [ -n "$alien" ] && [ -f "$alien" ]; then
    local aw ah rotate=""
    read -r aw ah <<< "$(ffprobe -v error -select_streams v:0 \
        -show_entries stream=width,height -of csv=p=0:nk=1 "$alien" | tr '\n,' '  ')"
    if { [ "$w" -gt "$h" ] && [ "$aw" -lt "$ah" ]; } || \
       { [ "$w" -lt "$h" ] && [ "$aw" -gt "$ah" ]; }; then rotate="transpose=1,"; fi
    "${ff[@]}" -i "$alien" -t 3 -vf \
      "${rotate}scale=$w:$h:force_original_aspect_ratio=decrease,pad=$w:$h:(ow-iw)/2:(oh-ih)/2" \
      -c:v libx264 -b:v 3000k -an "$o/.foreign.mp4"
    "${ff[@]}" -i "$src" -i "$o/.foreign.mp4" -filter_complex \
      "[0:v]trim=0:40,setpts=PTS-STARTPTS[a];[1:v]setpts=PTS-STARTPTS[m];[0:v]trim=40,setpts=PTS-STARTPTS[b];[a][m][b]concat=n=3:v=1[v]" \
      -map "[v]" -c:v libx264 -b:v 3000k "$o/L3-foreign.mp4"
    rm -f "$o/.foreign.mp4"
    lnote L3-foreign.mp4 "3 s of another recording at 40 s"
    lnote L5-swap "no file — the other recording against this bundle"
  fi

  # Fractions of the frame, never pixels: a box in pixels falls outside a
  # differently shaped seed and the case silently stops testing anything.
  local bx=$((w / 6)) by=$((h * 55 / 100)) bw=$((w / 4)) bh=$((h / 5))
  "${ff[@]}" -i "$src" \
    -vf "drawbox=x=$bx:y=$by:w=$bw:h=$bh:color=black:t=fill:enable='between(t,30,40)'" \
    -c:v libx264 -b:v 3000k -an "$o/L6-retouched.mp4"
  lnote L6-retouched.mp4 "${bw}x${bh} patch at ($bx,$by), 30 s to 40 s"

  "${ff[@]}" -i "$src" -vf "gblur=sigma=8:enable='between(n,1500,1500)'" \
    -c:v libx264 -b:v 3000k -an "$o/L7-one-frame.mp4"
  lnote L7-one-frame.mp4 "frame 1500 blurred"

  local even_two_thirds=$(( ((w * 2 / 3) / 2) * 2 ))
  local even_half=$(( ((w / 2) / 2) * 2 ))
  for pair in "150k:150k:$w" "two-thirds:2000k:$even_two_thirds" "half:2000k:$even_half"; do
    IFS=: read -r name rate width <<< "$pair"
    if [ "$width" = "$w" ]; then
      "${ff[@]}" -i "$src" -c:v libx264 -b:v "$rate" -preset fast -an "$o/L-$name.mp4"
    else
      "${ff[@]}" -i "$src" -vf "scale=$width:-2" -c:v libx264 -b:v "$rate" -an "$o/L-$name.mp4"
    fi
    lnote "L-$name.mp4" "recompression only; one shot, no cut"
  done
}

if [ "$SHORT_ONLY" = 0 ]; then
  say "Long corpus (85.9 s, 2576 frames)"
  if [ -f "$LONG/L7-one-frame.mp4" ]; then
    note "already built — delete $LONG to rebuild"
  else
    build_long "$ORIG_LONG" "${OTHER_LONG_MEDIA:-}" "$LONG"
  fi
  mkdir -p "$LONG/bench/logs"
fi

# ── us ────────────────────────────────────────────────────────────────────
# Run first, so the bench states our own numbers as measured today rather than
# as remembered from a previous session.
say "edit-report (this project)"
BIN="$ROOT/target/release/edit-report"
if [ ! -x "$BIN" ]; then
  note "not built — run: cargo build --release"
  record edit-report short all not-run "binary absent" -
else
  B_SHORT="$(bundle_dir "$SEED_SHORT_ID")"
  for f in "$SHORT"/*.mp4; do
    n="$(basename "$f" .mp4)"; t0=$(date +%s.%N)
    "$BIN" --bundle "$B_SHORT" --video "$f" \
           --html "$SHORT/bench/ours-$n.html" > "$SHORT/bench/logs/ours-$n.log" 2>&1
    s=$(secs_since "$t0")
    reading="$(grep -oE 'correspondence: [^;]*' "$SHORT/bench/logs/ours-$n.log" | head -1)"
    record edit-report short "$n" reported "${reading:-no line}" "$s"
    printf '   %-26s %5ss  %s\n' "$n" "$s" "${reading:-no line}"
  done
  if [ -n "${OTHER_SHORT_MEDIA:-}" ]; then
    t0=$(date +%s.%N)
    "$BIN" --bundle "$B_SHORT" --video "$OTHER_SHORT_MEDIA" \
           --html "$SHORT/bench/ours-case5-swap.html" \
           > "$SHORT/bench/logs/ours-case5-swap.log" 2>&1
    s=$(secs_since "$t0")
    reading="$(grep -oE 'correspondence: [^;]*' "$SHORT/bench/logs/ours-case5-swap.log" | head -1)"
    record edit-report short case5-swap reported "${reading:-no line}" "$s"
    printf '   %-26s %5ss  %s\n' "case5-swap" "$s" "${reading:-no line}"
  fi

  if [ "$SHORT_ONLY" = 0 ]; then
    B_LONG="$(bundle_dir "$SEED_LONG_ID")"
    for f in "$LONG"/*.mp4; do
      n="$(basename "$f" .mp4)"; t0=$(date +%s.%N)
      "$BIN" --bundle "$B_LONG" --video "$f" \
             --json "$LONG/bench/ours-$n.json" > "$LONG/bench/logs/ours-$n.log" 2>&1
      s=$(secs_since "$t0")
      reading="$(grep -oE 'correspondence: [^;]*' "$LONG/bench/logs/ours-$n.log" | head -1)"
      record edit-report long "$n" reported "${reading:-no line}" "$s"
      printf '   %-26s %5ss  %s\n' "$n" "$s" "${reading:-no line}"
    done
    if [ -n "${OTHER_LONG_MEDIA:-}" ]; then
      t0=$(date +%s.%N)
      "$BIN" --bundle "$B_LONG" --video "$OTHER_LONG_MEDIA" \
             --json "$LONG/bench/ours-L5-swap.json" \
             > "$LONG/bench/logs/ours-L5-swap.log" 2>&1
      s=$(secs_since "$t0")
      reading="$(grep -oE 'correspondence: [^;]*' "$LONG/bench/logs/ours-L5-swap.log" | head -1)"
      record edit-report long L5-swap reported "${reading:-no line}" "$s"
      printf '   %-26s %5ss  %s\n' "L5-swap" "$s" "${reading:-no line}"
    fi
  fi
fi

# ── ffmpeg: MPEG-7 video signature ────────────────────────────────────────
# Input order matters and it is not documented: ffmpeg tears the filtergraph
# down when the SHORTER input reaches EOF, and the filter then never reports
# at all. Feeding the shorter file as input 0 is the only invocation that
# always produces a line, so that is what is used here.
signature_pair() {
  local a="$1" b="$2" fa fb t
  fa="$(frames_of "$a")"; fb="$(frames_of "$b")"
  if [ "${fa:-0}" -gt "${fb:-0}" ]; then t="$a"; a="$b"; b="$t"; fi
  ffmpeg -hide_banner -nostats -loglevel info -i "$a" -i "$b" \
    -filter_complex "[0:v][1:v]signature=nb_inputs=2:detectmode=full" \
    -f null - 2>&1 | grep -oE '(no )?matching of video[^$]*' | head -1
}

run_signature() {
  local corpus="$1" orig="$2" dir="$3" other="$4" prefix="$5"
  for f in "$dir"/*.mp4; do
    local n; n="$(basename "$f" .mp4)"; local t0; t0=$(date +%s.%N)
    local r; r="$(signature_pair "$orig" "$f")"; local s; s=$(secs_since "$t0")
    local state=reported
    case "$r" in
      "")             state=silent; r="filter produced no line at all" ;;
      "no matching"*) state=silent ;;
    esac
    record "ffmpeg-signature" "$corpus" "$n" "$state" "$r" "$s"
    printf '   %-26s %5ss  %s\n' "$n" "$s" "$r"
  done
  if [ -n "$other" ]; then
    local t0; t0=$(date +%s.%N)
    local r; r="$(signature_pair "$orig" "$other")"; local s; s=$(secs_since "$t0")
    local state=reported; case "$r" in ""|"no matching"*) state=silent ;; esac
    record "ffmpeg-signature" "$corpus" "${prefix}5-swap" "$state" "${r:-no line}" "$s"
    printf '   %-26s %5ss  %s\n' "${prefix}5-swap" "$s" "${r:-no line}"
  fi
}

if [ "$HAVE_SIGNATURE" = 1 ]; then
  say "ffmpeg -lavfi signature (MPEG-7 Video Signature, ISO/IEC 15938-3)"
  run_signature short "$ORIG_SHORT" "$SHORT" "${OTHER_SHORT_MEDIA:-}" case
  [ "$SHORT_ONLY" = 0 ] && \
    run_signature long "$ORIG_LONG" "$LONG" "${OTHER_LONG_MEDIA:-}" L

  # The length floor, measured rather than asserted.
  #
  # NOT by comparing a file with itself. Feeding ffmpeg the same path twice
  # makes the two decoders race, the graph is torn down before one of them
  # finishes, and the filter then prints NOTHING — measured at 9 runs out of
  # 10 on the 562-frame original. A single run of that experiment is not a
  # measurement, and an earlier version of this bench published a floor drawn
  # from exactly that. Each length is therefore compared against its own
  # 4 Mbit/s re-encode — two distinct files — and repeated.
  say "ffmpeg signature: the length floor, truncation vs its own re-encode"
  if [ "$SHORT_ONLY" = 0 ]; then
    for n in 562 750 870 880 900 1200 2576; do
      o="$LONG/bench/floor-o$n.mp4"; c="$LONG/bench/floor-c$n.mp4"
      ffmpeg -hide_banner -nostats -loglevel error -y -i "$ORIG_LONG" \
             -frames:v "$n" -c copy "$o" 2>/dev/null
      ffmpeg -hide_banner -nostats -loglevel error -y -i "$o" \
             -c:v libx264 -b:v 4000k -preset fast -an "$c" 2>/dev/null
      m=0; nm=0; none=0; last=""
      for _ in 1 2 3 4 5; do
        r="$(signature_pair "$o" "$c")"
        case "$r" in
          "no matching"*) nm=$((nm + 1)) ;;
          "")             none=$((none + 1)) ;;
          *)              m=$((m + 1)); last="$r" ;;
        esac
      done
      if [ "$m" -gt 0 ]; then state=reported; reading="$m/5 runs: $last"
      elif [ "$nm" -gt 0 ]; then state=silent; reading="$nm/5 runs: no matching"
      else state=silent; reading="$none/5 runs produced no line at all"; fi
      record "ffmpeg-signature" floor "${n}-frames" "$state" "$reading" -
      printf '   %5s frames : match=%d nomatch=%d no-line=%d  %s\n' \
             "$n" "$m" "$nm" "$none" "$last"
      rm -f "$o" "$c"
    done
  else
    note "needs the long seed; skipped"
  fi
fi

# ── ffmpeg: ssim and psnr ─────────────────────────────────────────────────
# Both compare frame n against frame n. They need the two files to be the same
# size and already aligned; neither condition survives an edit, which is the
# whole point of showing what they do on one.
say "ffmpeg -lavfi ssim / psnr"
for f in "$SHORT"/*.mp4; do
  n="$(basename "$f" .mp4)"; t0=$(date +%s.%N)
  log="$SHORT/bench/ssim/$n.log"
  out="$(ffmpeg -hide_banner -nostats -loglevel info -i "$ORIG_SHORT" -i "$f" \
         -lavfi "[0:v][1:v]ssim=stats_file=$log" -f null - 2>&1)"
  s=$(secs_since "$t0")
  global="$(echo "$out" | grep -oE 'SSIM.*' | head -1)"
  if [ -z "$global" ]; then
    why="$(echo "$out" | grep -oE 'Width and height of input videos must be same.*' | head -1)"
    record ffmpeg-ssim short "$n" cannot-express "${why:-filter refused to configure}" "$s"
    printf '   %-26s %5ss  REFUSED: %s\n' "$n" "$s" "${why:-see log}"
    continue
  fi
  record ffmpeg-ssim short "$n" reported "$global" "$s"
  printf '   %-26s %5ss  %s\n' "$n" "$s" "$global"
done

if [ -n "${OTHER_SHORT_MEDIA:-}" ]; then
  t0=$(date +%s.%N)
  global="$(ffmpeg -hide_banner -nostats -loglevel info -i "$ORIG_SHORT" \
            -i "$OTHER_SHORT_MEDIA" -lavfi "[0:v][1:v]ssim" -f null - 2>&1 \
            | grep -oE 'SSIM.*' | head -1)"
  s=$(secs_since "$t0")
  record ffmpeg-ssim short case5-swap reported "${global:-refused}" "$s"
  printf '   %-26s %5ss  %s\n' "case5-swap" "$s" "${global:-refused}"
fi

for n in case1-faithful case6-retouched case7-one-frame "$SEED_SHORT_ID-150k"; do
  [ -f "$SHORT/$n.mp4" ] || continue
  t0=$(date +%s.%N)
  p="$(ffmpeg -hide_banner -nostats -loglevel info -i "$ORIG_SHORT" -i "$SHORT/$n.mp4" \
       -lavfi "[0:v][1:v]psnr" -f null - 2>&1 | grep -oE 'PSNR.*' | head -1)"
  s=$(secs_since "$t0")
  record ffmpeg-psnr short "$n" reported "${p:-refused}" "$s"
  printf '   %-26s %5ss  %s\n' "$n" "$s" "${p:-refused}"
done

# Per-frame ssim with an outlier rule. This is the closest any competitor gets
# to section 7 of our report, and it is a few lines of shell — so it belongs in
# the bench, unfavourable or not.
say "ffmpeg ssim, per frame, against each file's own spread"
for log in "$SHORT/bench/ssim"/*.log; do
  [ -f "$log" ] || continue
  n="$(basename "$log" .log)"
  out="$(python3 "$OUT/ssim_outliers.py" "$log" 8 2>&1)"
  # The top THREE, not just the first. On case7 the substituted frame is third:
  # the two frames above it are x264's rate control settling at the start of the
  # file, and a table that shows only the strongest reading would say ffmpeg
  # found an encoder artefact when it also found the real one.
  hits="$(echo "$out" | sed -n '2,4p' | sed 's/^ *//;s/  */ /g' | paste -sd';' - \
          | sed 's/;/ · /g')"
  case "$out" in
    *"no per-frame lines"*)
      record ffmpeg-ssim-perframe short "$n" cannot-express \
             "the ssim filter refused these two sizes, so there is no per-frame log to read" - ;;
    *"no frame departs"*)
      record ffmpeg-ssim-perframe short "$n" silent \
             "no frame departs from this file's own spread by 8 deviations" - ;;
    *)
      record ffmpeg-ssim-perframe short "$n" reported "$hits" - ;;
  esac
  printf '   %-26s %s\n' "$n" "${hits:-see above}"
done

# ── ffmpeg: scdet, no reference at all ────────────────────────────────────
say "ffmpeg -vf scdet (one video, no original)"
scdet_of() {
  ffmpeg -hide_banner -nostats -loglevel info -i "$1" -vf "scdet=threshold=10" \
    -f null - 2>&1 | grep -oE 'lavfi\.scd\.time: *[0-9.]+' | sed 's/.*: *//' | tr '\n' ' '
}
for f in "$SHORT"/*.mp4 "$ORIG_SHORT"; do
  n="$(basename "$f" .mp4)"; [ "$f" = "$ORIG_SHORT" ] && n="ORIGINAL-untouched"
  t0=$(date +%s.%N); r="$(scdet_of "$f")"; s=$(secs_since "$t0")
  if [ -z "$r" ]; then record ffmpeg-scdet short "$n" silent "no boundary" "$s"
  else record ffmpeg-scdet short "$n" reported "boundaries at $r" "$s"; fi
  printf '   %-26s %5ss  %s\n' "$n" "$s" "${r:-no boundary}"
done
if [ "$SHORT_ONLY" = 0 ]; then
  for f in "$LONG"/*.mp4 "$ORIG_LONG"; do
    n="$(basename "$f" .mp4)"; [ "$f" = "$ORIG_LONG" ] && n="ORIGINAL-untouched"
    t0=$(date +%s.%N); r="$(scdet_of "$f")"; s=$(secs_since "$t0")
    if [ -z "$r" ]; then record ffmpeg-scdet long "$n" silent "no boundary" "$s"
    else record ffmpeg-scdet long "$n" reported "boundaries at $r" "$s"; fi
    printf '   %-26s %5ss  %s\n' "$n" "$s" "${r:-no boundary}"
  done
fi

# ── the Python family ─────────────────────────────────────────────────────
say "Python competitors"
if [ "$NO_INSTALL" = 1 ] && [ ! -x "$VENV/bin/python" ]; then
  note "--no-install and no venv — skipping videohash, vidhash and PySceneDetect"
else
  if [ ! -x "$VENV/bin/python" ]; then
    note "creating $VENV"
    python3 -m venv "$VENV" || note "venv creation failed"
  fi
  if [ -x "$VENV/bin/pip" ] && [ "$NO_INSTALL" = 0 ]; then
    "$VENV/bin/pip" install -q --upgrade pip >/dev/null 2>&1
    # vidhash pins numpy<2; scenedetect pulls a newer one in. Installing in
    # this order and then pinning numpy back is what makes all three importable
    # in one environment.
    "$VENV/bin/pip" install -q videohash vidhash scenedetect "numpy<2" \
      > "$SHORT/bench/logs/pip.log" 2>&1
    # vpdq builds a C++ extension against libav*. Without pkg-config pointing
    # at them its CMake step fails with "A required package was not found",
    # which is a build problem and not a statement about the algorithm — so it
    # gets the path it needs before being recorded as uninstallable.
    for pc in /opt/homebrew/lib/pkgconfig /usr/local/lib/pkgconfig \
              /usr/lib/x86_64-linux-gnu/pkgconfig; do
      [ -d "$pc" ] && PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}:$pc"
    done
    PKG_CONFIG_PATH="${PKG_CONFIG_PATH#:}" \
      "$VENV/bin/pip" install -q vpdq >> "$SHORT/bench/logs/pip.log" 2>&1
    note "pip log: $SHORT/bench/logs/pip.log"
  fi
fi

py_has() { [ -x "$VENV/bin/python" ] && "$VENV/bin/python" -c "import $1" >/dev/null 2>&1; }

# videohash — one 64-bit hash for the whole recording
if py_has videohash; then
  say "akamhy/videohash"
  t0=$(date +%s.%N)
  "$VENV/bin/python" "$OUT/videohash_run.py" "$ORIG_SHORT" \
      "$SHORT"/*.mp4 ${OTHER_SHORT_MEDIA:+"$OTHER_SHORT_MEDIA"} \
      > "$SHORT/bench/logs/videohash.log" 2>&1
  s=$(secs_since "$t0")
  while IFS=$'\t' read -r name hash dist similar took; do
    [ "$name" = reference ] && continue
    [ -z "${dist:-}" ] && continue
    n="${name%.mp4}"; [ "$n" = back ] && n=case5-swap
    if [ "$hash" = FAILED ]; then
      record videohash short "$n" not-run "$similar" "$took"
    else
      record videohash short "$n" reported "distance=$dist of 64, is_similar=$similar" "$took"
    fi
    printf '   %-26s %s  distance=%-3s is_similar=%s\n' "$n" "$took" "$dist" "$similar"
  done < "$SHORT/bench/logs/videohash.log"
  # Localising anything is not a question this tool can be asked.
  for q in where-is-the-cut which-region which-frame; do
    record videohash short "$q" cannot-express \
           "output is one 64-bit hash for the whole recording" -
  done
  note "total $s"
else
  say "akamhy/videohash"
  note "not importable — recorded as not-run"
  record videohash short all not-run "import failed; see $SHORT/bench/logs/pip.log" -
fi

# vidhash — per-frame hashes, boolean answer
if py_has vidhash; then
  say "SpangleLabs/vidhash"
  t0=$(date +%s.%N)
  "$VENV/bin/python" "$OUT/vidhash_run.py" "$ORIG_SHORT" \
      "$SHORT"/*.mp4 ${OTHER_SHORT_MEDIA:+"$OTHER_SHORT_MEDIA"} \
      > "$SHORT/bench/logs/vidhash.log" 2>&1
  s=$(secs_since "$t0")
  while IFS=$'\t' read -r name verdict detail took; do
    [ "$name" = reference ] && continue
    [ -z "${detail:-}" ] && continue
    n="${name%.mp4}"; [ "$n" = back ] && n=case5-swap
    if [ "$verdict" = FAILED ]; then
      record vidhash short "$n" not-run "$detail" "$took"
    else
      record vidhash short "$n" reported "$verdict, $detail" "$took"
    fi
    printf '   %-26s %s  %s  %s\n' "$n" "$took" "$verdict" "$detail"
  done < "$SHORT/bench/logs/vidhash.log"
  for q in where-is-the-cut which-region; do
    record vidhash short "$q" cannot-express "check_match returns True or False" -
  done
  note "total $s"
else
  say "SpangleLabs/vidhash"
  note "not importable — recorded as not-run"
  record vidhash short all not-run "import failed; see $SHORT/bench/logs/pip.log" -
fi

# vpdq — PDQ per frame, and the one competitor designed to say WHICH parts match
if py_has vpdq; then
  say "facebook/ThreatExchange vpdq"
  t0=$(date +%s.%N)
  "$VENV/bin/python" "$OUT/vpdq_run.py" "$ORIG_SHORT" \
      "$SHORT"/*.mp4 ${OTHER_SHORT_MEDIA:+"$OTHER_SHORT_MEDIA"} \
      > "$SHORT/bench/logs/vpdq.log" 2>&1
  s=$(secs_since "$t0")
  while IFS=$'\t' read -r name detail took; do
    [ "$name" = reference ] && continue
    [ -z "${took:-}" ] && continue
    n="${name%.mp4}"; [ "$n" = back ] && n=case5-swap
    case "$detail" in
      FAILED)   record vpdq short "$n" not-run "$took" - ;;
      "no frame within"*) record vpdq short "$n" silent "$detail" "$took" ;;
      *)        record vpdq short "$n" reported "$detail" "$took" ;;
    esac
    printf '   %-26s %s  %s\n' "$n" "$took" "$detail"
  done < "$SHORT/bench/logs/vpdq.log"
  record vpdq short which-region cannot-express \
         "a frame hash has no inside; it cannot point at a part of the picture" -
  note "total $s"
else
  say "facebook/ThreatExchange vpdq"
  note "not importable — recorded as not-run"
  record vpdq short all not-run \
         "build failed; needs libav* via pkg-config — see $SHORT/bench/logs/pip.log" -
fi

# PySceneDetect — no original, shot boundaries within one file
if [ -x "$VENV/bin/scenedetect" ]; then
  say "Breakthrough/PySceneDetect (detect-content)"
  psd_of() {
    local f="$1" base; base="$(basename "$f" .mp4)"
    "$VENV/bin/scenedetect" -i "$f" -o "$SHORT/bench/psd" \
        detect-content list-scenes -q >/dev/null 2>&1
    awk -F, 'NR>2 && $1 ~ /^[0-9]+$/ && $1>1 {print $4}' \
        "$SHORT/bench/psd/$base-Scenes.csv" 2>/dev/null | tr '\n' ' '
  }
  for f in "$SHORT"/*.mp4 "$ORIG_SHORT" ${OTHER_LONG_MEDIA:+}; do
    n="$(basename "$f" .mp4)"; [ "$f" = "$ORIG_SHORT" ] && n="ORIGINAL-untouched"
    t0=$(date +%s.%N); r="$(psd_of "$f")"; s=$(secs_since "$t0")
    if [ -z "$r" ]; then record pyscenedetect short "$n" silent "no boundary" "$s"
    else record pyscenedetect short "$n" reported "boundaries at $r" "$s"; fi
    printf '   %-26s %5ss  %s\n' "$n" "$s" "${r:-no boundary}"
  done
  if [ "$SHORT_ONLY" = 0 ]; then
    t0=$(date +%s.%N); r="$(psd_of "$ORIG_LONG")"; s=$(secs_since "$t0")
    if [ -z "$r" ]; then record pyscenedetect long ORIGINAL-untouched silent "no boundary" "$s"
    else record pyscenedetect long ORIGINAL-untouched reported "boundaries at $r" "$s"; fi
    printf '   %-26s %5ss  %s\n' "LONG-ORIGINAL-untouched" "$s" "${r:-no boundary}"
  fi
  record pyscenedetect short is-this-an-edit cannot-express \
         "it never sees the original, so a boundary it finds may always have been there" -
else
  say "Breakthrough/PySceneDetect"
  note "not installed — recorded as not-run"
  record pyscenedetect short all not-run "scenedetect binary absent" -
fi

# ── the table ─────────────────────────────────────────────────────────────
say "Writing the table"
python3 - "$TSV" "$OUT/results.md" <<'PY'
import collections, sys

tsv, md = sys.argv[1], sys.argv[2]
rows = []
with open(tsv) as fh:
    header = fh.readline()
    for line in fh:
        parts = line.rstrip("\n").split("\t")
        if len(parts) == 6:
            rows.append(parts)

by_tool = collections.OrderedDict()
for tool, corpus, case, state, reading, secs in rows:
    by_tool.setdefault((tool, corpus), []).append((case, state, reading, secs))

with open(md, "w") as out:
    out.write("<!-- Generated by benchmark/run.sh. Do not edit by hand. -->\n\n")
    out.write("# Measured readings\n\n")
    out.write("Three states. `reported` means the tool produced a reading; "
              "`silent` means it ran and produced none; `cannot-express` means "
              "the question cannot be put to that tool at all. `silent` is not "
              "a clearance and `cannot-express` is not a failure.\n\n")
    for (tool, corpus), entries in by_tool.items():
        out.write(f"## `{tool}` — {corpus} corpus\n\n")
        out.write("| case | state | reading | s |\n|---|---|---|---|\n")
        for case, state, reading, secs in entries:
            reading = reading.replace("|", "\\|")
            out.write(f"| {case} | `{state}` | {reading} | {secs} |\n")
        out.write("\n")

counts = collections.Counter(state for _, _, _, state, _, _ in rows)
print("   " + "  ".join(f"{k}={v}" for k, v in sorted(counts.items())))
print(f"   {md}")
print(f"   {tsv}")
PY

say "Done"
note "$TSV"
note "$OUT/results.md"
note "artefacts under $SHORT/bench and $LONG/bench — none of it committed"
