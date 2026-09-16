//! What the copy's own picture says about where it came from, and whether the
//! pictures agree.
//!
//! ## The two halves, and why they are separate
//!
//! A Forsheur frame carries a counter burned into it. A copy frame showing
//! `f=1528` is **declaring** that it is frame 1528 of its recording. That
//! declaration turns alignment from a search into a check, and it is the whole
//! reason this path exists.
//!
//! It is also unsigned. Anyone can draw `f=1528` into anything. So every
//! declaration is put to the original: fetch the original's frame 1528 and see
//! whether the pictures are the same. Three outcomes, and the third is not a
//! failure of the tool:
//!
//!   * **confirmed** — the copy frame declares a frame of the original and
//!     looks like it;
//!   * **contradicted** — it declares one and does not look like it;
//!   * **unverifiable** — no counter could be read, or the original has no
//!     frame there to compare against.
//!
//! **The safety property that makes this worth doing.** Forging the band can
//! make this tool fail to establish a correspondence. It cannot make it assert
//! a false one: a forged counter points at a frame of the original, that frame
//! is fetched, and the pictures decide. The direction of the failure is the
//! right one.
//!
//! ## What the sequence of counters shows
//!
//! Once frames are confirmed, the edit is read straight off their counters:
//!
//! | In the counters | In the copy |
//! |---|---|
//! | runs forward, step matching the sampling | one continuous stretch |
//! | jumps forward | material was cut out |
//! | goes backwards, or repeats | a stretch was moved or used twice |
//! | absent, or present and contradicted | material that is not this recording |

use crate::fingerprint::Fingerprint;

/// One examined frame of the copy and what its band declared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Declared {
    pub copy_index: u64,
    pub copy_t_us: i64,
    /// The `f=` counter read from the picture, if any.
    pub counter: Option<u64>,
    /// The two-byte recording signature carried by the machine-readable strip,
    /// when the strip was read. `None` means no strip, which is not the same
    /// thing as a strip carrying the wrong signature.
    pub tag: Option<u16>,
    pub fp: Fingerprint,
}

/// One frame of the original, keyed by the counter its own band carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OriginalFrame {
    pub counter: u64,
    pub index: u64,
    pub t_us: i64,
    pub fp: Fingerprint,
}

/// One frame worth naming in the report: where it is in the copy, what it
/// declared, and how far its picture stands from the original's.
///
/// Counts alone would hide the single frame that matters. A reader has to be
/// able to go and look at it, so every frame that is not plainly conforming is
/// listed with the timestamp that puts it on screen.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameNote {
    pub copy_t_us: i64,
    pub copy_index: u64,
    pub counter: Option<u64>,
    /// Where the same counter sits on the original's timeline.
    pub original_t_us: Option<i64>,
    /// Bits differing out of the fingerprint's 63, when a comparison happened.
    pub distance: Option<u32>,
    pub reason: NoteReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteReason {
    /// Declared a frame of the original and does not look like it.
    PictureDiffers,
    /// Declared a counter the original does not have.
    NotInOriginal,
    /// Nothing could be read from the picture, so nothing was put to the
    /// original. Neither conforming nor differing — this is the third state,
    /// and it must stay populated.
    NoDeclaration,
    /// The question was put and the fingerprint did not settle it: too far to
    /// confirm, not far enough to call a different picture.
    TooFarToJudge,
    /// The strip names another recording. Not compared to this one.
    ForeignSignature,
}

/// How a single copy frame stands against the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameVerdict {
    /// Declared a frame of the original, and looks like it.
    Confirmed,
    /// Declared a frame of the original, and does not look like it.
    Contradicted,
    /// Declared a frame of the original and the pictures neither agree nor
    /// plainly disagree. Not evidence in either direction.
    Unsettled,
    /// Declared a counter the original does not have.
    DeclaredOutsideOriginal,
    /// No counter could be read from the picture.
    NoDeclaration,
    /// The strip carries another recording's signature. Its counter is that
    /// recording's and says nothing about this original, so nothing is put
    /// to it: "frame 12 of recording B" is not a claim about recording A,
    /// and comparing it as one produced the accusation-shaped "names a moment
    /// of the original and does not look like it" for material that had
    /// simply announced, in its own strip, where it came from.
    Foreign,
}

/// Distance at or below which the picture is taken to confirm the declaration.
///
/// Looser than the threshold the blind search uses, and deliberately so: this
/// is not choosing among candidates, it is checking one named frame. The only
/// question is whether the two pictures are the same picture after whatever a
/// platform did to it, and a re-encode down to 150 kbit/s moves a fingerprint
/// by a couple of bits on this material.
pub const CONFIRM_DISTANCE: u32 = 16;

/// Distance at or above which the two pictures are taken to be different
/// pictures, rather than the same one after a rough journey.
///
/// One threshold is not enough, and using one made the report accuse a copy
/// that had only been recompressed: at 150 kbit/s on real material three
/// frames of a faithful Android recording crossed 16, and were listed as
/// differing. Two thresholds give the band in between its own name. Frames
/// there are not conforming and are not differing — they are frames the
/// fingerprint cannot settle, which is what the third state is for and why it
/// has to stay well populated.
///
/// Two unrelated pictures sit near 31 of 63 by construction, since half the
/// bits agree by chance. 26 is clear of the recompression noise and clear of
/// chance.
pub const DIFFER_DISTANCE: u32 = 26;

