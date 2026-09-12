//! Temporal alignment: which parts of the copy come from which parts of the
//! original.
//!
//! ## The formulation
//!
//! A cut-free stretch of copy taken from the original has one property that
//! survives everything a platform does to it: **a constant time offset**. Frame
//! at 12.0 s in the copy is frame at 47.0 s in the original; 12.5 s is 47.5 s;
//! and so on, for as long as the stretch lasts. Re-encoding does not change it,
//! rescaling does not change it, a frame-rate conversion does not change it —
//! only a cut does, and that is the point.
//!
//! So the work is: propose (copy frame, original frame) pairs by perceptual
//! distance, read the time offset off each pair, and look for offsets that many
//! pairs agree on. Each agreed offset is a segment. The boundaries between
//! segments are the cuts. Copy time no offset explains is copy time with no
//! correspondence established.
//!
//! This is preferred over a generic DTW for two reasons. It tolerates holes by
//! construction — a stretch of copy nobody can match simply contributes no
//! pairs, rather than dragging a warping path through it. And it is arguable:
//! a reader who disputes a segment can be shown the offset, how many frames
//! agreed on it, and how close they were. A warping path is a much harder thing
//! to put in front of someone.
//!
//! ## What a match is not
//!
//! A pair of frames within the distance threshold is a **hypothesis**, never a
//! finding. Two frames of a grey sky are within it, and so are two frames of
//! the same wall filmed on different days. Nothing here concludes from a single
//! pair; a segment needs many pairs agreeing on one offset, and the report says
//! how many.
//!
//! Symmetrically, a stretch with no correspondence is reported as exactly that.
//! It is not evidence that the copy was altered — it is what a crop, a heavy
//! re-encode, an unrelated video, or a dark passage all produce.

use crate::fingerprint::Fingerprint;

/// One frame of one video, reduced to what alignment needs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sample {
    /// Position in the decoded sequence.
    pub index: u64,
    /// Time from the start of the video, microseconds.
    pub t_us: i64,
    pub fp: Fingerprint,
}

/// Hamming distance at or below which two frames are proposed as a pair.
///
/// **Measured, not chosen.** Over the fixture corpus — one recording
/// re-encoded from 4000 kbit/s down to 150 kbit/s, cropped, letterboxed and
/// subtitled — corresponding frames sit below this and non-corresponding
/// frames sit well above it. The number is deliberately loose: a threshold
/// tight enough to be selective on its own would drop true pairs on the heavy
/// re-encode, and selectivity is not this step's job. Agreement on an offset
/// is what makes a segment, and a loose threshold that produces some wrong
/// pairs costs nothing because wrong pairs do not agree with each other.
pub const MAX_DISTANCE: u32 = 22;

/// Half-width of the window an offset is counted over.
///
/// **A window, not a bucket, and the difference mattered.** Grouping offsets
/// into fixed bins puts a boundary somewhere, and when the true offset falls on
/// one its votes split between two bins — each half can then be outranked by a
/// wrong offset that happened to land in the middle of its own bin. Measured on
/// a twenty-second extract whose true offset was +40.00 s: the tool reported
/// +35.02 s, and the burn-in on both videos (`f=1200` in the copy's first frame
/// and in the original at 40.00 s) showed exactly what the right answer was.
///
/// Counting every offset within ±this of each candidate has no boundaries to
/// fall on.
///
/// Wide enough to absorb the difference between a 30 fps original and a 25 fps
/// copy, plus the sampling step on each side; narrow enough that two different
/// segments of the same recording stay apart.
pub const OFFSET_WINDOW_US: i64 = 250_000;

/// A run needs at least this many agreeing frames to be reported as a segment.
///
/// Three is not a lot, and it is meant to be reachable: a two-second clip
/// sampled sparsely should still be describable. What keeps it honest is that
/// the count travels into the report, so a segment resting on three frames is
/// visibly a segment resting on three frames.
pub const MIN_SEGMENT_FRAMES: usize = 3;

/// A gap longer than this inside one offset ends the run.
pub const MAX_RUN_GAP_US: i64 = 2_000_000;

/// How many offset peaks are examined in detail.
///
/// The windowed score is a cheap pre-filter; the run-based score that decides
/// the ranking costs a pass over the pairs, so it cannot be spent on every
/// distinct offset. But the pre-filter is the WRONG ranking — that is the whole
/// reason the run-based one exists — so cutting it tight throws away the right
/// answer before anything good can look at it.
///
/// Measured: at 64, a recording with a modest haze of near-matches filled every
/// slot with haze peaks and the true offsets never entered the candidate set at
/// all. A twenty-second extract that matches perfectly reported nothing. The
/// cap is now loose enough that the pre-filter only excludes offsets no
/// plausible segment could rest on, and the ranking that matters decides the
/// rest.
pub const MAX_CANDIDATE_OFFSETS: usize = 512;

/// Mean distance a run must achieve before it is reported as a segment.
///
/// **Two thresholds, doing two different jobs.** [`MAX_DISTANCE`] decides which
/// pairs are worth *proposing* and is deliberately loose, because a heavily
/// re-encoded frame is still the same picture and a tight proposal threshold
/// would drop it. This one decides which runs are worth *reporting*, and it is
/// tight, because a run whose frames merely scrape inside the proposal
/// threshold is what coincidence looks like.
///
/// The measurement that forced it: over twenty pairings of two unrelated
/// videos, density alone let a false segment through on one of them. A tool
/// that occasionally announces "this copy contains material from the original"
/// about an unrelated recording has produced the single most damaging output it
/// is capable of, and one pairing in twenty is not rare.
///
/// Unrelated 64-bit fingerprints differ in about 31 bits with a spread near 4,
/// so a mean under 14 is roughly four standard deviations out — reachable by
/// chance a handful of times in a million pairs, not a handful of times in a
/// thousand.
pub const MAX_SEGMENT_MEAN_DISTANCE: f32 = 14.0;

