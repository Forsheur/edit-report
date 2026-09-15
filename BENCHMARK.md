# What else is out there, and what it reports on the same corpus

This document exists so that nobody has to take the claims in
[README.md](README.md) on trust. Every number below came out of a command in
[`benchmark/run.sh`](benchmark/run.sh), which anyone can re-run. Where a
competitor does something better than this project does, that is stated in the
same voice as everything else — a bench that only produces favourable results
is a marketing document, and this one found several results that are not.

**The rules this project works under apply to this page too.** There is no
score, no ranking, no total and no winner. A tool that reports less than
another is not worse; it was built to answer a different question, and the
tables below are grouped by question rather than by tool for that reason. And
the three states are used here as they are used in a report:

| state | meaning |
|---|---|
| `reported` | the tool produced a reading that locates the thing |
| `silent` | the tool ran and produced no reading about it |
| **`cannot-express`** | the question cannot be put to this tool at all |

The third state carries most of the weight. Asking a tool that emits one
64-bit hash for a whole recording *where the cut is* has no answer — and
writing "found nothing" in that box would read as a clearance the tool never
gave. `silent` is not a clearance either.

---

## Reproducing this

```bash
cd fixtures
./fetch.sh
SEED_SHORT=vXcAFFLUBpCa OTHER_SHORT=vYvhiLEQ3pUm OUT=derived_bench ./derive_bitrow.sh
cd ..
cargo build --release
./benchmark/run.sh                 # ~25 min, writes benchmark/results.{tsv,md}
```

`run.sh --short-only` skips the 86 s corpus; `--no-install` never touches pip.
A tool that will not install is recorded as a tool that would not install.
Nothing it downloads is committed: everything lands under `fixtures/derived_bench/`
and `fixtures/derived_bench_long/`, which `.gitignore` already covers.

Measured on macOS 26.2 / Apple Silicon, ffmpeg 8.1, Python 3.12.

### Three measurement bugs found while building this, all of the same kind

Named because they are the failure mode this whole bench exists against — a
number that looks measured and is not. Two were in `run.sh` and are fixed; the
third was in the analysis and produced a conclusion that was briefly believed.

* `ffmpeg -filters | grep -q ' signature '` reports the filter **absent when it
  is present**. `grep -q` exits at the first match, ffmpeg dies of `SIGPIPE`,
  and `set -o pipefail` turns that into a failed pipeline. The first full run
  of this bench skipped the most important competitor entirely, and said so in
  a line that looked like a legitimate skip.
* Under a `fr_FR` locale, `awk` parses `10.093539` as `10`. Every timing in the
  first run was silently rounded to the second. `run.sh` now sets `LC_ALL=C`; a
  bench whose numbers depend on the operator's locale is not reproducible.
* Counting outcomes with `grep -q ' matching of video'` **also matches `no
  matching of video`**, because the substring is there. For about ten minutes
  this produced a table showing that ffmpeg's signature filter matched every
  case including the unrelated recording — a spectacular result, entirely an
  artefact of testing the two branches in the wrong order. The lesson is the
  one this project applies to video: a reading that is surprising is a reading
  to go and check, not to publish.

---

## The two corpora, and why there are two

**Short corpus** — the one the project's fixtures build: seven manipulations
of `vXcAFFLUBpCa`, 1280×720, 18.73 s, 562 frames, plus a four-step
recompression ladder.

**Long corpus** — the same seven manipulations scaled onto `3gvqtL7Y7HZL`,
720×1280, 85.87 s, 2576 frames. It exists because of a measured fact, not a
preference: **ffmpeg's MPEG-7 signature filter reports `no matching` between a
file and itself below roughly 875 frames.** Judging it only on 18.7 s of
material would be judging it where it structurally cannot work, and that would
be the same dishonesty in the other direction.

The long seed carries no machine-readable strip — it predates it — so on that
corpus this tool falls back to its blind path (`core/src/align.rs`), which is
precisely the part being compared against MPEG-7 signature. That makes the long
corpus a fair fight and the short corpus an unfair one in our favour; both are
reported.

Caveat that applies to every row of the long corpus equally: `3gvqtL7Y7HZL`
measures 16 % self-similarity, so offsets on it are approximate by construction
(see `fixtures/README.md`). It handicaps us and the competitors alike.

---

## What this project reports, measured today

Not copied from a previous session. These are the lines `run.sh` produced on
the short corpus, where the burn-in is present and the declaration path runs:

| case | ground truth | reported |
|---|---|---|
| case1-faithful | re-encode at 4 Mbit/s, nothing else | 1 shot, 0 joins, 562 frames even, 0 located |
| case2-cut | 6 s removed at 8 s | 2 shots, join at **0:07.98**, frame 240 → 421, **181 frames absent, 6.00 s** |
| case3-foreign | 1 s of another recording at 9 s | 2 shots, join at 0:09.49, stretch **0:08.97→0:10.01 corresponding to nothing** |
| case4-reordered | halves swapped | 2 shots, **backwards** join at 0:08.72, frame 562 → frame 1 |
| case5-swap | a different recording | 0 shots, 0/97 frames matched |
| case6-retouched | 320×144 patch at (16.6 %, 55.0 %), 6→10 s | 1 shot, 0 joins, region **0:06.00→0:10.00, frames 181–301, 31 % × 22 % at (12 %, 55 %)**, peak 96.0 deviations |
| case7-one-frame | frame 300 blurred | 1 shot, 0 joins, **f=301 alone at 10.00 s**, 27 deviations |
| ladder ×4 | recompression only | 1 shot, 0 joins, 0 located, on all four |

Two honest corrections to the numbers this bench was handed:

* **case6's coordinates are tile-quantised and over-state the patch.** The box
  really sits at (16.6 %, 55.0 %) and measures 25 % × 20 %; the report says
  (12 %, 55 %) and 31 % × 22 %. On a 16×16 grid a tile is 80 px wide, and a
  region is reported as the tiles it touches. The reading is correct as "the
  tiles containing the change" and wrong as "the change".
* **case3 fires section 7 at 0.67 s, nowhere near the 9 s insertion.** Frames
  21–25, 26 deviations. That is the x264 rate control settling at the start of
  the file, it appears in *every* re-encoded file in the corpus, and the report
  labels it `[opening of the file]` rather than hiding it.

On the long corpus, with no strip to read, the blind path alone gives:

| case | reported | ground truth |
|---|---|---|
| L1-faithful | 1 segment, 0 cuts | correct |
| L2-cut | 2 segments, cut at copy **30.00 s**, original left 29.87 → resumed **50.17 s** | cut at 30 s, 20 s removed — correct to 0.17 s |
| L3-foreign | 2 segments, stretch **39.73→43.20 s** corresponding to nothing | insert at 40→43 s — correct to 0.3 s |
| L4-reordered | 2 segments, offsets **+42.97 s** and **−42.93 s**, backwards join at 42.80 s | split at 43 s — correct to 0.2 s |
| L5-swap | 0 segments, 0/1520 frames matched | correct |
| **L6-retouched** | **3 segments, 2 cuts at 30.93 s and 36.00 s** | **there is no cut. This is wrong** |
| L7-one-frame | 1 segment, 0 cuts | the blurred frame is invisible to the blind path |
| ladder ×3 | 1 segment, 0 cuts, on all three | correct |

**L6 is a false positive in our own blind path, and it is the worst kind.** A
black patch held over 10 s of a strip-less recording breaks the perceptual
hashes enough to split the correspondence, and the report then contains two
joins that nobody made. The two spurious cuts sit at 30.93 s and 36.00 s,
inside the 30→40 s retouch. On the short corpus, where the strip is readable,
the same manipulation is reported correctly as one shot with a located region —
so the failure is specific to the blind path, which is exactly the path a
platform re-encode leaves you with.

---

## The state of the art, with its receipts

Counted 13 September 2026 from the GitHub API. "Commits" is the repository's
total, which is the cheapest honest proxy for whether a thing was built or
posted.

### Tools that compare a video against a reference