/// A stretch of copy whose frames confirm a continuous run of the original.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeclaredSegment {
    pub copy_start_us: i64,
    pub copy_end_us: i64,
    /// Counters at each end, as burned into the picture and confirmed.
    pub counter_start: u64,
    pub counter_end: u64,
    pub original_start_us: i64,
    pub original_end_us: i64,
    /// Every frame of the copy inside the shot's span, however it turned out.
    pub frames_examined: usize,
    pub frames_confirmed: usize,
    /// Frames that declared a moment of the original and do not look like it,
    /// each one named. A shot is not summarised by an average: one frame that
    /// differs is the whole point, and an average of five hundred would bury
    /// it. The list is the finding.
    pub differing: Vec<FrameNote>,
    /// Frames nothing could be established about. Never evidence of anything;
    /// shown so the reader can see how much of the shot was not judged.
    pub inconclusive: Vec<FrameNote>,
    /// The largest distance seen among the confirmed frames — the closest this
    /// shot ever came to differing, which is what a mean would hide.
    pub worst_confirmed_distance: u32,
}

impl DeclaredSegment {
    /// A shot with nothing to report: every frame examined was confirmed.
    pub fn is_clean(&self) -> bool {
        self.differing.is_empty() && self.inconclusive.is_empty()
    }
}

/// A discontinuity between two confirmed stretches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeclaredCut {
    pub copy_at_us: i64,
    /// The last counter before the join and the first after it.
    pub counter_before: u64,
    pub counter_after: u64,
    pub original_left_us: i64,
    pub original_resumed_us: i64,
    /// Where the two shots end and resume in the copy, so the copy's own
    /// elapsed time across the join can be compared with the original's.
    pub copy_left_us: i64,
    pub copy_resumed_us: i64,
}

/// What kind of join this is. Reading the counters alone cannot tell removal
/// from insertion — both leave a gap between two shots — so the two elapsed
/// times decide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    /// The original runs on further than the copy does: material of the
    /// original is not here.
    Removal,
    /// The copy runs on further than the original does: time was spent here
    /// on something the original does not account for.
    Insertion,
    /// The copy returns to an earlier moment of the original.
    Backwards,
}

impl DeclaredCut {
    /// Frames of the original skipped over. Negative when the copy goes back.
    pub fn counters_skipped(&self) -> i64 {
        self.counter_after as i64 - self.counter_before as i64
    }

    pub fn goes_backwards(&self) -> bool {
        self.counters_skipped() < 0
    }

    /// Copy time across the join that the original does not account for.
    ///
    /// Positive means the copy lingered where the original moved on — that is
    /// material the original does not have. Negative means the original moved
    /// on where the copy did not — that is material of the original missing.
    ///
    /// Both show up as a gap between two shots, which is why counting skipped
    /// counters alone got an insertion of 30 frames reported as "1 frame of
    /// the original absent" — true, and beside the point.
    pub fn inserted_us(&self) -> i64 {
        let copy_gap = self.copy_resumed_us - self.copy_left_us;
        let original_gap = self.original_resumed_us - self.original_left_us;
        copy_gap - original_gap
    }

    pub fn kind(&self) -> JoinKind {
        if self.goes_backwards() {
            JoinKind::Backwards
        } else if self.inserted_us() > 0 {
            JoinKind::Insertion
        } else {
            JoinKind::Removal
        }
    }
}

/// What the caller has to tell the core, because the core cannot know it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Distance at or below which a picture confirms its declaration.
    pub confirm_distance: u32,
    /// Distance at or above which the pictures are taken to be different.
    /// Between the two the frame is inconclusive, never differing.
    pub differ_distance: u32,
    /// Floor on the continuity tolerance, in microseconds.
    ///
    /// It exists to absorb the step between two examined frames, so it belongs
    /// to the sampling and not to this module. Reading every frame makes the
    /// step one frame period, and the floor should follow: left at the value
    /// that suited four samples a second, a cut of half a second would slip
    /// through as continuous.
    pub continuity_floor_us: f64,
    /// The recording signature the bundle says these frames should carry.
    pub expected_tag: Option<u16>,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            confirm_distance: CONFIRM_DISTANCE,
            differ_distance: DIFFER_DISTANCE,
            continuity_floor_us: CONTINUITY_FLOOR_US,
            expected_tag: None,
        }
    }
}

impl Tuning {
    /// Every frame is read, so the step between two examined frames is one
    /// frame period; three of them is a floor that absorbs the step without
    /// absorbing an edit.
    pub fn every_frame(fps: f64) -> Self {
        let period = if fps > 0.0 {
            1_000_000.0 / fps
        } else {
            33_333.0
        };
        Tuning {
            continuity_floor_us: (period * 3.0).max(50_000.0),
            ..Tuning::default()
        }
    }
}

/// A stretch of copy that confirms nothing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnconfirmedStretch {
    pub copy_start_us: i64,
    pub copy_end_us: i64,
    pub frames_examined: usize,
    /// How many of them read a counter at all.
    pub frames_declaring: usize,
    /// How many declared a frame of the original and did not look like it.
    /// The sharpest signal available here — and still not an accusation.
    pub frames_contradicted: usize,
    /// How many carry another recording's signature. Never compared to this
    /// original: their strip already said where they came from.
    pub frames_foreign: usize,
    /// Those signatures, with how many frames carried each.
    pub foreign_tags: Vec<(u16, usize)>,
}

/// Everything the declarations establish. No score, no verdict on the video.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeclaredCorrespondence {
    pub segments: Vec<DeclaredSegment>,
    pub cuts: Vec<DeclaredCut>,
    pub unconfirmed: Vec<UnconfirmedStretch>,
    pub frames_examined: usize,
    pub frames_declaring: usize,
    pub frames_confirmed: usize,
    pub frames_contradicted: usize,
    /// Counters read more than once in the copy — a stretch used twice.
    pub counters_repeated: usize,
    /// Contradictions that stood alone between two confirmations. Reported
    /// separately because they are the reader's own noise, not the video's.
    pub isolated_contradictions: usize,
    /// Counters set aside as misreads: the sequence ran on without them.
    /// Also the reader's noise, and also worth showing rather than hiding.
    pub counters_set_aside: usize,
    pub confirm_distance: u32,
    /// The recording signature the bundle says to expect, when one was given.
    pub tag_expected: Option<u16>,
    /// Every signature actually seen in the copy's strips, with how many
    /// frames carried it. More than one entry means frames from more than one
    /// recording; an entry that is not `tag_expected` means frames from
    /// another recording altogether.
    pub tags_seen: Vec<(u16, usize)>,
    /// Frames whose strip could not be read at all. Not a signature mismatch —
    /// a cropped, rescaled or overwritten strip reads as nothing.
    pub frames_without_band: usize,
}

