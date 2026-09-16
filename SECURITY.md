# Threat model

Written for people who will try to break this, because those are the people
whose opinion decides whether it is worth anything. The section on **how you
would fool it** is the one to read first; it is deliberately specific.

---

## What this tool protects against

**A copy passed off as the original.** Given a sealed bundle and a video, it
describes where the two correspond and where they do not. Cuts, inserted
material, and regions of the picture that differ from the original become
visible and locatable in time.

**A claim of provenance with nothing behind it.** The bundle's own verifier
establishes that the sealed bytes carry a device signature, a notary seal, and
an anchor. If it does not verify, no comparison is performed at all.

**A verification you have to take on faith.** Everything runs on your machine.
The binary makes no network request; the browser build uploads nothing. The
report quotes the verifier verbatim rather than summarising it, and names the
SHA-256 of the copy that ran.

---

## What it does not protect against

**Anything about the truth of the scene.** A staged event filmed on a genuine
phone, sealed and anchored, passes every check in this tool and in Forsheur.
This is the largest limit and it is not a defect: provenance and truth are
different questions, and no measurement of pixels answers the second.

**A recording that is authentic and misleading.** Framing, timing, what is out
of shot, what the camera was turned away from — none of it is measurable here.

**The identity of the filmer.** The chain ends at an account's public key. That
key is a pseudonym: it links a recording to the same signer as their other
recordings, and to nothing else.

**Whether a bundle you were given is the bundle it claims to be.** This tool
reads whatever archive you point it at. How you obtained it, and whether the
party who gave it to you is the party you think, is outside its scope.

---

## How you would fool it

### Fool the alignment by forging the burn-in

The strongest signal in a Forsheur frame is the burned-in overlay: a UTC
timestamp and a **monotonic frame counter**, in a fixed font at a fixed
position. Read it and alignment stops being a correlation problem.

Those pixels are not signed. Render `2026-09-03T10:44:05Z  f=91` into your own
footage in the same font at the same place and the burn-in reader will read it.

**Why that is not enough, and what it costs us.** The burn-in only ever
*proposes* an alignment; the report is built from an actual comparison of
pictures at the proposed offset, which forged text does not survive. The real
cost is different: a convincing forgery makes the sweep spend its effort on a
wrong hypothesis, and on a video with no genuine correspondence the result is a
report that establishes none — which is the correct outcome, reached slowly.

### Fool the burn-in reader into silence

Crop the top of the frame. Overlay a subtitle bar. Re-encode hard enough that
the code stops decoding. All three are ordinary things that happen to a video
on a platform, all three remove the burn-in, and **none of them is reported as
anything but what it is**: no code was read.

This is a deliberate asymmetry. Removing the signal is easy and is not
evidence of anything. That is why the second alignment path — audio correlation
plus perceptual hashing — is a first-class path and not a fallback: an attacker
who removes the burn-in has removed a convenience, not the method.

### Hide a change inside recompression

The distinction between `consistent-with-recompression` and
`localized-difference` is structural: degradation from a codec is global,
spatially unstructured, and gets worse as bitrate drops; a substituted region is
concentrated, persistent across consecutive frames, and sits in a frame that
otherwise aligns well.

**So make your change look like a codec.** Change a small region, then re-encode
the whole video at a low enough bitrate that the difference introduced sits
under the difference the codec introduces everywhere. The right answer then is
`inconclusive`, and the tool is built to give it — which is precisely why there
is a third state and why it must stay common. A two-state classifier under this
attack would have to guess, and half its guesses would be accusations.

**Or change one frame.** A single substituted frame in an otherwise identical
copy is the sharpest version of this attack. It is a fixture in the test set,
and it is the case that taught us the most.