/// Fraction of the copy frames inside a segment's own span that must be the
/// ones agreeing on its offset.
///
/// Without this, three coincidences scattered across twenty seconds are a
/// "segment". They will happen: the distance threshold is deliberately loose,
/// so a few unrelated frames land inside it, and a few of those will line up on
/// some offset by chance. What separates a real correspondence from that is not
/// how close the matches are but how CONTINUOUS they are — a stretch of copy
/// taken from the original agrees at essentially every frame of its span, and
/// coincidences are sparse by construction.
///
/// Not 1.0: a real segment loses frames to blank passages and to heavy
/// compression, and demanding perfection would report a cut wherever the sky
/// filled the frame.
pub const MIN_RUN_DENSITY: f32 = 0.6;

/// How much a recording looks like itself at other moments.
///
/// **The limit that decides what any of this is worth.** A camera left pointing
/// at a doorway produces a video in which the frame at 10 s and the frame at
/// 50 s are, to any perceptual measure, the same picture — because they are.
/// No fingerprint can separate them, and an alignment on such a recording can
/// settle on the wrong offset while agreeing at every frame.
///
/// This is measured on the original rather than assumed away, and it travels
/// into the report. Compensating for it quietly would mean the tool making a
/// judgement it cannot support; saying it lets the reader discount the
/// timestamps by the right amount.
///
/// Measured on the fixture recording — a near-static shot of a roofline — 36 %
/// of frame pairs more than 20 seconds apart fell inside the proposal
/// threshold and 18 % inside the segment gate. On such a recording the
/// correspondence establishes that the copy comes from the original far more
/// firmly than it establishes WHERE in the original.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SelfSimilarity {
    /// Pairs compared, all at least [`SELF_SIMILARITY_MIN_GAP_US`] apart.
    pub pairs_examined: usize,
    /// How many of them are close enough to be proposed as a match.
    pub pairs_within_threshold: usize,
    /// How many are close enough to pass the segment gate.
    pub pairs_within_segment_gate: usize,
}

/// Frames must be at least this far apart to count towards self-similarity.
/// Neighbouring frames of any video resemble each other; that is not the
/// problem being measured.
pub const SELF_SIMILARITY_MIN_GAP_US: i64 = 20_000_000;

/// Above this fraction, the offsets a correspondence reports are weakly
/// determined and the report says so.
pub const SELF_SIMILARITY_AMBIGUOUS: f32 = 0.05;

impl SelfSimilarity {
    pub fn fraction_within_gate(&self) -> f32 {
        if self.pairs_examined == 0 {
            return 0.0;
        }
        self.pairs_within_segment_gate as f32 / self.pairs_examined as f32
    }

    /// Whether a reader should treat the reported positions as approximate.
    pub fn offsets_weakly_determined(&self) -> bool {
        self.fraction_within_gate() > SELF_SIMILARITY_AMBIGUOUS
    }

    /// One sentence for the report, or `None` when there is nothing to warn of.
    pub fn caveat(&self) -> Option<String> {
        if !self.offsets_weakly_determined() {
            return None;
        }
        Some(format!(
            "This recording looks like itself at other moments: of {} pairs of its own frames more than {} seconds apart, {} ({:.0}%) are as close as a genuine match would be. A static shot does that, and nothing can separate two frames that are the same picture. Read the correspondence below as establishing that the copy comes from this recording much more firmly than establishing WHERE in it — the segment boundaries and offsets may be displaced to another moment that looks the same.",
            self.pairs_examined,
            SELF_SIMILARITY_MIN_GAP_US / 1_000_000,
            self.pairs_within_segment_gate,
            100.0 * self.fraction_within_gate(),
        ))
    }
}

/// Measure how much a recording resembles itself elsewhere.
///
/// Every pair at least [`SELF_SIMILARITY_MIN_GAP_US`] apart, capped so the cost
/// stays bounded on a long video.
pub fn self_similarity(samples: &[Sample]) -> SelfSimilarity {
    // Cap the number of anchors, not the number of samples: the measure is
    // about the spread of the whole video, so thinning must keep both ends.
    let stride = (samples.len() / 64).max(1);
    let anchors: Vec<&Sample> = samples
        .iter()
        .step_by(stride)
        .filter(|s| !s.fp.is_low_variance())
        .collect();

    let (mut examined, mut prop, mut gate) = (0usize, 0usize, 0usize);
    for (i, a) in anchors.iter().enumerate() {
        for b in anchors.iter().skip(i + 1) {
            if (b.t_us - a.t_us).abs() < SELF_SIMILARITY_MIN_GAP_US {
                continue;
            }
            examined += 1;
            let d = a.fp.distance(b.fp);
            if d <= MAX_DISTANCE {
                prop += 1;
            }
            if (d as f32) <= MAX_SEGMENT_MEAN_DISTANCE {
                gate += 1;
            }
        }
    }
    SelfSimilarity {
        pairs_examined: examined,
        pairs_within_threshold: prop,
        pairs_within_segment_gate: gate,
    }
}