impl DeclaredCorrespondence {
    /// Whether any frame carries a signature other than the expected one.
    /// Decisive and cheap: it settles a swapped recording without comparing a
    /// single picture. It cannot settle the reverse — a signature is two bytes
    /// and anyone can draw them.
    pub fn carries_a_foreign_signature(&self) -> bool {
        match self.tag_expected {
            None => self.tags_seen.len() > 1,
            Some(want) => self.tags_seen.iter().any(|(t, _)| *t != want),
        }
    }
}

impl DeclaredCorrespondence {
    pub fn nothing_confirmed(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn confirmed_duration_us(&self) -> i64 {
        self.segments
            .iter()
            .map(|s| s.copy_end_us - s.copy_start_us)
            .sum()
    }
}

/// How far the copy's elapsed time may stray from the original's between the
/// same two counters, as a fraction plus a floor in seconds.
///
/// **Continuity is measured against the ORIGINAL's own timeline, not against a
/// frame rate.** Two counters that the original itself separates by four
/// seconds must be four seconds apart in the copy. That is the whole test.
///
/// Two earlier versions were wrong, each in an instructive way:
///
///   * comparing the counter's advance against the MEDIAN advance observed
///     made the test depend on how many frames the reader happened to manage,
///     and it refuses about half. A faithful copy came back with six cuts in
///     it, each at a place where two reads simply sat further apart than usual.
///
///   * comparing it against a constant frame rate assumed the counter advances
///     with the clock. It does not. The counter increments once per COMPOSED
///     frame, and when the sensor delivers nothing — common on Android — no
///     frame is composed while time keeps running. A recording that dropped
///     frames would have been reported as cut at every drop.
///
/// The original carries every one of those irregularities already, because it
/// is the same recording. Asking it what the interval between two counters
/// should be costs nothing and is right by construction.
///
/// The fraction absorbs a copy re-encoded at a slightly different rate; the
/// floor absorbs the sampling step on each side.
const CONTINUITY_TOLERANCE: f64 = 0.25;
const CONTINUITY_FLOOR_US: f64 = 700_000.0;

/// Put every declaration to the original.
pub fn verify(
    copy: &[Declared],
    original: &[OriginalFrame],
    confirm_distance: u32,
) -> (Vec<FrameVerdict>, Vec<f32>) {
    verify_with(copy, original, confirm_distance, DIFFER_DISTANCE, None)
}

pub fn verify_with(
    copy: &[Declared],
    original: &[OriginalFrame],
    confirm_distance: u32,
    differ_distance: u32,
    expected_tag: Option<u16>,
) -> (Vec<FrameVerdict>, Vec<f32>) {
    let mut verdicts = Vec::with_capacity(copy.len());
    let mut distances = Vec::with_capacity(copy.len());
    for c in copy {
        // Another recording's frame is not a claim about this one. Settled
        // before the counter is even looked at.
        if let (Some(want), Some(got)) = (expected_tag, c.tag) {
            if got != want {
                verdicts.push(FrameVerdict::Foreign);
                distances.push(f32::NAN);
                continue;
            }
        }
        let Some(counter) = c.counter else {
            verdicts.push(FrameVerdict::NoDeclaration);
            distances.push(f32::NAN);
            continue;
        };
        // The original's frame with EXACTLY this counter, or nothing.
        //
        // This used to take the nearest one instead, and that was a way of
        // manufacturing accusations. Both sides were sampled one frame in
        // seven; after a cut the two grids no longer line up, so a copy frame
        // declaring 425 was compared against the original's 428 — a tenth of a
        // second away, a different picture on anything that moves — and the
        // difference was reported as a contradiction. Nineteen faithful frames
        // were accused that way on a single test, and the strip's own counters
        // proved them faithful.
        //
        // A declaration the original cannot answer is not a contradiction. It
        // is a question left open, and it says so.
        let Some(o) = exact_by_counter(original, counter) else {
            verdicts.push(FrameVerdict::DeclaredOutsideOriginal);
            distances.push(f32::NAN);
            continue;
        };
        let d = c.fp.distance(o.fp);
        distances.push(d as f32);
        verdicts.push(if d <= confirm_distance {
            FrameVerdict::Confirmed
        } else if d >= differ_distance {
            FrameVerdict::Contradicted
        } else {
            FrameVerdict::Unsettled
        });
    }
    (verdicts, distances)
}

/// The original's frame carrying exactly this counter.
///
/// `consistent_original` leaves the slice sorted by counter, so this is a
/// binary search.
fn exact_by_counter(original: &[OriginalFrame], counter: u64) -> Option<&OriginalFrame> {
    original
        .binary_search_by_key(&counter, |o| o.counter)
        .ok()
        .map(|i| &original[i])
}

/// The original's frame nearest this counter. Used only to place a segment's
/// ends on the original's timeline, never to judge a picture.
fn nearest_by_counter(original: &[OriginalFrame], counter: u64) -> Option<&OriginalFrame> {
    original.iter().min_by_key(|o| o.counter.abs_diff(counter))
}

/// Build the correspondence with the defaults. Kept for callers that sample.
pub fn build(
    copy: &[Declared],
    original: &[OriginalFrame],
    confirm_distance: u32,
) -> DeclaredCorrespondence {
    build_with(
        copy,
        original,
        Tuning {
            confirm_distance,
            ..Tuning::default()
        },
    )
}

/// Build the correspondence from declarations that were put to the original.
pub fn build_with(
    copy: &[Declared],
    original: &[OriginalFrame],
    tuning: Tuning,
) -> DeclaredCorrespondence {
    let confirm_distance = tuning.confirm_distance;
    // The original's own counters, cleaned before anything is measured
    // against them.
    let original = consistent_original(original);
    let original = &original[..];

    let (verdicts, distances) = verify_with(
        copy,
        original,
        confirm_distance,
        tuning.differ_distance.max(confirm_distance + 1),
        tuning.expected_tag,
    );

    let frames_declaring = copy.iter().filter(|c| c.counter.is_some()).count();
    let frames_confirmed = verdicts
        .iter()
        .filter(|v| **v == FrameVerdict::Confirmed)
        .count();
    let frames_contradicted = verdicts
        .iter()
        .filter(|v| **v == FrameVerdict::Contradicted)
        .count();

    // A counter seen twice means a stretch of the original appears twice in the
    // copy. Counted over CONFIRMED frames only: an unconfirmed repeat says
    // nothing, since the declaration was not established in the first place.
    let mut seen: std::collections::BTreeMap<u64, usize> = Default::default();
    for (c, v) in copy.iter().zip(&verdicts) {
        if *v == FrameVerdict::Confirmed {
            if let Some(k) = c.counter {
                *seen.entry(k).or_default() += 1;
            }
        }
    }
    let counters_repeated = seen.values().filter(|n| **n > 1).count();

    // Drop counters that no neighbour agrees with.
    //
    // A counter read wrong points at some other moment of the original, and on
    // a recording that revisits a view that moment can look close enough to
    // confirm. One such frame then ends a segment in the wrong place and
    // invents a cut: a faithful copy was reported as jumping back 568 frames
    // because a `2` had been read as an `8`.
    //
    // The test is whether the sequence runs on WITHOUT the frame. An edit
    // lasts longer than one examined frame — at four a second, an insertion
    // would have to be under a quarter of a second to look like this — so a
    // lone frame that breaks continuity in both directions while its
    // neighbours agree with each other is a misread, and is set aside rather
    // than believed.
    let mut verdicts = verdicts;
    let confirmed_at: Vec<usize> = (0..copy.len())
        .filter(|i| verdicts[*i] == FrameVerdict::Confirmed)
        .collect();
    let mut misread = 0usize;
    for w in confirmed_at.windows(3) {
        let (p, i, n) = (w[0], w[1], w[2]);
        let (cp, ci, cn) = (
            copy[p].counter.unwrap_or(0),
            copy[i].counter.unwrap_or(0),
            copy[n].counter.unwrap_or(0),
        );
        let link = |a: usize, b: usize, ca: u64, cb: u64| -> bool {
            cb > ca
                && is_continuous(
                    original,
                    ca,
                    cb,
                    copy[b].copy_t_us - copy[a].copy_t_us,
                    tuning.continuity_floor_us,
                )
        };
        if !link(p, i, cp, ci) && !link(i, n, ci, cn) && link(p, n, cp, cn) {
            verdicts[i] = FrameVerdict::NoDeclaration;
            misread += 1;
        }
    }

    let mut segments: Vec<DeclaredSegment> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    let flush = |run: &mut Vec<usize>, out: &mut Vec<DeclaredSegment>| {
        if run.len() >= 2 {
            let (a, b) = (run[0], run[run.len() - 1]);
            let ca = copy[a].counter.unwrap_or(0);
            let cb = copy[b].counter.unwrap_or(0);
            let oa = nearest_by_counter(original, ca);
            let ob = nearest_by_counter(original, cb);

            // Walk the whole span, not just the confirmed frames, so a frame
            // that differs in the middle of an otherwise clean shot is
            // reported inside that shot instead of vanishing between two.
            let mut differing = Vec::new();
            let mut inconclusive = Vec::new();
            let mut worst = 0u32;
            for i in a..=b {
                let note = |reason: NoteReason, distance: Option<u32>| FrameNote {
                    copy_t_us: copy[i].copy_t_us,
                    copy_index: copy[i].copy_index,
                    counter: copy[i].counter,
                    original_t_us: copy[i].counter.and_then(|c| original_time_of(original, c)),
                    distance,
                    reason,
                };
                let d = if distances[i].is_nan() {
                    None
                } else {
                    Some(distances[i] as u32)
                };
                match verdicts[i] {
                    FrameVerdict::Confirmed => worst = worst.max(d.unwrap_or(0)),
                    FrameVerdict::Contradicted => {
                        differing.push(note(NoteReason::PictureDiffers, d))
                    }
                    FrameVerdict::DeclaredOutsideOriginal => {
                        differing.push(note(NoteReason::NotInOriginal, None))
                    }
                    FrameVerdict::Unsettled => {
                        inconclusive.push(note(NoteReason::TooFarToJudge, d))
                    }
                    FrameVerdict::Foreign => {
                        differing.push(note(NoteReason::ForeignSignature, None))
                    }
                    FrameVerdict::NoDeclaration => {
                        inconclusive.push(note(NoteReason::NoDeclaration, None))
                    }
                }
            }

            out.push(DeclaredSegment {
                copy_start_us: copy[a].copy_t_us,
                copy_end_us: copy[b].copy_t_us,
                counter_start: ca,
                counter_end: cb,
                original_start_us: oa.map(|o| o.t_us).unwrap_or(0),
                original_end_us: ob.map(|o| o.t_us).unwrap_or(0),
                frames_examined: b - a + 1,
                frames_confirmed: run.len(),
                differing,
                inconclusive,
                worst_confirmed_distance: worst,
            });
        }
        run.clear();
    };

    for (i, v) in verdicts.iter().enumerate() {
        match v {
            FrameVerdict::Confirmed => {}
            // A frame whose counter could not be read says NOTHING. It must not
            // end a run: the reader refuses roughly half of them, and treating
            // each refusal as a boundary chopped a faithful copy into
            // twenty-odd two-frame segments with no edit anywhere near them.
            // Same for a frame the fingerprint could not settle: it is not
            // evidence, so it must not end a run either.
            FrameVerdict::NoDeclaration | FrameVerdict::Unsettled => continue,
            // These two do say something: a frame that names a moment of the
            // original and does not look like it, or names one the original
            // does not have, is positive evidence of other material here.
            //
            // But ONE of them is not evidence of anything. A counter misread
            // by a digit points at the wrong frame, the pictures then disagree,
            // and a single such frame was enough to split a faithful copy and
            // report two cuts that were never made. Material actually spliced
            // in lasts longer than one examined frame — at four a second, an
            // insertion has to be under a quarter of a second to show up as
            // one — so a lone contradiction is noise and is counted as such.
            FrameVerdict::Contradicted
            | FrameVerdict::DeclaredOutsideOriginal
            | FrameVerdict::Foreign => {
                if is_isolated(&verdicts, i) {
                    continue;
                }
                flush(&mut run, &mut segments);
                continue;
            }
        }
        if let Some(&last) = run.last() {
            let a = copy[last].counter.unwrap_or(0);
            let b = copy[i].counter.unwrap_or(0);
            // A run continues while the copy takes as long between the two
            // counters as the original did. Going backwards, standing still,
            // or leaping ends it — each of those IS the edit, and belongs on
            // the boundary rather than inside a segment.
            let continuous = b > a
                && is_continuous(
                    original,
                    a,
                    b,
                    copy[i].copy_t_us - copy[last].copy_t_us,
                    tuning.continuity_floor_us,
                );
            if !continuous {
                flush(&mut run, &mut segments);
            }
        }
        run.push(i);
    }
    flush(&mut run, &mut segments);

    // Join segments whose junction is continuous.
    //
    // A cut IS a discontinuity, so two stretches that meet with the counter
    // advancing by exactly what the elapsed time requires are one stretch —
    // whatever made the run break in the first place. Without this, a single
    // misread counter in the middle of a faithful copy was reported as a cut
    // of forty frames that nobody made, and the counters either side of it
    // said plainly that nothing had been removed.
    let segments = merge_continuous(segments, original, tuning.continuity_floor_us);

    let cuts = cuts_between(&segments);
    let unconfirmed = unconfirmed_between(copy, &verdicts, &segments);

    DeclaredCorrespondence {
        segments,
        cuts,
        unconfirmed,
        frames_examined: copy.len(),
        frames_declaring,
        frames_confirmed,
        frames_contradicted,
        counters_repeated,
        counters_set_aside: misread,
        isolated_contradictions: (0..verdicts.len())
            .filter(|i| {
                matches!(
                    verdicts[*i],
                    FrameVerdict::Contradicted
                        | FrameVerdict::DeclaredOutsideOriginal
                        | FrameVerdict::Foreign
                ) && is_isolated(&verdicts, *i)
            })
            .count(),
        confirm_distance,
        tag_expected: tuning.expected_tag,
        tags_seen: {
            let mut seen: std::collections::BTreeMap<u16, usize> = Default::default();
            for c in copy {
                if let Some(t) = c.tag {
                    *seen.entry(t).or_default() += 1;
                }
            }
            seen.into_iter().collect()
        },
        frames_without_band: copy.iter().filter(|c| c.tag.is_none()).count(),
    }
}

/// Whether the contradiction at `i` stands alone among confirmations.
///
/// Looks at the nearest neighbour on each side that said anything at all —
/// unread frames are skipped, since they are not evidence either way.
fn is_isolated(verdicts: &[FrameVerdict], i: usize) -> bool {
    let speaking = |range: &mut dyn Iterator<Item = usize>| -> Option<FrameVerdict> {
        range
            .map(|j| verdicts[j])
            .find(|v| *v != FrameVerdict::NoDeclaration)
    };
    let before = speaking(&mut (0..i).rev());
    let after = speaking(&mut (i + 1..verdicts.len()));
    matches!(before, Some(FrameVerdict::Confirmed))
        && matches!(after, Some(FrameVerdict::Confirmed))
}

/// Keep the largest set of the original's frames whose counter increases with
/// time, in time order.
///
/// **The original is one continuous recording, so its counter must rise as its
/// clock does.** Any frame that breaks that read its counter wrong, and one
/// such frame does real damage: the timeline lookup binary-searches this list,
/// so a counter out of place makes every interpolation near it nonsense.
///
/// Measured on a real pair, one frame of the original had its `2` read as an
/// `8` — `f=281` became `f=881`. The COPY misread the same frame the same way,
/// so the pictures agreed and the correspondence was in fact correct; but the
/// impossible counter turned the join into a jump backwards of 568 frames, and
/// a faithful copy was reported as edited.
///
/// The longest increasing subsequence is the honest reading: it keeps the
/// largest consistent story the reads support and sets aside the rest, rather
/// than trusting whichever came first.
fn consistent_original(original: &[OriginalFrame]) -> Vec<OriginalFrame> {
    let mut frames: Vec<OriginalFrame> = original.to_vec();
    frames.sort_by_key(|o| o.t_us);
    if frames.len() < 2 {
        return frames;
    }

    // Patience sorting: `tails[k]` is the smallest counter that can end an
    // increasing run of length k+1, and `prev` walks the chain back.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; frames.len()];
    for i in 0..frames.len() {
        let c = frames[i].counter;
        let pos = tails.partition_point(|&j| frames[j].counter < c);
        prev[i] = if pos > 0 { Some(tails[pos - 1]) } else { None };
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
    }
    let mut chain = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(i) = at {
        chain.push(frames[i]);
        at = prev[i];
    }
    chain.reverse();
    chain
}

/// Where counter `c` sits on the original's own timeline, interpolated
/// between the two examined frames either side of it.
///
/// The original is sampled, so an arbitrary counter usually falls between two
/// of its frames. Interpolating is right because the counter advances smoothly
/// BETWEEN drops; the drops themselves sit between sampled frames and are
/// carried by the interval, which is exactly what makes this immune to them.
pub fn original_time_of(original: &[OriginalFrame], c: u64) -> Option<i64> {
    if original.is_empty() {
        return None;
    }
    let i = original.partition_point(|o| o.counter < c);
    if i == 0 {
        return Some(original[0].t_us);
    }
    if i >= original.len() {
        return Some(original[original.len() - 1].t_us);
    }
    let (lo, hi) = (&original[i - 1], &original[i]);
    let span = hi.counter.saturating_sub(lo.counter);
    if span == 0 {
        return Some(lo.t_us);
    }
    let f = (c - lo.counter) as f64 / span as f64;
    Some(lo.t_us + ((hi.t_us - lo.t_us) as f64 * f) as i64)
}

/// Whether the copy took as long between two counters as the original did.
fn is_continuous(
    original: &[OriginalFrame],
    a: u64,
    b: u64,
    copy_dt_us: i64,
    floor_us: f64,
) -> bool {
    let (Some(ta), Some(tb)) = (original_time_of(original, a), original_time_of(original, b))
    else {
        return false;
    };
    let expected = (tb - ta) as f64;
    let tol = (expected * CONTINUITY_TOLERANCE).abs().max(floor_us);
    (copy_dt_us as f64 - expected).abs() <= tol
}

/// Join consecutive segments whose counters run on across the join.
fn merge_continuous(
    segments: Vec<DeclaredSegment>,
    original: &[OriginalFrame],
    floor_us: f64,
) -> Vec<DeclaredSegment> {
    let mut out: Vec<DeclaredSegment> = Vec::with_capacity(segments.len());
    for seg in segments {
        let joins = out.last().is_some_and(|prev: &DeclaredSegment| {
            seg.counter_start > prev.counter_end
                && is_continuous(
                    original,
                    prev.counter_end,
                    seg.counter_start,
                    seg.copy_start_us - prev.copy_end_us,
                    floor_us,
                )
        });
        match (joins, out.last_mut()) {
            (true, Some(prev)) => {
                prev.frames_confirmed += seg.frames_confirmed;
                prev.frames_examined += seg.frames_examined;
                prev.worst_confirmed_distance = prev
                    .worst_confirmed_distance
                    .max(seg.worst_confirmed_distance);
                prev.differing.extend(seg.differing.iter().copied());
                prev.inconclusive.extend(seg.inconclusive.iter().copied());
                prev.copy_end_us = seg.copy_end_us;
                prev.counter_end = seg.counter_end;
                prev.original_end_us = seg.original_end_us;
            }
            _ => out.push(seg),
        }
    }
    out
}

fn cuts_between(segments: &[DeclaredSegment]) -> Vec<DeclaredCut> {
    segments
        .windows(2)
        .map(|w| DeclaredCut {
            copy_at_us: (w[0].copy_end_us + w[1].copy_start_us) / 2,
            counter_before: w[0].counter_end,
            counter_after: w[1].counter_start,
            original_left_us: w[0].original_end_us,
            original_resumed_us: w[1].original_start_us,
            copy_left_us: w[0].copy_end_us,
            copy_resumed_us: w[1].copy_start_us,
        })
        .collect()
}

fn unconfirmed_between(
    copy: &[Declared],
    verdicts: &[FrameVerdict],
    segments: &[DeclaredSegment],
) -> Vec<UnconfirmedStretch> {
    if copy.is_empty() {
        return Vec::new();
    }
    let start = copy[0].copy_t_us;
    let end = copy[copy.len() - 1].copy_t_us;

    // Each edge remembers whether it abuts a segment. A frame at a segment's
    // endpoint belongs to that segment and must not be counted here as well;
    // a frame at the copy's own first or last instant belongs to nobody and
    // must be. Without the distinction a whole-copy gap loses its two end
    // frames and an interior one gains two.
    let mut gaps: Vec<(i64, bool, i64, bool)> = Vec::new();
    let mut cursor = start;
    let mut after_segment = false;
    for s in segments {
        if s.copy_start_us > cursor {
            gaps.push((cursor, after_segment, s.copy_start_us, true));
        }
        cursor = cursor.max(s.copy_end_us);
        after_segment = true;
    }
    if cursor < end {
        gaps.push((cursor, after_segment, end, false));
    }
    gaps.into_iter()
        .map(|(a, a_open, b, b_open)| {
            let inside: Vec<usize> = (0..copy.len())
                .filter(|i| {
                    let t = copy[*i].copy_t_us;
                    (if a_open { t > a } else { t >= a }) && (if b_open { t < b } else { t <= b })
                })
                .collect();
            // Bounded by its OWN frames, not by the neighbouring shots' ends.
            // `a` is the last confirmed frame of the shot before, so a reader
            // who clicked the stretch's start landed on a genuine frame of
            // the original — identical to it, naturally — and concluded the
            // report was wrong. The first frame this stretch is actually
            // about is the one after.
            UnconfirmedStretch {
                copy_start_us: inside.first().map_or(a, |i| copy[*i].copy_t_us),
                copy_end_us: inside.last().map_or(b, |i| copy[*i].copy_t_us),
                frames_examined: inside.len(),
                frames_declaring: inside
                    .iter()
                    .filter(|i| copy[**i].counter.is_some())
                    .count(),
                frames_contradicted: inside
                    .iter()
                    .filter(|i| verdicts[**i] == FrameVerdict::Contradicted)
                    .count(),
                frames_foreign: inside
                    .iter()
                    .filter(|i| verdicts[**i] == FrameVerdict::Foreign)
                    .count(),
                foreign_tags: {
                    let mut seen: std::collections::BTreeMap<u16, usize> = Default::default();
                    for i in &inside {
                        if verdicts[*i] == FrameVerdict::Foreign {
                            if let Some(t) = copy[*i].tag {
                                *seen.entry(t).or_default() += 1;
                            }
                        }
                    }
                    seen.into_iter().collect()
                },
            }
        })
        // A join between two shots always leaves a sliver — one frame period
        // wide — between the end of one and the start of the next. With every
        // frame read there is nothing inside it, and reporting an empty
        // stretch as "corresponding to nothing" invites the reader to look for
        // something that is not there.
        .filter(|u: &UnconfirmedStretch| u.frames_examined > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(id: u64) -> Fingerprint {
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

    /// An original sampled every 8 counters, as 4/s on 30 fps material.
    fn original(n: u64) -> Vec<OriginalFrame> {
        (0..n)
            .map(|i| OriginalFrame {
                counter: i * 8 + 1,
                index: i * 8,
                t_us: (i * 8) as i64 * 33_333,
                fp: fp(i * 8),
            })
            .collect()
    }

    /// Copy frames declaring the counters of `originals[range]`, in order.
    fn copy_from(orig: &[OriginalFrame], take: &[usize], forge: &[usize]) -> Vec<Declared> {
        take.iter()
            .enumerate()
            .map(|(j, &k)| Declared {
                copy_index: j as u64 * 8,
                copy_t_us: (j as i64) * 8 * 33_333,
                counter: Some(orig[k].counter),
                tag: None,
                // A forged frame declares a real counter and shows something
                // else, which is the attack this path exists to catch.
                fp: if forge.contains(&j) {
                    fp(9_000 + j as u64)
                } else {
                    orig[k].fp
                },
            })
            .collect()
    }

    #[test]
    fn a_clean_copy_is_one_segment_with_no_cuts() {
        let o = original(40);
        let c = copy_from(&o, &(0..40).collect::<Vec<_>>(), &[]);
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 1, "{:#?}", r.segments);
        assert!(r.cuts.is_empty());
        assert_eq!(r.frames_confirmed, 40);
        assert_eq!(r.frames_contradicted, 0);
    }

    /// An original whose capture dropped frames: the counter stalls for a
    /// stretch while its timeline keeps running, and jumps where the numbering
    /// itself skipped. Both happen in the field, Android especially.
    fn original_with_drops(n: u64) -> Vec<OriginalFrame> {
        let mut out = Vec::new();
        let mut counter = 1u64;
        for i in 0..n {
            // Around a third of the way in, the sensor delivers nothing for a
            // while: time advances, the counter barely does.
            let stalled = (n / 3..n / 3 + 4).contains(&i);
            // Later, the numbering itself skips a block.
            if i == 2 * n / 3 {
                counter += 200;
            }
            counter += if stalled { 1 } else { 8 };
            out.push(OriginalFrame {
                counter,
                index: i * 8,
                t_us: (i * 8) as i64 * 33_333,
                fp: fp(counter),
            });
        }
        out
    }

    #[test]
    fn frames_dropped_at_capture_are_not_reported_as_cuts() {
        // The false positive this test exists to prevent. The counter advances
        // once per COMPOSED frame, so a recording that dropped frames has
        // counters that stall against the clock and sometimes skip outright.
        // Judged against a constant frame rate, every drop is a cut; judged
        // against the original's own timeline — which has the identical
        // irregularities — none of them is.
        let o = original_with_drops(40);
        let c: Vec<Declared> = o
            .iter()
            .enumerate()
            .map(|(j, f)| Declared {
                copy_index: j as u64 * 8,
                copy_t_us: f.t_us,
                counter: Some(f.counter),
                tag: None,
                fp: f.fp,
            })
            .collect();
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 1, "drops became cuts: {:#?}", r.segments);
        assert!(r.cuts.is_empty(), "{:#?}", r.cuts);
    }

    #[test]
    fn a_cut_in_a_recording_that_also_dropped_frames_is_still_found() {
        // And the other half: absorbing drops must not absorb edits.
        let o = original_with_drops(60);
        let mut c: Vec<Declared> = Vec::new();
        for (j, f) in o.iter().enumerate().filter(|(j, _)| *j < 15 || *j >= 40) {
            c.push(Declared {
                copy_index: c.len() as u64 * 8,
                copy_t_us: (c.len() as i64) * 8 * 33_333,
                counter: Some(f.counter),
                tag: None,
                fp: f.fp,
            });
            let _ = j;
        }
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 2, "{:#?}", r.segments);
        assert_eq!(r.cuts.len(), 1);
        assert!(!r.cuts[0].goes_backwards());
    }