**A frame changed evenly is invisible to every per-frame check.** Measured on a
real recording with one frame blurred: it lost 85 % of its texture, and the
report called it conforming alongside 561 others. The 63-bit fingerprint
thresholds DCT coefficients at their median, so a uniform blur scales them all
down while preserving their order — barely a bit flips. And the localised
comparison correctly answered the question it was asked: the difference is
spread evenly, which is what a recompression looks like.

What none of them asked is whether that much difference is normal for this
film. Its neighbours differed from the original by 3.6 and 6.0 grey levels; it
differed by 19.7. Section 7 of the report asks that question now, per shot and
against the shot's own spread: the frame came back at 26.7 deviations where the
bar is 8, and nothing fired on the faithful copy, the cut, the retouch, or the
150 kbit/s and half-width recompressions.

**Two things it still cannot see, and both are ways to hide.** A change applied
to the whole film: blur every frame and every frame's neighbours are blurred
too, so nothing stands out. And a shot with too little texture to measure — on
a static indoor recording averaging 2.5 grey levels of gradient, the same
blurred frame is invisible, because tiles that flat are set aside before the
comparison runs and the change is set aside with them. Film something with
detail in it and the check works; film a wall and it does not.

### Film something that looks the same for a long time

This is the sharpest limit in the tool, it needs no attacker at all, and it was
measured rather than reasoned about.

Alignment works by finding a **constant time offset** that many frames agree on.
That only identifies a position in the original if the original's moments are
distinguishable from each other. On a static shot — a camera on a tripod
pointing at a doorway, a wide view of a landscape — the frame at 10 s and the
frame at 50 s *are* the same picture, and no perceptual measure invented or
inventable separates them.

On the fixture recording, a near-static shot of a roofline, **36 % of frame
pairs more than twenty seconds apart fell inside the matching threshold and
18 % inside the stricter gate a segment must pass.** A correspondence on such a
recording establishes that the copy comes from it far more firmly than it
establishes *where* in it, and a segment can settle on a displaced offset while
agreeing at every frame.

The tool measures this on the original at run time and prints the number and
the caveat in the report. It does not silently compensate: adjusting thresholds
to hide the ambiguity would be the tool making a judgement it cannot support,
and the reader is better served by being told to discount the timestamps.

**How this is exploited on purpose.** Cut material out of a static passage. The
correspondence closes over the excision because the frames either side of it
look the same as the frames that were removed, no offset changes, and no cut is
reported. There is no defence against this in image comparison; what defends
against it is the original's own chain, which commits to every sealed segment
and its capture time.

### Make the two videos incomparable

Any of: re-encode at a very low bitrate; change the frame rate; crop to a
different aspect; mirror horizontally; add a border. Enough of them together and
no correspondence is established.

The report then says no correspondence was established, and **that is not a
finding against the video**. An attacker who reaches this state has produced an
inconclusive report, not a favourable one — but they have also denied the
journalist an answer, which for some purposes is the goal.

### Attack the reader instead of the tool

A QR payload is text from a stranger's video, and it lands in an HTML file a
journalist opens. It is escaped, and there is a test for that. An evidence
bundle is an archive from a source who may not be a friend; entry paths are
checked and traversal outside the extraction directory is refused.

### Substitute the verifier

This tool runs `verify_bundle.py` **from inside the bundle being examined**. An
attacker who controls the bundle controls that script, and could ship one that
prints `VERDICT: PASS` over anything.

**This is a real limitation and it is not fully solved.** What exists today: the
report names the SHA-256 of the verifier that ran, so two bundles claiming the
same session can be compared and a substituted verifier shows up as a different
hash. What that does not do is tell you which hash is right. If a bundle matters,
diff its `verify_bundle.py` against the copy published by the Forsheur server, or
run a verifier you obtained separately against the bundle's data.

We chose this over the alternative — pinning one verifier inside this tool —
because a pinned copy stops understanding bundle formats that are newer than
this binary, and a verification tool that silently stops verifying is worse than
one whose trust boundary is written down.

---

## Metadata this tool leaks

**None to any network.** There is no HTTP client in this binary.