/// A stretch of the copy that corresponds to a stretch of the original.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub copy_start_us: i64,
    pub copy_end_us: i64,
    pub original_start_us: i64,
    pub original_end_us: i64,
    /// `original_time - copy_time` for this stretch.
    pub offset_us: i64,
    /// How many sampled frames agreed on this offset. The weight behind the
    /// claim, printed wherever the claim is.
    pub frames_agreeing: usize,
    /// Mean perceptual distance over those frames, out of 63 bits.
    pub mean_distance: f32,
}

impl Segment {
    pub fn copy_duration_us(&self) -> i64 {
        self.copy_end_us - self.copy_start_us
    }
}

/// A stretch of the copy no segment explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unmatched {
    pub copy_start_us: i64,
    pub copy_end_us: i64,
    /// Frames examined inside this stretch, so "nothing matched here" can be
    /// told apart from "nothing was looked at here".
    pub frames_examined: usize,
}

/// A discontinuity between two consecutive segments of the copy.
///
/// Derived from the correspondence, never searched for separately: a cut IS a
/// change of offset, and looking for cuts by any other means would be a second
/// opinion that could disagree with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cut {
    /// Where the discontinuity falls in the copy.
    pub copy_at_us: i64,
    /// Where the outgoing segment stopped reading the original.
    pub original_left_us: i64,
    /// Where the incoming segment resumed reading it.
    pub original_resumed_us: i64,
}

impl Cut {
    /// Original time skipped over. Negative when the copy goes back — a
    /// re-ordering rather than an excision, which is a different edit and is
    /// reported as one.
    pub fn original_skipped_us(&self) -> i64 {
        self.original_resumed_us - self.original_left_us
    }

    pub fn goes_backwards(&self) -> bool {
        self.original_skipped_us() < 0
    }
}

/// Everything alignment establishes. No score, and no summary number.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Correspondence {
    /// In copy order.
    pub segments: Vec<Segment>,
    pub unmatched: Vec<Unmatched>,
    pub cuts: Vec<Cut>,
    pub copy_frames_examined: usize,
    pub copy_frames_matched: usize,
    /// Frames skipped as anchors for having too little structure to mean
    /// anything. Reported because it changes how the rest should be read.
    pub copy_frames_low_variance: usize,
    pub threshold_used: u32,
    /// How much the ORIGINAL resembles itself at other moments. The single
    /// most important qualifier on everything above.
    pub original_self_similarity: SelfSimilarity,
}

impl Correspondence {
    /// True when nothing was established. Named for the absence, not for
    /// anything it might be taken to imply.
    pub fn nothing_established(&self) -> bool {
        self.segments.is_empty()
    }

    /// Total copy time covered by segments.
    pub fn matched_duration_us(&self) -> i64 {
        self.segments.iter().map(|s| s.copy_duration_us()).sum()
    }
}

/// Align a copy against an original.
///
/// Both slices must be in ascending time order; the caller decides which
/// frames to spend on.
pub fn align(copy: &[Sample], original: &[Sample]) -> Correspondence {
    align_with(copy, original, MAX_DISTANCE)
}

