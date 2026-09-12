# Fixtures

The test corpus is built from **one real Forsheur recording**, not from
synthetic video. A synthetic fixture would exercise the code and prove nothing
about H.264 at 2 Mbit/s over a moving scene, which is the only thing that
matters here.

## Choosing a seed: measure it first

A recording that looks like itself at other moments makes every alignment
result mush, and it is not obvious from watching. Measure before adopting:

    cargo run --release -p edit-report --example compare_videos -- candidate.mp4

Under 5 % is good, 5–15 % usable with approximate offsets, above that the
timestamps are weakly determined. Measured on real material: an office desk
shot scores 16 %, a phone filming a tablecloth from above scores 92 %, and
outdoor travel footage scores 0 %.

**Both of the first two seeds were pathological** and it took a third to notice.
A static shot is not a bad recording — it is a realistic one, and the tool must
keep saying honestly what it can and cannot get from it — but a corpus built
only on those measures the wrong thing.

## The seed

| | |
|---|---|
| Recording | `3gvqtL7Y7HZL` (preprod), cleartext, dual camera |
| Sealed | 2026-09-03, 19 segments, 85.9 s |
| Video | 720×1280 portrait, ~30 fps, H.264 in fragmented MP4 |
| Burn-in | full — UTC + `f=` frame counter, GPS, lens line, QR |

A second recording, `olLp8YUjH1kH` (405 s), serves as the *unrelated video*:
a genuine Forsheur recording with its own burn-in, which is a far better
negative case than arbitrary footage. It scores 92 % self-similarity and is
useful for nothing else.

### Outdoor material

Travel footage — a temple courtyard, the Great Wall, a building site, a road —
measures 0 % self-similarity and is what the alignment should be judged on.
Those recordings are **migrated archives**: they carry no notary proof, so
`evidence.zip` refuses to serve one, deliberately. Their media is still public
chunk by chunk, and `fetch_media.py` reassembles it.

That means they exercise normalisation, fingerprinting and alignment, through
`compare_videos`, and cannot exercise the tool end-to-end. Do not manufacture a
bundle around them: a made-up bundle is precisely what this project refuses.

Bundles are not committed — they are tens of megabytes of somebody's real
recording. `fetch.sh` downloads them; `derive.sh` builds every copy from the
seed with ffmpeg.

## The cases

Derived from the seed, each with the classification the report must reach:

| Fixture | Expected |
|---|---|
| re-encoded, light / medium / heavy | correspondence over the whole length; differences `consistent-with-recompression` |
| cropped | correspondence after normalisation; no burn-in read |
| letterboxed | correspondence after normalisation — the case that breaks first if step 4 is skipped |
| burned-in subtitles | correspondence; `localized-difference` in the subtitle band |
| single continuous extract | one interval, no cuts |
| three non-contiguous segments | three intervals, two cuts, timestamps in both videos |
| one region retouched | `localized-difference` with coordinates and duration |
| **one substituted frame** | a one-frame discontinuity, or `localized-difference` on that frame |
| unrelated video carrying a copied QR | code read; **no correspondence established** — and no accusatory word anywhere |
| no QR / two different QRs | reported as observed; two codes is information, not an error |
| no audio track | image-only path; correspondence still established |
| bundle whose chain fails | comparison refused, section 1 reports the failure, sections 3–5 say why they did not run |

Every case asserts two things: the expected classification, **and** that the
report contains no accusatory language. The second has its own test —
`core/tests/language.rs` — because it is the property most likely to erode one
helpful sentence at a time.

## Note while milestone 1 is current

A bundle's `verify_bundle.py` is the copy compiled into the server that
generated it, so which server a bundle came from decides whether `--json` is in
it.

| Server | `--json` | |
|---|---|---|
| dev | **yes** | verified 2026-09-10 on a freshly generated bundle |
| preprod | not yet | the seed recordings live here |
| prod | not yet | |

A bundle without it is reported as a chain this tool could not establish —
a third state, never a failure. Until preprod is redeployed, `fetch.sh`
refreshes the script from the working tree, which is exactly what a freshly
generated bundle already contains; the guard makes it a no-op once it is not.