| project | ★ | commits | last push | what it actually measures | what it emits |
|---|---|---|---|---|---|
| [FFmpeg](https://github.com/FFmpeg/FFmpeg) `signature` | 64 185 | 126 538 | 2026-09-13 | MPEG-7 Video Signature (ISO/IEC 15938-3): 380 bits per frame from fixed region sums, plus coarse 90-frame block signatures | one line: `matching of video 0 at T0 and 1 at T1, N frames matching`, or `no matching` |
| [FFmpeg](https://github.com/FFmpeg/FFmpeg) `ssim` / `psnr` | — | — | — | frame *n* against frame *n*, pixel by pixel | a global average, plus one line per frame if asked |
| [Netflix/vmaf](https://github.com/Netflix/vmaf) | 5 478 | 2 273 | 2026-08-14 | the best-engineered member of that family: several elementary metrics fused by a model trained on human opinion scores | **one number, 0–100** — a quality score for an encoder ladder, by design |
| [thorn-oss/perception](https://github.com/thorn-oss/perception) | 204 | 506 | 2026-08-24 | a toolkit of perceptual hashers with a benchmarking harness, video hashers included | hashes and a benchmarking report, not a comparison of two files |
| [facebook/ThreatExchange](https://github.com/facebook/ThreatExchange) `vpdq` | 1 388 | 2 196 | 2026-09-11 | PDQ, 256 bits per sampled frame, designed to match subsequences | a list of frame hashes; the matching logic is in the C++ side, not in the PyPI binding |
| [akamhy/videohash](https://github.com/akamhy/videohash) | 382 | 228 | **2024-07-12** | frames tiled into one collage image, then `imagehash.whash` of the collage | **one 64-bit hash for the whole recording** |
| [SpangleLabs/vidhash](https://github.com/SpangleLabs/vidhash) | **4** | 85 | 2026-01-13 | dHash per sampled frame, then a run of consecutive agreeing frames | **`True` or `False`** |

### Tools that look at one video with no reference

| project | ★ | commits | last push | what it measures | what it emits |
|---|---|---|---|---|---|
| [Breakthrough/PySceneDetect](https://github.com/Breakthrough/PySceneDetect) | 5 174 | 1 709 | 2026-09-12 | HSL change between consecutive frames | a shot list with frame-accurate boundaries |
| [FFmpeg](https://github.com/FFmpeg/FFmpeg) `scdet` | — | — | — | the same idea, one filter | timestamps of boundaries |

### The duplicate-detection world, which has more resources and a neighbouring problem

| project | ★ | commits | last push | note |
|---|---|---|---|---|
| [qarmin/czkawka](https://github.com/qarmin/czkawka) | 33 444 | — | 2026-09-09 | mainstream duplicate finder; its "Similar Videos" mode depends on `similario_core` per `czkawka_core/Cargo.toml` |
| [idealo/imagededup](https://github.com/idealo/imagededup) | 5 668 | 569 | 2025-08-15 | images only; the video story is "extract frames yourself" |
| [0x90d/videoduplicatefinder](https://github.com/0x90d/videoduplicatefinder) | 3 650 | 970 | 2026-09-05 | C#/.NET, thumbnail-grid comparison, a GUI product |
| [JohannesBuchner/imagehash](https://github.com/JohannesBuchner/imagehash) | 3 870 | 353 | 2026-09-05 | the pHash implementation half this field is built on |
| [Farmadupe/vid_dup_finder_lib](https://github.com/Farmadupe/vid_dup_finder_lib) | 25 | — | 2025-04-23 | Rust; a spatial-temporal hash library with its own CLI, the closest thing here to a reusable crate |

Also in this neighbourhood:
[iscc/iscc-core](https://github.com/iscc/iscc-core) (25 ★, last push
2026-05-19), which computes an ISCC "Content-Code" for video — a similarity-
preserving identifier heading for an ISO standard. It is an identifier scheme,
not a comparison: two codes can be near each other, and nothing in the format
says where the files diverge.

They all answer "are these the same video?" and stop there, because that is
what deduplication needs. None of them is built to answer "where do they stop
being the same?", which is the only question that matters here. Their maturity
is real and their output is a boolean or a distance.

**VMAF deserves singling out.** It is 5 478 stars and 2 273 commits of serious
engineering, it is what every encoding team actually uses, and it produces one
number between 0 and 100. It was built to compare an encode against its source
for quality, which is a different job from deciding whether somebody changed
the picture — and a single fused score is precisely the shape of output this
project refuses to emit, because it is the shape that gets quoted out of
context. It was not run: it needs the same alignment and identical geometry
`ssim` needs, so on this corpus it would answer where `ssim` already answered.

### The "video forensics tool" genre, checked rather than assumed

The prediction that most of these are two-commit student projects was put to
the API rather than repeated:

| project | ★ | commits | last push |
|---|---|---|---|
| [LitZeus/VidForensicsTool](https://github.com/LitZeus/VidForensicsTool) | 7 | **2** | 2024-10-13 |
| [siddeshkankrale/Video-forensics](https://github.com/siddeshkankrale/Video-forensics) | 2 | **4** | 2025-05-11 |

Two commits and four commits. A GitHub search for `video forensics` returns
pages of these. By their own descriptions, the first is a Streamlit front end
over metadata examination, frame-by-frame analysis, deepfake detection and
watermark authentication; the second "calculates a unique MD5 hash to detect
tampering". An MD5 changes when a video is re-encoded, which happens to every
video that is ever shared, so it answers "is this the same file", never "is
this the same recording". Neither was run: there is no reference-comparison
path in either to put the corpus through.

### What it took to make each of them run, which is part of "is it maintained"

| project | installed cleanly? |
|---|---|
| ffmpeg filters | already present; nothing to install |
| PySceneDetect | `pip install scenedetect` — clean |
| vidhash | `pip install vidhash` — clean, but it pins `numpy<2` while PySceneDetect pulls a newer one into the same environment; `run.sh` installs then re-pins |
| **akamhy/videohash** | **no.** It calls `PIL.Image.ANTIALIAS`, removed in Pillow 10 in July 2023. `AttributeError` on the first video. Upstream has not shipped a fix in the two years since; last push 2024-07-12. It is measured here behind a three-line shim in [`benchmark/videohash_run.py`](benchmark/videohash_run.py) |
| **vpdq** | **not by default.** It builds a C++ extension against `libav*` and its CMake step fails with `A required package was not found` unless `PKG_CONFIG_PATH` points at them. `run.sh` sets it before deciding the package is uninstallable |

The videohash case is the one worth dwelling on. A 382-star package that
*cannot import* against a two-year-old Pillow is not a package anyone is
running in production, and a bench that quietly skipped it would have hidden
that. Recording it as "would not install" and then measuring it anyway behind a
documented shim is the only version of this that tells a reader both things.

### Two families named and deliberately not tested

**Passive single-video forensics** — ELA, sensor-noise residue, double-JPEG and
double-compression traces, inter-frame inconsistency. It answers a different
question: *does this file bear the marks of having been edited*, with no
original to compare against. It is a real research field and it is not a
competitor to a tool that is handed a sealed original. It also has the failure
mode this project is built to avoid: its outputs are usually a heat map or a
probability, and both read as verdicts.

**Cryptographic provenance — C2PA / Content Credentials**
([contentauth/c2pa-rs](https://github.com/contentauth/c2pa-rs), 419 ★, 1 873
commits, 2026-09-12). Serious, industrial, and solving the adjacent problem:
it signs a manifest and attaches it *to the file*. The attachment is the
difference. A C2PA manifest does not survive the first re-encode a platform
performs, and a stripped manifest is indistinguishable from a file that never
had one. This project starts from the opposite assumption — that the copy in
front of you has been through several platforms and carries nothing — and
compares it against an original anchored somewhere else. The two are
complementary, not rivals.

---

## The questions, answered by measurement

### Which of them find the cut in case2, and to what precision?

| tool | state | reading | true answer |
|---|---|---|---|
| edit-report | `reported` | join at 0:07.98, 181 frames absent, 6.00 s | 8.00 s, 180 frames |
| PySceneDetect | `reported` | **boundary at 8.000 s**, frame 240 / 241 | exact |
| ffmpeg `scdet` | `reported` | **boundary at 8.000 s** | exact |
| ffmpeg `signature` | `silent` | `no matching` — the same words it gives for the faithful copy and for an unrelated recording at this length | — |
| ffmpeg `ssim` | `cannot-express` | compares frame *n* to frame *n*; after the cut everything is misaligned, so it reports a low global average and no position | — |
| vpdq | `reported` | 100 % of frames matched, but only 40 of 64 at offset 0 | the discontinuity is in its data, not in its output |
| vidhash | `cannot-express` | `check_match=True` | — |
| videohash | `cannot-express` | distance 19 of 64 | — |

**Two competitors locate the cut to the exact frame, and one of them needs no
original at all.** `ffmpeg -vf scdet` is one line of shell. So is
`scenedetect -i copy.mp4 detect-content list-scenes`. On this case they are not
worse than we are; on timing they are frame-exact, the same as us.

What they do not do is say what the cut *means*. Neither of them ever sees the
original, so neither can say that 180 frames of it are missing, nor that the
material either side came from one recording. That distinction is the next
question.

### Which of them distinguish case1 (faithful) from case5 (another recording)?

| tool | case1-faithful | case5-swap | separates? |
|---|---|---|---|
| edit-report | 1 shot, 562 frames matched | 0 shots, 0/97 matched | yes |
| vpdq | 100 % of frames matched | **no frame within distance 31 of any reference frame** | yes, cleanly |
| vidhash | `check_match=True`, 94/94 | `check_match=False`, 0/129 | yes, cleanly |
| ffmpeg `ssim` | Y 0.9769 | Y 0.2525 | yes — **but see below** |
| videohash | distance 1, `is_similar=True` | distance 30, `is_similar=False` | yes — **but see below** |
| ffmpeg `signature` (18.7 s) | `no matching` | `no matching` | **no: identical output for both** |
| ffmpeg `signature` (86 s) | 2576 frames matching, offset 0 | `no matching` | yes, cleanly |
| PySceneDetect / `scdet` | `cannot-express` — they never see the original | | |

Two of the "yes" answers do not survive contact with the recompression ladder,
and that is the next question.

### Which of them cry wolf on the 150 kbit/s recompression?

Nothing was done to this file. It is the original, re-encoded badly, which is
what circulation does to a video. It is the case where a false reading costs
the most.

| tool | reading on the 150 kbit/s file | reading on a real manipulation | what the pair shows |
|---|---|---|---|
| edit-report | 1 shot, 0 joins, 0 located | case6: region located, peak 96 deviations | separates |
| **PySceneDetect** | **two boundaries, at 8.333 s and 16.667 s** | case2: boundary at 8.000 s | **invents two cuts in an untouched file** |
| ffmpeg `scdet` | no boundary | case2: 8.000 s | separates |
| **ffmpeg `ssim` (global)** | **Y 0.456** | case6-retouched: Y 0.960 | **the untouched file scores far worse than the retouched one** |
| **ffmpeg `psnr`** | **19.0 dB** | case6-retouched: 26.6 dB | same inversion |
| **videohash** | **distance 10 of 64** — its default limit is `ceil(0.15 × 64)` = **exactly 10** | case4-reordered: distance 13 | **the untouched file lands exactly on the threshold; one more bit and it is rejected** |
| **vpdq** | **49 % of frames matched** | case6-retouched: 84 % matched | **the untouched file matches worse than the retouched one** |
| vidhash | `check_match=True` | `check_match=True` | does not cry wolf, does not distinguish either |
| ffmpeg `signature` (86 s) | 2576 frames matching | L6: 2576 frames matching | does not cry wolf |
| ffmpeg `ssim` (per frame) | no frame above 8 deviations | case7: f=301 at 103.9 | separates |

**This is the single clearest result in the bench.** Four of these readings —
global SSIM, PSNR, videohash's distance, vpdq's match rate — rank a file that
nobody touched as *more different from the original* than a file with a black
rectangle pasted into it for four seconds. A fifth, PySceneDetect, invents two
cuts in it. Any threshold drawn on a global similarity number condemns the
innocent recompression before it catches the manipulation. That is not a tuning
problem to be fixed with a better constant; it is what happens when one number
has to carry both questions.

PySceneDetect deserves its own line. It is a 5 174-star, 1 709-commit, actively
maintained project and it is very good at what it does. On the 150 kbit/s file
it reports two shot boundaries that do not exist, because at that bitrate the
frame-to-frame HSL change from blocking exceeds its threshold. It is not
malfunctioning — it was asked whether the picture changes abruptly, and at
150 kbit/s it does.

And on an **untouched original**, the 86 s recording that nobody edited at all:

| tool | boundaries reported on the unedited original |
|---|---|
| PySceneDetect | **5** — at 6.167, 14.633, 21.167, 77.802, 80.735 s |
| ffmpeg `scdet` | **2** — at 77.368, 77.735 s |

Neither tool is wrong: something in the scene does change at 77 s. But a reader
handed that list, next to a list from an edited copy, has no way to tell which
entries are edits. `scdet` on `L2-cut` returns `30 · 57.37 · 57.73`: the first
is the cut, the other two are that same scene change, moved 20 s earlier by the
removal. The three are printed in one list, in the same format, with nothing to
separate them.

### Which of them see anything on case6 (a local retouch) and case7 (one blurred frame)?

The bet was that nobody would. **The bet is lost on case7, and half-lost on
case6.**

| tool | case6 — 320×144 patch, 6→10 s | case7 — frame 300 blurred |
|---|---|---|
| edit-report | `reported`: 103 frames, 31 % × 22 % at (12 %, 55 %), peak 96.0 deviations | `reported`: f=301 alone at 10.00 s, 27 deviations |
| **ffmpeg `ssim` per frame + a MAD rule** | `silent` — peaks at 6.0 deviations, below an 8 bar | **`reported`: f=301 at t=10.00 s, 103.9 deviations below the file's own median** |
| **vpdq** | **`reported` in effect: 79/94 frames matched against 94/94 for the faithful copy** | `silent`: 94/94, invisible |
| ffmpeg `ssim` (global) | Y 0.960 against 0.977 for the faithful copy | Y 0.968 against 0.977 |
| videohash | `silent`: distance 1, identical to the faithful copy | `silent`: distance 1 |
| vidhash | `silent`: `check_match=True`, 86/94 | `silent`: `check_match=True`, 94/94 |
| PySceneDetect, `scdet`, `signature` | `silent` on both | `silent` on both |
| all of them | `cannot-express`: none of them says **where in the picture** | — |

**`ffmpeg -lavfi ssim` finds the substituted frame, at the same frame number
and the same timestamp this project reports.** The recipe is
`ssim=stats_file=out.log` followed by an outlier rule over the per-frame
column — about fifteen lines of Python, which is
[`benchmark/ssim_outliers.py`](benchmark/ssim_outliers.py). Anyone claiming
that finding a single substituted frame requires this project is wrong, and the
measurement is in `fixtures/derived_bench/bench/ssim/`.

The difference is what the same rule does to the rest of the corpus:

| file | strongest per-frame SSIM outlier | what it is |
|---|---|---|
| case7-one-frame | f=5 at **144.9** deviations, f=3 at 129.6, then f=301 at 103.9 | two encoder artefacts **ranked above** the real finding |
| case1-faithful | f=5 at **220.9** deviations, f=3 at 198.3 | nothing was done to this file |
| vXcAFFLUBpCa-reencode | f=5 at 220.9, f=3 at 198.3 | nothing was done to this file either |

The rule fires hardest on frames 3 and 5 of **every re-encoded file in the
corpus, including the faithful copy**. That is x264's rate control settling at
the start of the file. A reader given the raw list sees the encoder's startup
at the top and the substituted frame third. Separating those two is not a
threshold — it is knowing that an encoder artefact is a run of consecutive
frames at the start of a file while a substitution is a single-frame event,
which is what this project's section 7 reports as shape and position, and what
`ssim_outliers.py` deliberately does not try to do.

So the honest statement is narrow: **ffmpeg finds the frame; it does not
distinguish it from the two frames the encoder ruined on its own.**

And vpdq's case6 reading deserves its due: 79/94 against 94/94 for the faithful
copy is a real signal from a real tool, obtained without the burn-in. It is
also 79/94 against 46/94 for the untouched 150 kbit/s file, so the signal is
not separable from recompression by the size of the number alone.

### Where is `ffmpeg -lavfi signature`, given it is our milestone 2 in an ISO standard?

This was the candidate to beat, and it needs the longest answer.

**On the 18.7 s corpus it is unusable, and its failure is silent.** Over five
runs of each comparison, it prints `no matching of video 0 and 1` for the
faithful copy, the cut, the insertion, the reordering, the retouch, the
single-frame substitution, the 150 kbit/s recompression **and the unrelated
recording** — the same six words for a perfect copy and for a different video.
Two exceptions make it stranger rather than better: the ½-width and ⅔-width
recompressions **do** match, at 562 and 561 frames with the correct offset. A
downscaled re-encode matches; a same-size one does not.

**The cause is a length floor.** Each length below is the 86 s original
truncated to *n* frames, compared against its own 4 Mbit/s re-encode, five runs
each:

| frames | result, 5 runs |
|---|---|
| 562 — our whole short corpus | `no matching` ×5 |
| 750 | `no matching` ×5 |
| 870 | `no matching` ×5 |
| **880** | **`matching`, offset 0, all 880 frames — ×5** |
| 900 · 1200 · 2576 | `matching`, offset 0, all frames — ×5 each |

The floor sits between 870 and 880 frames on this material, about 29 s at
30 fps. Our short corpus is 562 frames, well under it.

> **A retracted measurement, left here because it is the point of the
> document.** An earlier draft established that floor by comparing a file with
> *itself* and reported it as 870–875 frames from single runs. That experiment
> is invalid. Handing ffmpeg the same path twice makes its two decoders race:
> the graph is torn down when one reaches EOF, and the filter then prints
> **nothing at all** — no match and no `no matching`. Measured on the 562-frame
> original: **9 runs out of 10 produced no signature line whatsoever**, and in
> the silent ones input 0 had decoded 522 of its 562 frames. Re-running the
> self-comparison at 870 and 875 frames gave different answers than the first
> pass had. The floor above is measured the sound way, against a distinct
> second file, and reproduces five times out of five at every length.

**There is also an input-order trap, undocumented and worth knowing.** The same
tear-down happens whenever the *second* input is the shorter one: with the
562-frame original as input 0 and the 382-frame cut copy as input 1, ffmpeg
decodes 408 of the original's 562 frames and exits 0 with no signature line at
all. Swapping the order produces a line. `run.sh` therefore always feeds the
shorter file first, which is the only invocation that reliably reports
anything.

**Above its floor, it is genuinely good, and this is the result that is least
comfortable to write.** On the 86 s corpus:

| case | `signature` reading | true answer | correct? |
|---|---|---|---|
| L1-faithful | 2576 frames matching, offset 0 | offset 0 | yes |
| L-150k | 2576 frames matching | untouched | yes, and no wolf |
| L-two-thirds (480 px) | 2576 frames matching | untouched | yes, scale-invariant |
| L-half (360 px) | 2576 frames matching | untouched | yes, scale-invariant |
| **L2-cut** | video 0 at 38.50 s, video 1 at 58.50 s → **offset +20.00 s** | exactly 20 s removed | **exact** |
| **L3-foreign** | 67.535 vs 70.533 → **offset +3.00 s** | exactly 3 s inserted | **exact** |
| L4-reordered | 5.167 vs 48.033, 1290 frames | one of the two halves | partially |
| L6-retouched | 2576 frames matching, whole | no cut exists — **correct, where our blind path invents two** | yes |
| L7-one-frame | 2576 frames matching, whole | invisible to it | expected |
| L5-swap | **`no matching`** | a different recording | yes, cleanly |

**On material long enough, `ffmpeg -lavfi signature` recovers the exact offset
of a removal and of an insertion, is invariant to a 3.6× downscale and to a
150 kbit/s recompression, correctly refuses an unrelated recording, and does it
in about ten seconds of one shell command against an ISO standard. On L6 it is
right and our blind alignment is wrong.** Anyone building this kind of tool
should try that filter before writing an aligner.

What it does not give, and why this project still has work to do:

* **One offset, not a correspondence.** For `L2-cut` it emits a single triple —
  one time in each video and a frame count. It reports the segment *after* the
  cut and says nothing about the one before it, nothing about where the join
  is, and nothing about which 1079 of 1976 frames agreed. Our blind path on the
  same file returns both segments, the cut at 30.00 s, and the original's
  29.87 → 50.17 s excision.
* **No stretch that corresponds to nothing.** On `L3-foreign` the +3.00 s
  offset is the inserted duration, which a reader has to already know to
  interpret. Our blind path names the interval 39.73→43.20 s of the copy as
  corresponding to nothing in the original, which is the thing a journalist
  needs to go and look at.
* **The floor makes its negative result unusable below ~29 s.** `no matching`
  from a tool that also says `no matching` about a file and itself carries no
  information, and nothing in the output distinguishes the two situations.
* **It is a matcher, not a report.** No three states, no statement of what was
  not established, no wording discipline. `no matching of video 0 and 1` is one
  line that a reader will quite reasonably read as "these are not the same
  video", which on an 18.7 s file is false.

---

## Where competitors do something we do not

Collected in one place so it cannot be missed.

* **`ffmpeg -lavfi signature` recovers exact edit offsets on material over
  ~29 s, in one command, against an ISO standard, and gets L6 right where our
  blind path reports two cuts that do not exist.**
* **`ffmpeg -vf scdet` and PySceneDetect locate the cut in case2 to the exact
  frame with no original at all** — the same precision we reach with a sealed
  bundle and a burn-in.
* **`ffmpeg -lavfi ssim` plus a fifteen-line outlier rule finds the substituted
  frame of case7, at the same frame and timestamp we report.**
* **vpdq separates the faithful copy from the unrelated recording perfectly and
  reacts measurably to the case6 retouch**, without any burn-in, from a
  256-bit-per-frame hash.
* `ffmpeg -vf scdet` answers in 0.27 s where we take 9.21 s, and PySceneDetect
  decodes at about 1 200 frames/s. Speed is not the point of this tool, but a
  reader deciding what to reach for first should know the difference is 34×.

And the two results that are unfavourable to this project specifically:

* **Our blind path reports two cuts in L6-retouched that nobody made.** ffmpeg's
  signature filter reports the file as one whole match, correctly.
* **Our case6 coordinates over-state the patch** by up to one 80 px tile on each
  side, and the report does not say that the extent is tile-quantised.

## Where the difference is structural rather than a matter of degree

Not a claim of superiority — a description of what was built for what.

Every tool above returns a number, a boolean, or a list of timestamps. None of
them returns a *statement of what was not established*. `no matching`, `False`,
`distance=30` and an empty scene list are all indistinguishable from "the tool
could not work here", and on the 18.7 s corpus, for `signature`, that is
precisely what they were. The three-state discipline and the `not_performed`
sections exist so that a reader can tell those apart, and no competitor in this
bench has an equivalent — nor should they, because none of them is handing its
output to somebody who may publish a conclusion from it.

The other structural difference is the sealed original. Of everything tested,
only this project is given an original whose provenance is established before
any comparison runs. `scdet` and PySceneDetect find boundaries in a file with
nothing to compare against, which is why they cannot tell a manipulation from a
scene change and why they report five boundaries in an untouched recording.
That is not a defect in them; it is the question they were asked.

---

## Timings, 18.73 s of 1280×720

Wall clock, warm cache, same machine. Not a ranking — a tool that reads every
frame twice cannot be compared with one that samples five per second.

Mean over the eleven cases, from `benchmark/results.tsv`.

| tool | mean s | range | what it read |
|---|---|---|---|
| ffmpeg `scdet` | 0.27 | 0.2–0.4 | every frame of one file |
| ffmpeg `ssim` | 0.37 | 0.1–0.5 | every frame of both files |
| ffmpeg `psnr` | 0.40 | 0.3–0.5 | every frame of both files |
| vidhash | 0.94 | 0.7–1.3 | 5 frames per second, per file hashed |
| vpdq | 1.11 | 0.5–1.5 | 5 frames per second of both files |
| videohash | 1.41 | 0.9–1.8 | a sample, tiled into one collage |
| PySceneDetect | 1.92 | 0.5–11.5 | every frame of one file; the 11.5 s is the first call, paying Python start-up |
| ffmpeg `signature` | 2.29 | 1.6–3.0 | every frame of both files |
| **edit-report** | **9.21** | 7.9–10.2 | every frame of both files, plus the bundle's Python verifier, plus the burn-in sweep |

vidhash's per-case figure counts one hashing pass; its `check_match` re-hashes
both files, so a full pass over the corpus takes about 33 s rather than the 10 s
the column suggests.

On the 86 s corpus: ffmpeg `signature` 10.6 s, edit-report 26.6 s for the blind
path alone.

The nine seconds are not free and the table is here so nobody pretends they
are. Where they go, measured on case1-faithful:

| stage | s |
|---|---|
| `verify_bundle.py`, timed on its own | **0.05** |
| bundle extraction, burn-in sweep, fingerprinting both files, alignment (`--json` alone) | 5.5 |
| the second, full-rate decode of both files that the picture comparison needs (`--html`) | +4.5 |
| total | 10.0 |

Two things that cost nothing, contrary to what one might assume: the Python
verifier, at 0.05 s, is noise — and the burn-in QR sweep is too, since dropping
it from 24 frames to 2 moves the total by less than the run-to-run spread. The
cost is decoding, twice, at full rate.

---

## What a reader should take from this

That the interesting part of this project is not the alignment. **ffmpeg does
the alignment, in an ISO standard, in one command, and on long enough material
it does it at least as well.** The parts that are not available anywhere else
in this bench are the sealed original the comparison starts from, the
localisation of a difference inside the frame, the separation of an encoder's
noise from a change somebody made, and a report format that cannot be reduced
to a number somebody quotes out of context.

Three of the five questions this bench asked were answered by a competitor as
well as by us or better. That is the useful outcome, and it is why the bench
is a file in the repository rather than a paragraph in the README.