pub fn align_with(copy: &[Sample], original: &[Sample], threshold: u32) -> Correspondence {
    let low_variance = copy.iter().filter(|s| s.fp.is_low_variance()).count();
    let similarity = self_similarity(original);

    let empty = Correspondence {
        segments: Vec::new(),
        unmatched: whole_copy_unmatched(copy),
        cuts: Vec::new(),
        copy_frames_examined: copy.len(),
        copy_frames_matched: 0,
        copy_frames_low_variance: low_variance,
        threshold_used: threshold,
        original_self_similarity: similarity,
    };
    if copy.is_empty() || original.is_empty() {
        return empty;
    }

    // ── 1. Propose pairs ───────────────────────────────────────────────────
    //
    // For each copy frame, every original frame within the threshold. Not just
    // the nearest one: a recording that revisits the same view produces several
    // near-equal candidates, and keeping only the closest would pick between
    // them on rounding noise. Agreement on an offset decides instead.
    //
    // Frames with too little structure propose nothing. A grey sky is within
    // the threshold of every other grey sky, and letting it vote would build
    // segments out of blankness.
    let mut pairs: Vec<(usize, usize, u32)> = Vec::new();
    for (ci, c) in copy.iter().enumerate() {
        if c.fp.is_low_variance() {
            continue;
        }
        for (oi, o) in original.iter().enumerate() {
            if o.fp.is_low_variance() {
                continue;
            }
            let d = c.fp.distance(o.fp);
            if d <= threshold {
                pairs.push((ci, oi, d));
            }
        }
    }
    if pairs.is_empty() {
        return empty;
    }

    // ── 2. Find the offsets many pairs agree on ────────────────────────────
    //
    // Each candidate offset is scored by the pairs within ±OFFSET_WINDOW_US of
    // it, and a pair counts for more the closer the two frames are. Both parts
    // matter: agreement alone lets a broad haze of loose pairs outrank a tight
    // cluster, and closeness alone lets a single excellent coincidence outrank
    // a whole segment.
    let offsets: Vec<i64> = pairs
        .iter()
        .map(|&(ci, oi, _)| original[oi].t_us - copy[ci].t_us)
        .collect();
    let mut order: Vec<usize> = (0..pairs.len()).collect();
    order.sort_by_key(|&n| (offsets[n], pairs[n].0, pairs[n].1));

    let weight = |d: u32| -> i64 { (threshold as i64 - d as i64 + 1).max(1) };
    let sorted_offsets: Vec<i64> = order.iter().map(|&n| offsets[n]).collect();

    // Score every candidate, then take the peaks. Ties are broken by offset so
    // two runs on identical input produce identical reports.
    let mut scored: Vec<(i64, i64)> = Vec::with_capacity(order.len());
    let (mut lo, mut hi) = (0usize, 0usize);
    let mut running: i64 = 0;
    for (k, &centre) in sorted_offsets.iter().enumerate() {
        while hi < order.len() && sorted_offsets[hi] <= centre + OFFSET_WINDOW_US {
            running += weight(pairs[order[hi]].2);
            hi += 1;
        }
        while lo < order.len() && sorted_offsets[lo] < centre - OFFSET_WINDOW_US {
            running -= weight(pairs[order[lo]].2);
            lo += 1;
        }
        // One candidate per distinct offset value; duplicates score the same.
        if k == 0 || sorted_offsets[k] != sorted_offsets[k - 1] {
            scored.push((centre, running));
        }
    }
    let mut ranked: Vec<(i64, i64)> = scored;
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    // A debug aid, off unless asked for. Offset selection is where this
    // algorithm decides everything, and when it is wrong the report looks
    // plausible; being able to see the peaks it weighed is the difference
    // between diagnosing that and guessing at it.
    if std::env::var_os("EDIT_REPORT_DEBUG_OFFSETS").is_some() {
        eprintln!(
            "  [debug] {} pairs proposed; top offsets by windowed score:",
            pairs.len()
        );
        for (off, score) in ranked.iter().take(6) {
            let n = offsets
                .iter()
                .filter(|o| (**o - off).abs() <= OFFSET_WINDOW_US)
                .count();
            eprintln!(
                "    offset {:+8.2}s  windowed {score:7}  pairs {n}",
                *off as f64 / 1e6
            );
        }
    }

    // Candidates whose windows overlap add nothing: they are the same peak seen
    // from one pair over, and they share their pairs. Separation is the FULL
    // window width, not half of it — at half, two centres 0.27 s apart both
    // survived, split a clip's frames between them, and the report showed a
    // three-frame segment sitting inside a thirty-four-frame one at
    // effectively the same offset.
    let mut centres: Vec<i64> = Vec::new();
    for (centre, _) in ranked {
        if centres
            .iter()
            .all(|c: &i64| (c - centre).abs() > 2 * OFFSET_WINDOW_US)
        {
            centres.push(centre);
        }
        if centres.len() >= MAX_CANDIDATE_OFFSETS {
            break;
        }
    }

    // Snap each candidate onto the actual peak: the median offset of the pairs
    // its window holds. Selection and collection must agree, and they did not.
    // Requiring a full window of separation while still collecting over half a
    // window let a centre sit up to 0.25 s away from the peak it was standing
    // for and then gather almost none of its own pairs — a twenty-second
    // extract that had matched 86 frames of 86 matched none at all. Snapping
    // makes the centre the peak, and makes two candidates on the same peak
    // collapse onto each other so the duplicate can be dropped.
    let snap = |centre: i64| -> i64 {
        let mut within: Vec<i64> = offsets
            .iter()
            .enumerate()
            .filter(|(_, o)| (**o - centre).abs() <= OFFSET_WINDOW_US)
            .map(|(_, o)| *o)
            .collect();
        if within.is_empty() {
            return centre;
        }
        within.sort_unstable();
        within[within.len() / 2]
    };
    let mut snapped: Vec<i64> = Vec::with_capacity(centres.len());
    for c in centres {
        let p = snap(c);
        if snapped
            .iter()
            .all(|q: &i64| (q - p).abs() > OFFSET_WINDOW_US)
        {
            snapped.push(p);
        }
    }
    let centres = snapped;

    // ── 2b. Re-rank the candidates by their best CONTIGUOUS run ────────────
    //
    // Total support is the wrong ranking and the fixture showed exactly how it
    // fails. A correct offset is confined to one stretch of the copy — a
    // twelve-second clip contributes at most forty-eight samples. A wrong
    // offset drawn from a self-similar recording draws a little support from
    // everywhere, and on a montage of three clips it pooled 165 pairs against
    // the true offset's 48 and won on volume alone, mapping the copy to a
    // passage the burn-in proves it did not come from.
    //
    // So rank by the best unbroken run each offset can produce, which is the
    // thing a segment actually is. Diffuse support scores nothing, however
    // much of it there is.
    let mut by_run: Vec<(i64, i64)> = centres
        .iter()
        .map(|&c| {
            (
                c,
                best_run_weight(c, &offsets, &pairs, copy, original, threshold),
            )
        })
        .collect();
    by_run.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if std::env::var_os("EDIT_REPORT_DEBUG_OFFSETS").is_some() {
        eprintln!("  [debug] top offsets by best contiguous run:");
        for (off, w) in by_run.iter().take(6) {
            eprintln!("    offset {:+8.2}s  run weight {w:7}", *off as f64 / 1e6);
        }
    }
    let centres: Vec<i64> = by_run.into_iter().map(|(c, _)| c).collect();

    // A copy frame belongs to at most one segment; `claimed` records that, and
    // feeds the matched count the report prints.
    let mut claimed = vec![false; copy.len()];
    let mut segments: Vec<Segment> = Vec::new();

    // ── 3. Assign each copy frame to the offset that fits it best ──────────
    //
    // Not first-come-first-served. Growing segments greedily, strongest offset
    // first, let a strong offset claim frames that belonged to a weaker one
    // simply by arriving earlier: on a three-clip montage whose three offsets
    // were each recovered correctly, the middle clip's offset swallowed half
    // the first clip, and the reported boundary was six seconds off.
    //
    // A copy frame belongs where it fits best. Candidate order still decides
    // ties, so a frame that matches two offsets equally well goes to the one
    // with the stronger run behind it.
    let mut assigned: Vec<Option<(usize, usize, u32)>> = vec![None; copy.len()];
    for (rank, &centre) in centres.iter().enumerate() {
        for (n, &off) in offsets.iter().enumerate() {
            if (off - centre).abs() > OFFSET_WINDOW_US {
                continue;
            }
            let (ci, oi, d) = pairs[n];
            match assigned[ci] {
                // Strictly better distance wins; an equal one does not, which
                // is what gives candidate order the tie-break.
                Some((cur_rank, _, cur_d)) if cur_d <= d && cur_rank != rank => continue,
                Some((cur_rank, _, cur_d)) if cur_rank == rank && cur_d <= d => continue,
                _ => assigned[ci] = Some((rank, oi, d)),
            }
        }
    }

    if std::env::var_os("EDIT_REPORT_DEBUG_ASSIGN").is_some() {
        let want: f64 = std::env::var("EDIT_REPORT_DEBUG_ASSIGN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1.0);
        for (ci, a) in assigned.iter().enumerate() {
            let t = copy[ci].t_us as f64 / 1e6;
            if want >= 0.0 && (t - want).abs() > 1.0 {
                continue;
            }
            let mut cands: Vec<(i64, u32)> = pairs
                .iter()
                .filter(|(c, _, _)| *c == ci)
                .map(|&(_, oi, d)| (original[oi].t_us - copy[ci].t_us, d))
                .collect();
            cands.sort_by_key(|&(_, d)| d);
            cands.truncate(4);
            eprintln!(
                "    [assign] copy {t:6.2}s  -> {:?}  best pairs {:?}",
                a.map(|(r, oi, d)| (
                    (centres[r] as f64 / 1e6 * 100.0).round() / 100.0,
                    original[oi].t_us as f64 / 1e6,
                    d
                )),
                cands
                    .iter()
                    .map(|(o, d)| ((*o as f64 / 1e6 * 100.0).round() / 100.0, *d))
                    .collect::<Vec<_>>()
            );
        }
        eprintln!(
            "    [assign] {} centres, first few: {:?}",
            centres.len(),
            centres
                .iter()
                .take(5)
                .map(|c| (*c as f64 / 1e6 * 100.0).round() / 100.0)
                .collect::<Vec<_>>()
        );
    }

    // Then each offset keeps only the frames that chose it, and those become
    // runs the same way as before.
    for (rank, _) in centres.iter().enumerate() {
        let entries: Vec<(usize, usize, u32)> = assigned
            .iter()
            .enumerate()
            .filter_map(|(ci, a)| match a {
                Some((r, oi, d)) if *r == rank => Some((ci, *oi, *d)),
                _ => None,
            })
            .collect();
        if entries.len() < MIN_SEGMENT_FRAMES {
            continue;
        }

        // Split into runs: consecutive in copy order, no long silence, and
        // moving forward through the original. A pair that would send the
        // original backwards inside one offset is noise, not a segment.
        let mut run: Vec<(usize, usize, u32)> = Vec::new();
        for e in entries {
            let breaks = run.last().is_some_and(|last| {
                copy[e.0].t_us - copy[last.0].t_us > MAX_RUN_GAP_US
                    || original[e.1].t_us < original[last.1].t_us
            });
            if breaks {
                debug_flush(rank, &centres, &run, copy, original, "break");
                flush_run(&mut run, copy, original, &mut claimed, &mut segments);
            }
            run.push(e);
        }
        debug_flush(rank, &centres, &run, copy, original, "end");
        flush_run(&mut run, copy, original, &mut claimed, &mut segments);
    }

    segments.sort_by_key(|s| s.copy_start_us);

    // The copy's timeline is a partition. Two segments cannot both own the same
    // instant, whatever route produced them, so a segment contained in one
    // already emitted is dropped rather than shown as a contradiction. Kept as
    // a guard even though centre separation should now prevent it: this is the
    // invariant the report rests on, and an invariant worth stating is worth
    // enforcing where it is read.
    let mut kept: Vec<Segment> = Vec::with_capacity(segments.len());
    for seg in segments.into_iter() {
        let overlaps = kept
            .iter()
            .any(|k| seg.copy_start_us <= k.copy_end_us && k.copy_start_us <= seg.copy_end_us);
        if overlaps {
            continue;
        }
        kept.push(seg);
    }
    let segments = kept;

    // ── 4. What is left over, and where the joins are ──────────────────────
    let unmatched = unmatched_between(copy, &segments);
    let cuts = cuts_between(&segments);
    let matched = claimed.iter().filter(|c| **c).count();

    Correspondence {
        segments,
        unmatched,
        cuts,
        copy_frames_examined: copy.len(),
        copy_frames_matched: matched,
        copy_frames_low_variance: low_variance,
        threshold_used: threshold,
        original_self_similarity: similarity,
    }
}

/// Weight of the strongest unbroken run this offset can produce.
///
/// Same run rules as segment growing — consecutive in copy order, no long
/// silence, moving forward through the original — so the ranking is scored on
/// the same thing the segments are built from.
fn best_run_weight(
    centre: i64,
    offsets: &[i64],
    pairs: &[(usize, usize, u32)],
    copy: &[Sample],
    original: &[Sample],
    threshold: u32,
) -> i64 {
    let mut best: std::collections::BTreeMap<usize, (usize, u32)> = Default::default();
    for (n, &off) in offsets.iter().enumerate() {
        if (off - centre).abs() > OFFSET_WINDOW_US {
            continue;
        }
        let (ci, oi, d) = pairs[n];
        best.entry(ci)
            .and_modify(|e| {
                if d < e.1 {
                    *e = (oi, d);
                }
            })
            .or_insert((oi, d));
    }

    let weight = |d: u32| -> i64 { (threshold as i64 - d as i64 + 1).max(1) };
    let (mut best_total, mut total) = (0i64, 0i64);
    let mut last: Option<(usize, usize)> = None;
    for (ci, (oi, d)) in best {
        let breaks = last.is_some_and(|(lc, lo)| {
            copy[ci].t_us - copy[lc].t_us > MAX_RUN_GAP_US || original[oi].t_us < original[lo].t_us
        });
        if breaks {
            best_total = best_total.max(total);
            total = 0;
        }
        total += weight(d);
        last = Some((ci, oi));
    }
    best_total.max(total)
}

/// Turn a completed run into a segment, if it earns being one.
fn flush_run(
    run: &mut Vec<(usize, usize, u32)>,
    copy: &[Sample],
    original: &[Sample],
    claimed: &mut [bool],
    out: &mut Vec<Segment>,
) {
    if run.len() >= MIN_SEGMENT_FRAMES {
        let first = run[0];
        let last = run[run.len() - 1];

        // A run whose matched frames all sit on ONE instant of the original is
        // not a stretch of correspondence — it is a single original frame that
        // several copy frames happen to resemble. Reporting it as a segment
        // gives a duration to something that has none.
        if original[last.1].t_us <= original[first.1].t_us {
            run.clear();
            return;
        }

        let mean = run.iter().map(|e| e.2 as f32).sum::<f32>() / run.len() as f32;
        // A run whose frames only just scrape inside the proposal threshold is
        // coincidence wearing a segment's clothes.
        if mean > MAX_SEGMENT_MEAN_DISTANCE {
            run.clear();
            return;
        }

        // Density: of the copy frames inside this run's own span, how many are
        // the ones that agreed? A sparse run is coincidence, not
        // correspondence. Frames with no picture are left out of the
        // denominator — they were never eligible to agree.
        let span_eligible = copy
            .iter()
            .filter(|s| {
                s.t_us >= copy[first.0].t_us
                    && s.t_us <= copy[last.0].t_us
                    && !s.fp.is_low_variance()
            })
            .count();
        if span_eligible > 0 && (run.len() as f32) / (span_eligible as f32) < MIN_RUN_DENSITY {
            run.clear();
            return;
        }

        // The offset reported is the median of the run's own offsets, not the
        // window it was grouped into: the window is a counting device, the
        // median is the measurement.
        let mut offsets: Vec<i64> = run
            .iter()
            .map(|e| original[e.1].t_us - copy[e.0].t_us)
            .collect();
        offsets.sort_unstable();
        out.push(Segment {
            copy_start_us: copy[first.0].t_us,
            copy_end_us: copy[last.0].t_us,
            original_start_us: original[first.1].t_us,
            original_end_us: original[last.1].t_us,
            offset_us: offsets[offsets.len() / 2],
            frames_agreeing: run.len(),
            mean_distance: mean,
        });
        for e in run.iter() {
            claimed[e.0] = true;
        }
    }
    run.clear();
}

/// Copy time no segment covers, with how many frames were examined inside it.
fn unmatched_between(copy: &[Sample], segments: &[Segment]) -> Vec<Unmatched> {
    if copy.is_empty() {
        return Vec::new();
    }
    let start = copy[0].t_us;
    let end = copy[copy.len() - 1].t_us;
    let mut gaps: Vec<(i64, i64)> = Vec::new();
    let mut cursor = start;
    for s in segments {
        if s.copy_start_us > cursor {
            gaps.push((cursor, s.copy_start_us));
        }
        cursor = cursor.max(s.copy_end_us);
    }
    if cursor < end {
        gaps.push((cursor, end));
    }
    gaps.into_iter()
        .map(|(a, b)| Unmatched {
            copy_start_us: a,
            copy_end_us: b,
            frames_examined: copy.iter().filter(|s| s.t_us >= a && s.t_us <= b).count(),
        })
        .collect()
}

/// Cuts are the joins between consecutive segments, never searched for
/// separately: a second method could disagree with the first.
fn cuts_between(segments: &[Segment]) -> Vec<Cut> {
    segments
        .windows(2)
        .map(|w| Cut {
            copy_at_us: (w[0].copy_end_us + w[1].copy_start_us) / 2,
            original_left_us: w[0].original_end_us,
            original_resumed_us: w[1].original_start_us,
        })
        .collect()
}

fn whole_copy_unmatched(copy: &[Sample]) -> Vec<Unmatched> {
    if copy.is_empty() {
        return Vec::new();
    }
    vec![Unmatched {
        copy_start_us: copy[0].t_us,
        copy_end_us: copy[copy.len() - 1].t_us,
        frames_examined: copy.len(),
    }]
}

/// Prints a run about to be flushed, when `EDIT_REPORT_DEBUG_ASSIGN` is set.
fn debug_flush(
    rank: usize,
    centres: &[i64],
    run: &[(usize, usize, u32)],
    copy: &[Sample],
    original: &[Sample],
    why: &str,
) {
    if std::env::var_os("EDIT_REPORT_DEBUG_ASSIGN").is_none() || run.is_empty() {
        return;
    }
    let first = run[0];
    let last = run[run.len() - 1];
    let span_eligible = copy
        .iter()
        .filter(|s| {
            s.t_us >= copy[first.0].t_us && s.t_us <= copy[last.0].t_us && !s.fp.is_low_variance()
        })
        .count();
    let mean = run.iter().map(|e| e.2 as f32).sum::<f32>() / run.len() as f32;
    eprintln!(
        "    [run] centre {:+7.2}  copy {:6.2}→{:6.2}  orig {:6.2}→{:6.2}  n={:3} eligible={:3} density={:.2} mean={:.1}  ({why})",
        centres[rank] as f64 / 1e6,
        copy[first.0].t_us as f64 / 1e6,
        copy[last.0].t_us as f64 / 1e6,
        original[first.1].t_us as f64 / 1e6,
        original[last.1].t_us as f64 / 1e6,
        run.len(),
        span_eligible,
        run.len() as f32 / span_eligible.max(1) as f32,
        mean,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fingerprint that is distinct per `id` and far from every other.
    fn fp(id: u64) -> Fingerprint {
        // Spread the bits so distances between different ids are large.
        let mut bits = 0u64;
        let mut h = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
        for _ in 0..4 {
            h ^= h >> 33;
            h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
            bits ^= h;
        }
        Fingerprint {
            bits: bits & !1,
            spread: 12.0,
        }
    }

    fn seq(ids: &[u64], start_us: i64, step_us: i64) -> Vec<Sample> {
        ids.iter()
            .enumerate()
            .map(|(i, &id)| Sample {
                index: i as u64,
                t_us: start_us + i as i64 * step_us,
                fp: fp(id),
            })
            .collect()
    }

    const STEP: i64 = 500_000; // 2 samples per second

    #[test]
    fn an_identical_copy_is_one_segment_with_no_cuts() {
        let ids: Vec<u64> = (0..40).collect();
        let original = seq(&ids, 0, STEP);
        let copy = seq(&ids, 0, STEP);
        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 1, "{:?}", c.segments);
        assert!(c.cuts.is_empty());
        assert!(c.unmatched.is_empty());
        assert_eq!(c.segments[0].offset_us, 0);
        assert_eq!(c.copy_frames_matched, copy.len());
    }

    #[test]
    fn a_single_extract_is_one_segment_at_a_constant_offset() {
        let ids: Vec<u64> = (0..60).collect();
        let original = seq(&ids, 0, STEP);
        // Copy holds original frames 20..40, restarting its own clock at 0.
        let copy = seq(&ids[20..40], 0, STEP);
        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 1);
        assert!(c.cuts.is_empty());
        assert_eq!(c.segments[0].offset_us, 20 * STEP);
    }

    #[test]
    fn three_non_contiguous_segments_give_two_cuts() {
        let ids: Vec<u64> = (0..90).collect();
        let original = seq(&ids, 0, STEP);
        let mut montage: Vec<u64> = Vec::new();
        montage.extend_from_slice(&ids[5..15]);
        montage.extend_from_slice(&ids[40..50]);
        montage.extend_from_slice(&ids[70..80]);
        let copy = seq(&montage, 0, STEP);

        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 3, "{:#?}", c.segments);
        assert_eq!(c.cuts.len(), 2);
        // Each cut skips forward through the original, and says by how much.
        for cut in &c.cuts {
            assert!(!cut.goes_backwards());
            assert!(cut.original_skipped_us() > 0);
        }
        // The offsets are the three different ones, in copy order.
        let offsets: Vec<i64> = c.segments.iter().map(|s| s.offset_us).collect();
        // Copy frame 10 holds original frame 40, so the offset is 30 steps,
        // not 35: an offset is original-time minus COPY-time, and the copy
        // restarted its clock at zero.
        assert_eq!(offsets, vec![5 * STEP, 30 * STEP, 50 * STEP]);
    }

    #[test]
    fn a_re_ordered_montage_is_reported_as_going_backwards() {
        let ids: Vec<u64> = (0..60).collect();
        let original = seq(&ids, 0, STEP);
        let mut reordered: Vec<u64> = Vec::new();
        reordered.extend_from_slice(&ids[40..50]);
        reordered.extend_from_slice(&ids[5..15]);
        let copy = seq(&reordered, 0, STEP);

        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 2);
        assert_eq!(c.cuts.len(), 1);
        assert!(c.cuts[0].goes_backwards(), "{:?}", c.cuts[0]);
    }

    #[test]
    fn an_unrelated_video_establishes_no_correspondence() {
        // Repeated over twenty independent pairings, because a single one
        // proves nothing here. The distance threshold is deliberately loose, so
        // a handful of unrelated frames fall inside it on any given pairing and
        // a few of those share an offset by chance. Density is what refuses
        // them: a real correspondence agrees at nearly every frame of its span,
        // coincidences are sparse. One pairing could get lucky; twenty in a row
        // is the claim worth making.
        for seed in 0..20u64 {
            let original = seq(&(0..40).collect::<Vec<_>>(), 0, STEP);
            let base = 1_000 + seed * 1_000;
            let copy = seq(&(base..base + 40).collect::<Vec<_>>(), 0, STEP);
            let c = align(&copy, &original);
            assert!(c.nothing_established(), "seed {seed}: {:#?}", c.segments);
            assert_eq!(c.copy_frames_matched, 0);
            // The whole copy is reported as unmatched, WITH how many frames
            // were looked at — "nothing matched" and "nothing was examined"
            // are different statements.
            assert_eq!(c.unmatched.len(), 1);
            assert_eq!(c.unmatched[0].frames_examined, copy.len());
        }
    }

    #[test]
    fn a_couple_of_real_frames_do_not_become_a_whole_segment_on_their_own() {
        // Two genuine frames from the original hidden in otherwise foreign
        // material. Two is below the minimum, so on their own they establish
        // nothing — and the report says the copy is unmatched rather than
        // inventing a correspondence around them.
        let ids: Vec<u64> = (0..40).collect();
        let original = seq(&ids, 0, STEP);
        let mut mixed: Vec<u64> = (7000..7020).collect();
        mixed[8] = ids[10];
        mixed[9] = ids[11];
        let copy = seq(&mixed, 0, STEP);
        let c = align(&copy, &original);
        // Whatever it finds, it cannot claim more copy time than the two
        // frames could possibly support.
        assert!(
            c.matched_duration_us() <= 2 * STEP,
            "claimed {} us from two frames: {:#?}",
            c.matched_duration_us(),
            c.segments
        );
    }

    #[test]
    fn inserted_foreign_material_leaves_a_hole_between_two_segments() {
        let ids: Vec<u64> = (0..60).collect();
        let original = seq(&ids, 0, STEP);
        let mut spliced: Vec<u64> = Vec::new();
        spliced.extend_from_slice(&ids[0..12]);
        spliced.extend((2000..2010).collect::<Vec<u64>>()); // not in the original
        spliced.extend_from_slice(&ids[12..24]);
        let copy = seq(&spliced, 0, STEP);

        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 2, "{:#?}", c.segments);
        assert_eq!(c.unmatched.len(), 1, "{:#?}", c.unmatched);
        assert!(c.unmatched[0].frames_examined >= 8);
    }

    #[test]
    fn blank_frames_never_build_a_segment_out_of_nothing() {
        // Twenty frames of grey sky in both videos. They are within the
        // threshold of each other and of everything else; if they voted, they
        // would manufacture a correspondence between unrelated recordings.
        let flat = |n: usize, start: i64| -> Vec<Sample> {
            (0..n)
                .map(|i| Sample {
                    index: i as u64,
                    t_us: start + i as i64 * STEP,
                    fp: Fingerprint {
                        bits: 0xAAAA_AAAA_AAAA_AAAA & !1,
                        spread: 0.2,
                    },
                })
                .collect()
        };
        let c = align(&flat(20, 0), &flat(20, 0));
        assert!(c.nothing_established());
        assert_eq!(c.copy_frames_low_variance, 20);
    }

    #[test]
    fn segments_never_overlap_in_the_copys_timeline() {
        // Two offsets competing over the same passage used to take alternate
        // frames of it and report two stretches covering the same instants.
        let ids: Vec<u64> = (0..80).collect();
        let original = seq(&ids, 0, STEP);
        let mut montage: Vec<u64> = Vec::new();
        montage.extend_from_slice(&ids[4..20]);
        montage.extend_from_slice(&ids[40..56]);
        montage.extend_from_slice(&ids[60..76]);
        let copy = seq(&montage, 0, STEP);

        let c = align(&copy, &original);
        for pair in c.segments.windows(2) {
            assert!(
                pair[0].copy_end_us <= pair[1].copy_start_us,
                "segments overlap: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn a_run_pinned_to_one_instant_of_the_original_is_not_a_segment() {
        // Several copy frames resembling ONE original frame is not a stretch
        // of correspondence, and giving it a duration would invent one.
        let original = seq(&(0..40).collect::<Vec<_>>(), 0, STEP);
        let same = vec![7u64; 12];
        let copy = seq(&same, 0, STEP);
        let c = align(&copy, &original);
        for s in &c.segments {
            assert!(
                s.original_end_us > s.original_start_us,
                "segment has a zero-length original interval: {s:?}"
            );
        }
    }

    #[test]
    fn a_dense_short_run_is_still_a_segment() {
        // The other side of the density rule: a genuinely short clip must not
        // be refused for being short. Six frames, all agreeing, nothing else
        // in the copy.
        let ids: Vec<u64> = (0..40).collect();
        let original = seq(&ids, 0, STEP);
        let copy = seq(&ids[12..18], 0, STEP);
        let c = align(&copy, &original);
        assert_eq!(c.segments.len(), 1, "{:#?}", c.segments);
        assert_eq!(c.segments[0].frames_agreeing, 6);
    }

    #[test]
    fn the_result_does_not_depend_on_iteration_order() {
        // Offsets are counted in a HashMap. Without an explicit tie-break the
        // report would differ between runs on identical input, which is fatal
        // for something two people are meant to compare.
        let ids: Vec<u64> = (0..60).collect();
        let original = seq(&ids, 0, STEP);
        let mut montage: Vec<u64> = Vec::new();
        montage.extend_from_slice(&ids[5..15]);
        montage.extend_from_slice(&ids[30..40]);
        let copy = seq(&montage, 0, STEP);
        let first = align(&copy, &original);
        for _ in 0..8 {
            assert_eq!(align(&copy, &original), first);
        }
    }

    #[test]
    fn an_empty_side_yields_nothing_established_rather_than_a_panic() {
        let original = seq(&(0..10).collect::<Vec<_>>(), 0, STEP);
        assert!(align(&[], &original).nothing_established());
        let copy = seq(&(0..10).collect::<Vec<_>>(), 0, STEP);
        let c = align(&copy, &[]);
        assert!(c.nothing_established());
        assert_eq!(c.copy_frames_examined, 10);
    }
}