It does write to the filesystem: it extracts a `.zip` bundle to a temporary
directory, and asks the bundle's verifier to write `extracted/` **inside the
bundle directory** when that directory is not a temporary one. On a shared or
backed-up machine, decoded media therefore lands on disk where a backup agent or
another user may reach it.

The earlier design of this tool fetched the original over the network from a QR
code read in the video. That fetch named the recording to whoever operates the
server and to anyone watching the connection — a journalist checking a leaked
video would have announced which video they were checking, and when. Taking the
bundle as the only input removes that leak entirely. **How you obtained the
bundle is where the exposure now lives, and it is outside this tool.**

---

## Reproducibility

A published hash is only worth something if someone else can arrive at it. So:
dependencies are pinned to exact versions and `Cargo.lock` is committed for
binaries as well as libraries; the compiler version is pinned in
`rust-toolchain.toml`; the release profile uses one codegen unit, because with
more LLVM suffixes promoted symbols with a hash derived from the object file's
path, and that suffix was — measured — the only difference between two builds
of the same commit from two directories; and paths are remapped out of the
binary. Release builds happen in CI from a tagged commit, with a signed
provenance attestation and `SHA256SUMS`. The CI also builds the browser page
twice from two directories and fails if they differ, so a regression here
cannot pass unnoticed.

A build on macOS and one on the Linux runner do **not** give the same bytes,
and that was measured rather than assumed. Two causes, neither of them in
this code: Cargo derives symbol suffixes from the whole `rustc -vV` string,
which names the host triple, so the host leaks into a wasm32 artefact that
otherwise has nothing host-specific in it; and a native executable is linked
by the system linker, so the distribution matters. The answer is that the
reproducible build is specified as an environment, not only as a source:
`web/build-reproducible.sh` runs it in a pinned container, and reproduces the
published `edit-report.html` and Linux executable exactly. The macOS
executables are not reproducible outside the runner's own OS; for them the
provenance attestation is the weaker thing that remains, and the README says
so rather than implying otherwise.

The Windows executable is worse, and stating it plainly matters more than
looking tidy: it is not reproducible **even on the runner**. Tags `v0.1.0` and
`v0.1.1` differ in documentation only, were built by the same workflow on the
same image, and produced byte-identical Linux and macOS binaries — while the
Windows one changed. MSVC's linker embeds something per-run that no flag of
ours controls. The consequence for a reader: two different digests for two
Windows builds prove nothing, in either direction. The check that still works
is the digest of the exact file GitHub published, in `SHA256SUMS`, plus the
attestation that names the workflow and commit it came from.

---

## Why a permissive licence

**Apache-2.0**, and the argument is about adoption rather than principle.

The point of a verifier is that people who do not trust you can run it, fork it,
vendor it into their own newsroom tooling, and ship it inside products whose
licensing you do not control. Copyleft asks each of those people a question
before they can do it, and for a tool whose value is being widely re-run, every
such question is a reader lost.

Apache-2.0 over MIT for the patent grant: this tool touches perceptual hashing
and video comparison, both areas with patent activity, and an explicit grant
plus its termination clause protects downstream users in a way MIT does not.

The case against, stated fairly: a permissive licence lets someone build a
closed product on this and publish conclusions it cannot support. That risk is
real, and it is not addressed by the licence — it is addressed by the report
format itself, which contains no score to lift out of context, and by the fact
that the wording rules are enforced by tests that a fork must delete on purpose
rather than drift past by accident.

---

## Reporting a problem

Open an issue for anything that is not itself a vulnerability. For a
vulnerability in this tool, or a way to make it produce a report that misleads,
contact the maintainers privately first: **security@forsheur.com**, or the
repository's private vulnerability reporting on GitHub.

A report that says "this tool called a legitimate video falsified" is treated as
a security issue, not a cosmetic one. That is the failure mode this whole design
exists to prevent.