    #[test]
    fn a_cut_shows_as_a_jump_in_the_counters() {
        let o = original(60);
        let mut take: Vec<usize> = (0..15).collect();
        take.extend(40..55);
        let c = copy_from(&o, &take, &[]);
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 2, "{:#?}", r.segments);
        assert_eq!(r.cuts.len(), 1);
        let cut = r.cuts[0];
        assert!(!cut.goes_backwards());
        // The counters say exactly how much of the original was skipped.
        assert_eq!(
            cut.counters_skipped(),
            o[40].counter as i64 - o[14].counter as i64
        );
    }

    #[test]
    fn a_stretch_moved_elsewhere_shows_as_counters_going_backwards() {
        let o = original(60);
        let mut take: Vec<usize> = (40..55).collect();
        take.extend(0..15);
        let c = copy_from(&o, &take, &[]);
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 2, "{:#?}", r.segments);
        assert_eq!(r.cuts.len(), 1);
        assert!(r.cuts[0].goes_backwards());
    }

    #[test]
    fn a_stretch_used_twice_is_counted() {
        let o = original(40);
        let mut take: Vec<usize> = (0..12).collect();
        take.extend(0..12);
        let c = copy_from(&o, &take, &[]);
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert!(
            r.counters_repeated >= 10,
            "repeated {}",
            r.counters_repeated
        );
        assert_eq!(r.cuts.len(), 1, "{:#?}", r.segments);
        assert!(r.cuts[0].goes_backwards());
    }

    #[test]
    fn material_from_elsewhere_reads_as_no_declaration_and_confirms_nothing() {
        let o = original(40);
        let mut c = copy_from(&o, &(0..12).collect::<Vec<_>>(), &[]);
        // Eight frames with no band at all, spliced in the middle.
        for j in 0..8u64 {
            c.push(Declared {
                copy_index: 96 + j * 8,
                copy_t_us: (12 + j as i64) * 8 * 33_333,
                counter: None,
                tag: None,
                fp: fp(5_000 + j),
            });
        }
        for (n, k) in (12..24usize).enumerate() {
            c.push(Declared {
                copy_index: 160 + n as u64 * 8,
                copy_t_us: (20 + n as i64) * 8 * 33_333,
                counter: Some(o[k].counter),
                tag: None,
                fp: o[k].fp,
            });
        }
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert_eq!(r.segments.len(), 2, "{:#?}", r.segments);
        assert_eq!(r.unconfirmed.len(), 1);
        assert_eq!(r.unconfirmed[0].frames_examined, 8);
        assert_eq!(r.unconfirmed[0].frames_declaring, 0);
        // Bounded by its own first and last frame — 12·8 and 19·8 frame
        // periods in — not by the shots either side. Bounded by the shots, a
        // click on the stretch's start landed on the last GENUINE frame of
        // the shot before, and the reader saw two identical pictures where
        // the report had promised foreign ones.
        assert_eq!(r.unconfirmed[0].copy_start_us, 12 * 8 * 33_333);
        assert_eq!(r.unconfirmed[0].copy_end_us, 19 * 8 * 33_333);
    }

    #[test]
    fn another_recordings_frames_are_foreign_not_contradicted() {
        // Case 3 on a landscape seed: the inserted second keeps its own
        // strip — recording B's signature and B's frame numbers 1..8. Put to
        // recording A as "frames 1..8", they were "contradicted"; they are
        // not claims about A at all.
        let o = original(40);
        let mut c = copy_from(&o, &(0..12).collect::<Vec<_>>(), &[]);
        for j in 0..8u64 {
            c.push(Declared {
                copy_index: 96 + j * 8,
                copy_t_us: (12 + j as i64) * 8 * 33_333,
                counter: Some(j * 8 + 1), // B's own numbering, overlapping A's
                tag: Some(0x66AF),
                fp: fp(5_000 + j),
            });
        }
        for (n, k) in (12..24usize).enumerate() {
            c.push(Declared {
                copy_index: 160 + n as u64 * 8,
                copy_t_us: (20 + n as i64) * 8 * 33_333,
                counter: Some(o[k].counter),
                tag: Some(0x9A8B),
                fp: o[k].fp,
            });
        }
        let r = build_with(
            &c,
            &o,
            Tuning {
                expected_tag: Some(0x9A8B),
                ..Tuning::default()
            },
        );
        assert_eq!(r.segments.len(), 2, "{:#?}", r.segments);
        assert_eq!(r.unconfirmed.len(), 1);
        let u = &r.unconfirmed[0];
        assert_eq!(u.frames_foreign, 8);
        assert_eq!(
            u.frames_contradicted, 0,
            "foreign frames must not be contradicted"
        );
        assert_eq!(u.foreign_tags, vec![(0x66AF, 8)]);
        assert_eq!(r.frames_contradicted, 0);
    }

    #[test]
    fn a_forged_band_cannot_assert_a_correspondence() {
        // The property the whole path rests on: a frame may claim to be frame
        // 1528 of the original, and the pictures decide. Forging can prevent a
        // finding; it cannot manufacture one.
        let o = original(40);
        let c = copy_from(
            &o,
            &(0..20).collect::<Vec<_>>(),
            &(0..20).collect::<Vec<_>>(),
        );
        let r = build(&c, &o, CONFIRM_DISTANCE);
        assert!(r.nothing_confirmed(), "{:#?}", r.segments);
        assert_eq!(r.frames_declaring, 20, "every frame did declare");
        assert_eq!(r.frames_confirmed, 0);
        assert_eq!(r.frames_contradicted, 20);
        assert_eq!(r.unconfirmed[0].frames_contradicted, 20);
    }

    #[test]
    fn a_counter_the_original_does_not_have_is_named_as_such() {
        let o = original(10);
        let c = vec![
            Declared {
                copy_index: 0,
                copy_t_us: 0,
                counter: Some(99_999),
                tag: None,
                fp: fp(1),
            },
            Declared {
                copy_index: 8,
                copy_t_us: 266_664,
                counter: Some(99_999),
                tag: None,
                fp: fp(2),
            },
        ];
        let (v, _) = verify(&c, &o, CONFIRM_DISTANCE);
        // The nearest counter is far away, so the pictures cannot match; what
        // matters is that it is reported and confirms nothing.
        assert!(v.iter().all(|x| *x != FrameVerdict::Confirmed));
    }

    #[test]
    fn an_empty_original_confirms_nothing_rather_than_panicking() {
        let c = copy_from(&original(5), &(0..5).collect::<Vec<_>>(), &[]);
        let r = build(&c, &[], CONFIRM_DISTANCE);
        assert!(r.nothing_confirmed());
        assert_eq!(r.frames_confirmed, 0);
    }
}
