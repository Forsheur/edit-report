# edit-report

**This is not a detector.** It produces no score, no percentage, no traffic
light, and no verdict on whether a video is genuine. It produces a *description*:
which parts of one video correspond to which parts of another, where the cuts
are, and where the picture differs.

It compares a video you were given against a **Forsheur evidence bundle** — the
`.zip` a [Forsheur](https://forsheur.com) server hands out, which carries the
sealed original, the signatures, and its own verifier.

**[Download the latest release](https://github.com/forsheur/edit-report/releases/latest)** —
`edit-report.html` runs in a browser with nothing installed; the executables do
the full comparison, chain verification included. Check what you got against
`SHA256SUMS`, and read what this tool cannot establish before using it.

---

## Read this before anything else: what this tool cannot establish

* **It says nothing about whether what the picture shows is true.** A scene can
  be staged. A witness can lie. A camera can be pointed away from what matters.
  None of that is visible to any check here. This tool is about provenance and
  integrity, and stops there.

* **A difference is not an accusation.** An edited video has a different length —
  that is what editing is. Platforms crop, subtitle, and re-encode as a matter
  of routine. This tool reports differences; it does not penalise them, and no
  output of it may be read as "this video is fake".

* **No match found means no match was found.** It does not mean the video is
  unrelated to the original, and it certainly does not mean it was falsified.
  Negative results are phrased as absence of a reading, everywhere, and there is
  a test that fails the build if that ever stops being true.

* **The burned-in overlay is pixels, not signatures.** A Forsheur recording
  burns a timestamp, a frame counter, coordinates and a QR code into every
  frame. Anyone can render the same text into their own footage. This tool uses
  the burn-in to *propose* an alignment and never, on its own, to conclude that
  two videos are related.

* **The cryptography is not checked by this tool.** It is checked by
  `verify_bundle.py`, which ships inside every bundle, and which this tool runs
  as a process and quotes verbatim. There is no second implementation here, on
  purpose — see [SECURITY.md](SECURITY.md).

* **Three states, never two.** An image difference is
  `consistent-with-recompression`, `localized-difference`, or `inconclusive`.
  The third is not a failure of the tool; it is the honest answer most of the
  time, and it is meant to be common.

---

## What it does

```
evidence bundle (.zip)  ──┐
                          ├──►  edit report  (standalone HTML + versioned JSON)
video to compare (.mp4)  ─┘
```

1. **Establish the original.** Run the bundle's own verifier. If the chain does
   not verify, stop — comparing against an original that was never established
   means nothing.
2. **Read the burn-in.** Sweep frames spread across the whole recording for the
   QR code. Report how many frames carried one and which codes they were.
   Several different codes is information to display, not an error.
3. **Normalise.** Rotation from metadata, letterbox and pillarbox removal, a
   common resolution and frame rate, variable frame rate handled. *(Milestone 2)*
4. **Align.** Audio cross-correlation for a coarse lock when both sides have
   sound, then per-frame perceptual hashing and a gap-tolerant sequence
   alignment. Produces a correspondence table; cuts fall out of its
   discontinuities. *(Milestone 2)*
5. **Compare the picture** on frames that already correspond, tile by tile,
   and classify each difference into one of the three states. Regions are
   grouped over time, because a retouch holds still and a codec's blocking
   does not. *(Milestone 3)*

### Current state

| Milestone | What it covers | Status |
|---|---|---|
| 1 | Bundle ingestion, chain state, burn-in reading, report | **done** |
| 2 | Normalisation, temporal alignment, correspondence, cuts | **done** |
| 3 | Image difference and its three-state classification | **done** |

A report contains every section at every milestone. Sections a build does not
fill carry an explicit `not_performed` state and the reason, so a section that
was not run cannot be mistaken for one that ran and found nothing. The schema
does not change shape when later milestones fill them in.

### How the picture comparison works, and its one dial

Each frame is reduced to a grid of per-tile statistics — mean, spread, and
horizontal and vertical texture, a byte each. A tile **stands out** when its
difference from the original's same tile exceeds **that frame's own** median by
more than `--sensitivity` median-absolute-deviations. Nothing is compared
against a fixed number of grey levels anywhere: a heavily recompressed frame
raises its own bar, so there is no published constant for a forger to tune
against.

Contiguous standing-out tiles become a region, and regions that overlap across
consecutive frames become a **track**. That grouping is what separates a
retouch from an encoder: measured on real material at 720×1280, a 200×200 patch
pasted over four seconds peaks at 79 deviations and holds one place for 36
frames, while the same recording merely recompressed to 150 kbit/s peaks at
11.4 in short bursts that move around. Single-frame regions are still listed —
nothing is hidden — but listed as single-frame regions, next to the persistent
ones, so the two cannot be read as the same finding.

The default sensitivity of 12 comes from those two measurements and nothing
else. It is a dial (`--sensitivity`, and a slider in the browser) precisely
because two measurements cannot be right for every camera, codec and bit rate.
`imagediff::Measure` names the statistic; adding a variant to it is the
intended way to change what is measured, and it touches nothing else.

A second question runs alongside it, per shot: **is this frame's difference
normal for this film?** A frame that was blurred, graded, re-rendered or
swapped in from another source differs *evenly*, so the question above reads it
as a recompression and it vanishes into the conforming count — measured, a
blurred frame that had lost 85 % of its texture was reported as conforming.
Each frame's overall difference is now compared with the spread of its own
shot's, at `--frame-sensitivity` deviations. It caught that frame at 26.7 and
fired on nothing else across fourteen test files. It cannot see a change
applied to the whole film, nor one inside footage too flat to measure.

The top tenth of the frame is left out: that is the burn-in band, whose content
is checked elsewhere and by checksum, and whose hard black-on-white edges are
the noisiest thing in the picture under recompression.

### How the alignment works

A stretch of copy taken from the original has one property that survives
re-encoding, rescaling and frame-rate conversion: **a constant time offset**.
Only a cut changes it. So each frame is reduced to a 64-bit perceptual hash
(pHash, DCT-based, as published — nothing invented here), pairs within a loose
distance are proposed, and offsets that many frames agree on become segments.
Cuts are the boundaries between them, read off the correspondence rather than
searched for separately.

Four rules keep coincidence out, and every one of them exists because a
measurement forced it rather than because it seemed wise:

* **Both sides are sampled at the same rate in time**, not at the same frame
  count. Sampling a long original coarsely and a short copy finely puts the
  copy's instants between the original's, and for a moving camera the picture
  in between is a different picture. A twenty-second extract matched 41 frames
  of 300 before this, and all of them onto a single original frame.
* **Offsets are counted over a sliding window**, not grouped into bins. A bin
  boundary the true offset happens to fall on splits its votes in half, and a
  wrong offset in the middle of its own bin outranks each half. Measured on an
  extract whose true offset was +40.00 s and which came out at +35.02 s.
* **Candidates are ranked by their best contiguous run**, not by total support.
  A true offset is confined to one stretch of the copy; a wrong one drawn from
  a self-similar recording collects a little support everywhere. On a
  three-clip montage the wrong offset pooled 165 pairs against the true
  offset's 48 and won on volume alone.
* **A run must be dense and close.** Its agreeing frames must be most of the
  frames in its own span, and its mean distance must be far below the proposal
  threshold — over twenty pairings of unrelated videos, density alone let a
  false segment through on one of them.

The copy's timeline is a partition: two segments never own the same instant,
and each copy frame goes to the offset that fits it best rather than to
whichever offset was processed first.

Every one of those numbers was checked against the burn-in, which carries a
monotonic frame counter and settles disagreements the code cannot settle with
itself.

---

## Usage

```
edit-report --bundle evidence.zip --html report.html --json report.json
```

| Option | |
|---|---|
| `--bundle <PATH>` | The evidence bundle `.zip`, or a directory it was unpacked into. Required. |
| `--html <PATH>` | Write the standalone HTML report. |
| `--json <PATH>` | Write the machine-readable report. |
| `--qr-frames <N>` | Frames to sweep for burned-in codes (default 24, spread across the whole recording). |
| `--python <PATH>` | Interpreter used to run the bundle's `verify_bundle.py`. |
| `--skip-crypto` | Do not run the verifier. The report then states the chain was not established here. |

With neither `--html` nor `--json`, a summary is printed and no file is written.

The exit code says whether the **run** could do its job — never what it thinks
of the recording. A bundle whose chain failed to verify is a successful run that
reports a failed chain. The report is the output; the exit code is not.

### Three transmission modes, and only one of them withholds the picture

| `transmission` | scheme | can this tool read the picture? |
|---|---|---|
| `clear` | `none` | yes — never sealed |
| `transport` | `box-seal-x25519` | **yes** — the phone sealed each payload to the platform's key so the recording was never at rest in cleartext on the device; the server opened it on arrival, and the bundle holds the media |
| `e2e` | `aes-gcm-256` | no — the DEK is sealed to users, no server holds a key |

Testing the scheme against `"none"` is the wrong test and produces the worst
kind of wrong answer: it tells the holder of a perfectly readable bundle that
their evidence is locked. Read the manifest's `transmission` field.

For an end-to-end encrypted bundle, this tool holds no keys and implements no
decryption. Decrypt with the bundle's own verifier first, then point this tool
at the result:

```
python3 verify_bundle.py . --extract --id-priv
edit-report --bundle .
```

### Requirements

* **Python 3** to run the bundle's verifier. Without it, the report states the
  chain was not established here — a third state, not a failure.
* **ffmpeg and ffprobe** for decoding. See *Decoder* below.
* A bundle generated on or after 2026-09-10, whose `verify_bundle.py` supports
  `--json`. An older bundle is reported as a chain this tool could not
  establish; run its verifier yourself, or fetch the bundle again.

---

## Architecture

```
core/   the library. No I/O, no video decoder, no network. Takes decoded
        frames and metadata; returns a correspondence table, cuts and
        classified differences. Compiles to wasm32.
cli/    native binary. Decodes with ffmpeg, runs verify_bundle.py.
wasm/   browser binding. A flat C interface, no wasm-bindgen.
web/    the page. Demuxes and decodes with WebCodecs.
```

**The decoder is outside the core, and that is the structural decision.** It buys
three things: the browser build uses the browser's own WebCodecs, so the wasm
module stays a few hundred kilobytes instead of the 25–30 MB an embedded
ffmpeg would cost — which is what makes a single-file, saveable page possible at
all; the native build can use whatever decoder handles the codec in front of it;
and a core that takes frames rather than files is testable with synthetic
fixtures that decode nothing.

**The core works without audio.** WebCodecs audio support is uneven — Chrome and
Edge since 94, Firefox desktop since 130, Safari complete only from 26.0 with a
partial window between 16.4 and 18.7, Firefox for Android not at all. Audio
alignment is therefore an optimisation that is used when available, never a
prerequisite.

### The browser build

```
./web/build.sh                       # cargo build --target wasm32-unknown-unknown
python3 -m http.server --directory web 8080
```

That is the whole toolchain. `wasm/` exports a flat C interface — allocate,
push a frame, ask for the report — and the JavaScript that drives it is written
by hand, so **there is no wasm-bindgen and no generated code**. In a tool meant
to be audited, glue a reader has to trust without having written it is a cost,
and the surface here is small enough not to pay it. The module is under 200 kB.

`web/mp4.js` is a small MP4 demuxer, present because WebCodecs deliberately has
none: `VideoDecoder` takes encoded chunks and getting them out of a container
is the caller's problem. The alternative — playing the file in a `<video>`
element and catching frames as they are presented — runs in real time and drops
frames whenever the tab is busy, and this tool reads **every** frame. Both
container shapes are handled: fragmented (`moof`/`trun`), which is what a
Forsheur phone writes, and progressive (`moov`/`stbl`), which is what a
platform re-encode produces.

**The browser does not check the cryptographic chain, and the report says so.**
That check belongs to the bundle's own `verify_bundle.py`, which this project
never re-implements — one implementation of the thing that must be right, not
two — and there is no Python in a browser. Run it yourself and read its verdict
beside the report.

### Decoder, and its licence

The native binary shells out to **ffmpeg**. The original from a bundle is always
H.264 in fragmented MP4, so a single-codec decoder would serve it — but the copy
is whatever a platform re-encoded it into (H.264, HEVC, VP9, AV1), and a tool
that cannot open the copy cannot do its job.

Shelling out rather than linking means **no ffmpeg code is distributed with this
tool**, so no LGPL or GPL obligation attaches to these binaries. ffmpeg is a
runtime requirement, named in this README and reported in every report's
`tool.decoder` field, so a reader always knows which decoder produced the pixels
a finding rests on. The browser build ships no decoder at all.

---

## Building

```
cargo build --release
cargo test
```

Dependencies are pinned to exact versions and `Cargo.lock` is committed,
binaries included: a verification tool whose build is not reproducible cannot
ask anyone to trust its output.

The compiler version is pinned too, in `rust-toolchain.toml`. `stable` means
"whatever was stable that day", and a different rustc produces different bytes
— which would make the SHA-256 published beside each release a number nobody
else could recalculate. An unverifiable hash is worse than none, because it
looks like a verification.

### Checking a release

Every tagged build is produced by CI, never on a developer machine, and
published as a permanent GitHub Release with `SHA256SUMS` covering every file.

    # Where did this file come from?
    gh attestation verify edit-report.html --repo forsheur/edit-report

    # Does it match the source?
    git checkout v0.1.0
    ./web/build-reproducible.sh              # needs docker
    # prints the SHA-256 of edit-report.html; it must equal the published one

The second command builds in a container rather than on your machine, and
the difference matters. Four things have to be fixed before two people get
the same bytes, and only three of them live in this repository: the compiler
version (`rust-toolchain.toml`), the dependency set (`Cargo.lock`, with
`--locked`), and a release profile of one codegen unit (workspace
`Cargo.toml` — with the default sixteen, LLVM leaves a path-derived hash in a
symbol suffix, which was the last thing differing between two checkouts of
the same commit). Paths are remapped out of the artefact on top of that.

The fourth is the machine, and it cannot be written down — it has to be
supplied. Cargo derives symbol suffixes from the full `rustc -vV` output,
which names the host triple, so a macOS host and a Linux host disagree even
when both target wasm32; and a native executable is linked by the system's
own linker, so Debian and Ubuntu disagree too. Measured against the v0.1.0
release:

| Built in | Artefact | Matches the release |
|---|---|---|
| `rust:1.93.0-bookworm` | `edit-report.html` | yes |
| `ubuntu:24.04` + rustup | `edit-report-linux-x86_64` | yes |
| macOS, natively | `edit-report.html` | **no** — different host |

So the page, which is the thing most readers actually run, is reproducible by
anyone with docker. The macOS and Windows executables would need the runner's
own OS and Xcode/MSVC; for those the provenance attestation is what stands,
and that is a weaker statement — it says who built the file, not that the file
follows from the source.

The CI builds the page twice from two directories and fails if they differ,
so the part that *is* under our control cannot silently regress.

The attestation is a signed statement, in a public transparency log, that this
file was produced by this workflow from this commit. It says where the file
came from. The rebuild says the file matches the source. **Neither says the
source is honest** — that part is on the reader, which is why the report names
frame numbers and timestamps and puts both videos on screen: a claim you can
check with your own eyes needs no chain of hashes at all.

Development happens on macOS Apple Silicon. Windows and Linux binaries are built
in CI, never on a developer machine — see `.github/workflows/`.

### macOS

The released executables are **not signed or notarised**, so Gatekeeper stops
the first run: *"cannot be opened because the developer cannot be verified"*.
Signing them would mean paying Apple for the right to say who built a file,
which the provenance attestation already says, in public, for free. So:

    chmod +x edit-report-macos-aarch64        # or -x86_64 on an Intel Mac
    xattr -d com.apple.quarantine edit-report-macos-aarch64

Or open it once from the Finder with *right-click → Open*, which offers the
same choice through a dialogue. Either way, check the published SHA-256 first —
that is the check that means something, and it does not depend on Apple.

`edit-report.html` needs none of this: a browser opens a local file without
asking anyone's permission.

### Windows

The released executable is **not code-signed**, so SmartScreen will warn the
first time you run it. That warning means the file came from the internet, not
that anything is wrong with it; choose *More info* then *Run anyway*, or check
the published SHA-256 against your download first.

Console output is forced to UTF-8, and redirecting to a file works. Paths with
spaces, accented characters, beyond 260 characters, and UNC network paths are
handled — arguments are kept as `OsString` and never lossily converted.

---

## Licence

Apache-2.0. See [LICENSE](LICENSE) and the reasoning in
[SECURITY.md](SECURITY.md#why-a-permissive-licence).

## No telemetry

There is no network client in this binary. It makes no request of any kind, to
anyone, ever. That is structural rather than a promise: there is nothing in the
dependency tree that could.

This tool is part of the Forsheur project ([forsheur.com](https://forsheur.com))
and needs nothing from it. A bundle is all it reads, from your own disk; it
never asks a server whether that bundle is good, because a verification that
depends on the party being verified is not one.
