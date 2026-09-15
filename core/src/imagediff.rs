//! Where two pictures differ, and whether that difference is localised.
//!
//! The fingerprint in `fingerprint.rs` answers "is this the same picture?" in
//! 63 bits. It will not move for a retouch of a few dozen pixels, and it says
//! nothing about **where**. This module answers the second question, and it is
//! the whole of milestone 3.
//!
//! ## Three states, and why the middle one is the point
//!
//! A copy that has merely been recompressed differs from the original
//! **everywhere, a little**. A copy with a patch pasted into it differs
//! **in one place, a lot**. Those are different shapes of the same
//! measurement, and telling them apart is the entire job:
//!
//!   * `ConsistentWithRecompression` — the difference is spread across the
//!     frame with no place standing out;
//!   * `LocalizedDifference` — one or more compact regions stand out against
//!     the frame's own noise, and the report names their coordinates;
//!   * `Inconclusive` — the measurement cannot support either statement. Not
//!     a failure, and never a hint: a frame too flat to have any structure, or
//!     too degraded for a local claim to mean anything, belongs here and
//!     nowhere else.
//!
//! ## No published threshold
//!
//! Nothing here is compared against a fixed number of grey levels. Each frame
//! is judged against **its own** distribution of tile differences: a tile
//! stands out when it exceeds the frame's median by more than `sensitivity`
//! times that frame's median absolute deviation. A recompressed frame raises
//! its own median, so the bar rises with it, and a forger tuning against a
//! published constant would be tuning against nothing.
//!
//! ## Swapping the measure later
//!
//! [`Measure`] names the statistic. There is one today and the extension point
//! is deliberate: adding a variant, matching on it in [`tile_stats`] and
//! [`tile_distance`], changes nothing else in the pipeline. [`DiffSettings`]
//! is the whole knob set and is meant to reach the user interface — grid,
//! sensitivity, how much of the top to skip — rather than being buried.
//!
//! ## Memory
//!
//! Frames are streamed and never held. What survives a pass is one small grid
//! of per-tile statistics — four bytes a tile, so 1 kB a frame at the default
//! 16×16, about 100 MB for an hour of video a side. The grid is the setting to
//! turn down when that matters.

use crate::frame::LumaFrame;

/// Which statistic the comparison rests on.
///
/// One variant today. Adding another means: a branch in [`tile_stats`], a
/// branch in [`tile_distance`], and nothing else — the states, the outlier
/// test, the regions and the report all stay as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// Per tile: mean, spread, and horizontal and vertical texture. Catches a
    /// flat patch pasted over detail, and detail pasted over a flat area,
    /// which a mean alone would miss in both directions.
    TileStats,
}

/// Everything the caller may set. Meant to be reachable from a user interface.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DiffSettings {
    pub measure: Measure,
    /// Grid across the frame. Tiles are defined in relative coordinates, so a
    /// rescaled copy compares against the original without either being
    /// resampled.
    pub cols: u32,
    pub rows: u32,
    /// How far above the frame's own noise a tile must sit to stand out, in
    /// median-absolute-deviations. Lower finds more and cries wolf more; this
    /// is the dial to expose to the reader.
    pub sensitivity: f32,
    /// Tiles a region must span before it is reported. One tile out of 256 is
    /// as likely to be an encoder artefact as anything else.
    pub min_region_tiles: usize,
    /// How far above ITS OWN SHOT's typical difference a frame must sit before
    /// it is called out, in median-absolute-deviations.
    ///
    /// A separate dial from `sensitivity`, because it answers a separate
    /// question. `sensitivity` asks "does one part of this frame stand out
    /// from the rest of it"; this asks "does this frame stand out from its
    /// neighbours". A change that is even across the frame — a blur, a grade,
    /// a re-render, a frame swapped in from elsewhere — is invisible to the
    /// first and obvious to the second.
    pub frame_sensitivity: f32,
    /// Frames a shot must have before its own distribution means anything.
    /// A run of four frames has no typical value to be unusual against.
    pub min_shot_frames: usize,
    /// Fraction of the frame height to leave out, for the burn-in band.
    ///
    /// The band carries the same content in both files by construction, and
    /// its hard black-on-white edges are the noisiest thing in the picture
    /// under recompression — left in, it is flagged on every heavily
    /// recompressed frame. What it carries is checked elsewhere and by
    /// checksum, so nothing is lost by not measuring it as a picture. The
    /// report says it was skipped.
    pub skip_top_fraction: f32,
}

impl Default for DiffSettings {
    fn default() -> Self {
        DiffSettings {
            measure: Measure::TileStats,
            cols: 16,
            rows: 16,
            // Measured, not chosen. On real material at 720×1280: a 200×200
            // patch pasted over four seconds peaks at 79 deviations; the same
            // recording merely recompressed to 150 kbit/s peaks at 11.4, from
            // blocks the encoder starved while feeding their neighbours. Six
            // sat inside that noise and reported 27 frames of a copy nobody
            // had touched. Twelve sits between the two populations.
            //
            // Two measurements are two measurements. This is a dial — exposed
            // as `--sensitivity` and in the browser — because no single number
            // is right for every camera, codec and bit rate, and the residue
            // either side of it is reported rather than hidden.
            sensitivity: 12.0,
            // Measured on a real recording with one frame blurred: its
            // neighbours differ from the original by 3.6 and 6.0 grey levels
            // on average, the blurred one by 19.7. Eight deviations sits well
            // clear of the frame-to-frame jitter of an ordinary re-encode and
            // well below that gap. Like every other number here it is a dial,
            // not a constant of nature.
            frame_sensitivity: 8.0,
            min_shot_frames: 24,
            min_region_tiles: 2,
            // The band is 98 px on a 1280-tall frame; a tenth covers it in
            // both orientations with room to spare.
            skip_top_fraction: 0.10,
        }
    }
}

/// Per-tile statistics for one frame, quantised to a byte each.
///
/// A byte is enough: the outlier test is robust and works on ranks, not on
/// fine differences, and four bytes a tile is what keeps a pass over an hour
/// of video affordable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileGrid {
    pub cols: u32,
    pub rows: u32,
    /// `[mean, spread, texture_x, texture_y]` per tile, row-major.
    pub stats: Vec<[u8; 4]>,
}

impl TileGrid {
    pub fn tile(&self, col: u32, row: u32) -> [u8; 4] {
        self.stats[(row * self.cols + col) as usize]
    }
}

/// Reduce one frame to its grid. This is the only thing kept per frame.
pub fn tile_stats(frame: &LumaFrame, s: &DiffSettings) -> TileGrid {
    let cols = s.cols.max(1);
    let rows = s.rows.max(1);
    let top = ((frame.height as f32) * s.skip_top_fraction.clamp(0.0, 0.9)) as u32;
    let usable = frame.height.saturating_sub(top).max(1);
    let mut stats = Vec::with_capacity((cols * rows) as usize);

    for r in 0..rows {
        let y0 = top + r * usable / rows;
        let y1 = (top + (r + 1) * usable / rows).min(frame.height);
        for c in 0..cols {
            let x0 = c * frame.width / cols;
            let x1 = ((c + 1) * frame.width / cols).min(frame.width);
            stats.push(match s.measure {
                Measure::TileStats => stats_of(frame, x0, y0, x1, y1),
            });
        }
    }
    TileGrid { cols, rows, stats }
}

fn stats_of(frame: &LumaFrame, x0: u32, y0: u32, x1: u32, y1: u32) -> [u8; 4] {
    if x1 <= x0 || y1 <= y0 {
        return [0, 0, 0, 0];
    }
    let n = ((x1 - x0) as u64 * (y1 - y0) as u64).max(1);
    let mut sum = 0u64;
    let mut sq = 0u64;
    let mut gx = 0u64;
    let mut gy = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let v = frame.at(x, y) as u64;
            sum += v;
            sq += v * v;
            if x + 1 < x1 {
                gx += (frame.at(x + 1, y) as i32 - v as i32).unsigned_abs() as u64;
            }
            if y + 1 < y1 {
                gy += (frame.at(x, y + 1) as i32 - v as i32).unsigned_abs() as u64;
            }
        }
    }
    let mean = (sum / n) as f64;
    let var = (sq as f64 / n as f64) - mean * mean;
    // Texture is doubled before clamping: real detail rarely averages above
    // 128 grey levels of gradient, and the low end is where the signal is.
    let clamp = |v: f64| v.clamp(0.0, 255.0) as u8;
    [
        clamp(mean),
        clamp(var.max(0.0).sqrt()),
        clamp((gx as f64 / n as f64) * 2.0),
        clamp((gy as f64 / n as f64) * 2.0),
    ]
}

fn tile_distance(a: [u8; 4], b: [u8; 4], measure: Measure) -> f32 {
    match measure {
        Measure::TileStats => (0..4)
            .map(|i| (a[i] as i32 - b[i] as i32).unsigned_abs() as f32)
            .sum(),
    }
}

/// A rectangle that stands out, in fractions of the frame.
///
/// Relative rather than in pixels because the copy may have been rescaled, and
/// a coordinate that only means something at one resolution is a coordinate
/// the reader cannot use.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub tiles: usize,
    /// How far the worst tile in it sat above the frame's own noise, in
    /// deviations. Reported so the reader can see the margin rather than take
    /// the word "differs" on trust.
    pub deviations: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DifferenceState {
    /// Spread across the frame with nothing standing out.
    ConsistentWithRecompression,
    /// One or more compact regions stand out against the frame's own noise.
    LocalizedDifference,
    /// Neither statement is supportable here.
    Inconclusive,
}

/// Why a frame could not be judged. An enum rather than a message so the
/// reason survives the JSON report and can be counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unsupportable {
    /// The two frames were not reduced to the same grid.
    GridMismatch,
    /// Most of the frame carries nothing to compare.
    NoStructure,
    /// Everything moved, so singling out a part would be arbitrary.
    WholeFrameMoved,
}

impl Unsupportable {
    pub fn explain(self) -> &'static str {
        match self {
            Unsupportable::GridMismatch => "the two frames were not reduced to the same grid",
            Unsupportable::NoStructure => {
                "most of the frame carries no structure to compare — a flat or blown-out picture"
            }
            Unsupportable::WholeFrameMoved => {
                "the whole frame differs so much that no part of it can be singled out"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameDifference {
    pub state: DifferenceState,
    pub regions: Vec<Region>,
    /// The frame's own median tile difference — its noise floor.
    pub baseline: f32,
    /// Median absolute deviation of the tile differences.
    pub spread: f32,
    /// Why, when the state is `Inconclusive`.
    pub inconclusive_because: Option<Unsupportable>,
}

/// One frame of the copy placed against the original's frame of the same
/// number, and what the comparison found.
///
/// Lives here rather than in the caller so the native binary, the browser and
/// the report all name the same thing.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Located {
    pub copy_index: u64,
    pub copy_t_us: i64,
    pub original_t_us: i64,
    pub counter: u64,
    pub difference: FrameDifference,
}

/// One frame of the original, reduced, keyed by the counter its strip carried.
pub struct OriginalGrid {
    pub counter: u64,
    pub t_us: i64,
    pub grid: TileGrid,
}

/// One frame of the copy, reduced.
pub struct CopyGrid {
    pub index: u64,
    pub t_us: i64,
    pub counter: Option<u64>,
    pub grid: TileGrid,
}

/// Run the localised comparison over the frames the correspondence CONFIRMED.
///
/// One implementation for the native binary and the browser both. The
/// selection rule is the whole reason this is not just a loop: a frame the
/// fingerprint contradicted is a different picture, and pointing at a
/// rectangle inside it would dress up "this is not the same shot" as "this
/// corner was retouched". A frame it could not settle is not eligible either —
/// there is no established correspondence to measure a departure from.
pub fn locate(
    correspondence: &crate::declared::DeclaredCorrespondence,
    original: &[OriginalGrid],
    copy: &[CopyGrid],
    s: &DiffSettings,
) -> Vec<Located> {
    use std::collections::{HashMap, HashSet};
    let by_counter: HashMap<u64, &OriginalGrid> = original.iter().map(|o| (o.counter, o)).collect();

    let mut eligible: HashSet<u64> = HashSet::new();
    for seg in &correspondence.segments {
        let excluded: HashSet<u64> = seg
            .differing
            .iter()
            .chain(seg.inconclusive.iter())
            .map(|n| n.copy_index)
            .collect();
        for c in copy {
            if c.t_us >= seg.copy_start_us
                && c.t_us <= seg.copy_end_us
                && !excluded.contains(&c.index)
            {
                eligible.insert(c.index);
            }
        }
    }

    copy.iter()
        .filter(|c| eligible.contains(&c.index))
        .filter_map(|c| {
            let counter = c.counter?;
            let o = by_counter.get(&counter)?;
            Some(Located {
                copy_index: c.index,
                copy_t_us: c.t_us,
                original_t_us: o.t_us,
                counter,
                difference: compare(&o.grid, &c.grid, s),
            })
        })
        .collect()
}

/// The same region, holding still across consecutive frames.
///
/// This is the discriminator that per-frame outliers alone cannot give. At
/// 150 kbit/s a codec starves some blocks and not others, and the starved ones
/// stand out against the frame's own noise exactly the way an edit does — 27
/// frames of a merely recompressed copy came back with located regions. What
/// separates them is **time**: a retouch sits in the same place for as long as
/// it is there, and encoder blocking moves every frame.
///
/// Nothing is suppressed by this. A one-frame track is still reported; it is
/// reported AS a one-frame track, next to the four-second one, so a reader can
/// tell a patch from confetti instead of being handed a single count that
/// mixes them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub frames: usize,
    pub first_copy_t_us: i64,
    pub last_copy_t_us: i64,
    pub first_counter: u64,
    pub last_counter: u64,
    /// Union of the boxes it was seen in, with the peak deviation over them.
    pub region: Region,
}

impl Track {
    pub fn duration_us(&self) -> i64 {
        self.last_copy_t_us - self.first_copy_t_us
    }
}

fn overlaps(a: &Region, b: &Region) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Group located regions that overlap in consecutive frames.
///
/// `gap` is how many eligible frames a track may skip and still be the same
/// track: a region right at the threshold flickers, and cutting it into
/// fifteen one-frame tracks would say the opposite of what is happening.
pub fn tracks(located: &[Located], gap: usize) -> Vec<Track> {
    struct Open {
        t: Track,
        last_seen: usize,
    }
    let mut open: Vec<Open> = Vec::new();
    let mut done: Vec<Track> = Vec::new();

    for (step, l) in located.iter().enumerate() {
        for r in &l.difference.regions {
            let hit = open
                .iter_mut()
                .find(|o| step.saturating_sub(o.last_seen) <= gap + 1 && overlaps(&o.t.region, r));
            match hit {
                Some(o) => {
                    // Union rather than intersection: the reader is being
                    // pointed at an area to go and look at, and a box that
                    // covers everywhere the difference was seen is the one
                    // that will contain it.
                    let u = &mut o.t.region;
                    let nx = u.x.min(r.x);
                    let ny = u.y.min(r.y);
                    u.width = (u.x + u.width).max(r.x + r.width) - nx;
                    u.height = (u.y + u.height).max(r.y + r.height) - ny;
                    u.x = nx;
                    u.y = ny;
                    u.tiles = u.tiles.max(r.tiles);
                    u.deviations = u.deviations.max(r.deviations);
                    o.t.frames += 1;
                    o.t.last_copy_t_us = l.copy_t_us;
                    o.t.last_counter = l.counter;
                    o.last_seen = step;
                }
                None => open.push(Open {
                    t: Track {
                        frames: 1,
                        first_copy_t_us: l.copy_t_us,
                        last_copy_t_us: l.copy_t_us,
                        first_counter: l.counter,
                        last_counter: l.counter,
                        region: *r,
                    },
                    last_seen: step,
                }),
            }
        }
        // Retire tracks that have gone quiet, so a later region somewhere else
        // does not join a track it has nothing to do with.
        let mut still = Vec::new();
        for o in open.into_iter() {
            if step.saturating_sub(o.last_seen) > gap + 1 {
                done.push(o.t);
            } else {
                still.push(o);
            }
        }
        open = still;
    }
    done.extend(open.into_iter().map(|o| o.t));
    done.sort_by(|a, b| {
        b.frames.cmp(&a.frames).then(
            b.region
                .deviations
                .partial_cmp(&a.region.deviations)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    done
}

/// One frame that does not sit with its neighbours.
///
/// Not "modified" — the report says what was measured and lets the reader
/// look. What is measured is that this frame departs from the original far
/// more than the frames around it do, in a shot where the others agree.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutOfPlace {
    pub copy_index: u64,
    pub copy_t_us: i64,
    pub original_t_us: i64,
    pub counter: u64,
    /// This frame's own median tile difference.
    pub baseline: f32,
    /// What the rest of its shot sits at.
    pub shot_baseline: f32,
    /// How far above that, in the shot's own deviations.
    pub deviations: f32,
}

/// Frames whose difference from the original is unusual **for their own shot**.
///
/// The gap this closes. Everything else here compares a frame with the
/// original's frame of the same number and asks whether the difference is
/// spread out or concentrated. A frame that was blurred, graded, re-rendered
/// or swapped in from another source differs EVENLY — so it reads as
/// "consistent with recompression", which is exactly what a recompression
/// reads as, and the frame vanishes into the conforming count. Measured on a
/// real recording: one blurred frame lost 85 % of its texture and the report
/// called it conforming, alongside 561 others.
///
/// What that comparison never asks is whether the amount of difference is
/// normal for this film. Its neighbours differed by 3.6 and 6.0 grey levels;
/// it differed by 19.7. Every frame carries such a number already — this puts
/// them side by side.
///
/// Judged per shot and against the shot's own spread, so nothing is compared
/// to a published constant: a heavily recompressed film raises the bar for
/// itself, a clean one lowers it.
///
/// **What it cannot see.** A change applied to the whole film. If every frame
/// is blurred, every frame's neighbours are blurred too, nothing stands out,
/// and this says nothing — the shot's "worst confirmed" figure and the
/// reader's own eyes are what remain.
pub fn out_of_place(
    correspondence: &crate::declared::DeclaredCorrespondence,
    original: &[OriginalGrid],
    copy: &[CopyGrid],
    s: &DiffSettings,
) -> Vec<OutOfPlace> {
    use std::collections::HashMap;
    let by_counter: HashMap<u64, &OriginalGrid> = original.iter().map(|o| (o.counter, o)).collect();

    let mut out = Vec::new();
    for seg in &correspondence.segments {
        // Every frame of the shot that can be measured at all, including the
        // ones the fingerprint could not settle.
        //
        // Those are the important ones and they were being dropped: a blur
        // moves the fingerprint into the unsettled band, `locate` skips
        // unsettled frames because asking WHERE two unmatched pictures differ
        // has no answer — and this check, which exists to catch exactly that
        // frame, inherited the exclusion. On one of two test recordings the
        // blurred frame was invisible for that reason alone.
        //
        // Asking HOW MUCH a frame differs is a different question from asking
        // where, and it has an answer whatever the fingerprint decided.
        let mut mine: Vec<(&CopyGrid, &OriginalGrid, f32)> = Vec::new();
        for c in copy {
            if c.t_us < seg.copy_start_us || c.t_us > seg.copy_end_us {
                continue;
            }
            let Some(counter) = c.counter else { continue };
            let Some(o) = by_counter.get(&counter) else {
                continue;
            };
            let d = compare(&o.grid, &c.grid, s);
            // A frame with nothing to compare — flat, blown out — has no
            // baseline worth the name and must not drag the shot's down.
            if d.inconclusive_because == Some(Unsupportable::NoStructure) {
                continue;
            }
            mine.push((c, o, d.baseline));
        }
        if mine.len() < s.min_shot_frames {
            continue;
        }

        let mut b: Vec<f32> = mine.iter().map(|(_, _, x)| *x).collect();
        let typical = median(&mut b);
        let mut devs: Vec<f32> = b.iter().map(|x| (x - typical).abs()).collect();
        // A floor, or a shot whose frames all differ by exactly the same
        // amount would call its own rounding an anomaly.
        let spread = median(&mut devs).max(1.5);
        let bar = typical + s.frame_sensitivity.max(1.0) * spread;

        for (c, o, baseline) in mine {
            if baseline > bar {
                out.push(OutOfPlace {
                    copy_index: c.index,
                    copy_t_us: c.t_us,
                    original_t_us: o.t_us,
                    counter: c.counter.unwrap_or(0),
                    baseline,
                    shot_baseline: typical,
                    deviations: (baseline - typical) / spread,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        b.deviations
            .partial_cmp(&a.deviations)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Frames out of place that sit together in time.
///
/// The shape of what was found carries as much as the count. An encoder
/// cannot degrade one frame and spare its neighbours — its rate control
/// settles over a RUN of frames, typically at the start of a file. A frame
/// that was blurred, graded or swapped in is by nature a single-frame event.
///
/// Nothing is suppressed by this grouping. A blurred passage is a run too,
/// and a real finding; the report shows the shape and the reader judges.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutOfPlaceRun {
    pub frames: usize,
    pub first_copy_t_us: i64,
    pub last_copy_t_us: i64,
    pub first_counter: u64,
    pub last_counter: u64,
    /// Where the first of them sits in the original.
    pub original_t_us: i64,
    pub peak_baseline: f32,
    pub shot_baseline: f32,
    pub peak_deviations: f32,
    /// Whether it begins in the opening second of the supplied file — where a
    /// codec's rate control is still settling. Stated as a fact for the reader
    /// to weigh, not used to drop anything.
    pub at_file_start: bool,
}

impl OutOfPlaceRun {
    pub fn duration_us(&self) -> i64 {
        self.last_copy_t_us - self.first_copy_t_us
    }
    pub fn is_single_frame(&self) -> bool {
        self.frames == 1
    }
}

/// How far apart two out-of-place frames may sit and still be one run. Half a
/// second: a codec settling produces a burst inside that, and two genuinely
/// separate events are unlikely to fall inside it.
const RUN_GAP_US: i64 = 500_000;

/// Frames within this of the file's start are in the opening, where rate
/// control is still finding its level.
const FILE_START_US: i64 = 1_000_000;

pub fn out_of_place_runs(items: &[OutOfPlace]) -> Vec<OutOfPlaceRun> {
    let mut sorted: Vec<&OutOfPlace> = items.iter().collect();
    sorted.sort_by_key(|o| o.copy_t_us);

    let mut out: Vec<OutOfPlaceRun> = Vec::new();
    for o in sorted {
        match out.last_mut() {
            Some(run) if o.copy_t_us - run.last_copy_t_us <= RUN_GAP_US => {
                run.frames += 1;
                run.last_copy_t_us = o.copy_t_us;
                run.last_counter = o.counter;
                if o.deviations > run.peak_deviations {
                    run.peak_deviations = o.deviations;
                    run.peak_baseline = o.baseline;
                }
            }
            _ => out.push(OutOfPlaceRun {
                frames: 1,
                first_copy_t_us: o.copy_t_us,
                last_copy_t_us: o.copy_t_us,
                first_counter: o.counter,
                last_counter: o.counter,
                original_t_us: o.original_t_us,
                peak_baseline: o.baseline,
                shot_baseline: o.shot_baseline,
                peak_deviations: o.deviations,
                at_file_start: o.copy_t_us <= FILE_START_US,
            }),
        }
    }
    // Single frames first: they are the shape an alteration takes, and a
    // reader with limited time should meet them before a codec's warm-up.
    out.sort_by(|a, b| {
        a.is_single_frame()
            .cmp(&b.is_single_frame())
            .reverse()
            .then(
                b.peak_deviations
                    .partial_cmp(&a.peak_deviations)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    out
}

/// Below this, a tile has no structure in either frame and comparing it says
/// nothing. A lens cap, a wall, an over-exposed sky, or the grey table a phone
/// was left face-down on all land here.
const FLAT_TEXTURE: u8 = 6;

/// Above this median tile difference the whole frame has moved so far that
/// calling any one part of it "different" would be arbitrary.
const TOO_FAR_GONE: f32 = 90.0;

/// Compare two grids of the same shape.
pub fn compare(original: &TileGrid, copy: &TileGrid, s: &DiffSettings) -> FrameDifference {
    let inconclusive = |why: Unsupportable| FrameDifference {
        state: DifferenceState::Inconclusive,
        regions: Vec::new(),
        baseline: 0.0,
        spread: 0.0,
        inconclusive_because: Some(why),
    };
    if original.cols != copy.cols || original.rows != copy.rows || original.stats.is_empty() {
        return inconclusive(Unsupportable::GridMismatch);
    }

    // Tiles with no structure on either side are set aside before anything is
    // measured. They are not evidence of sameness — nothing was compared.
    let judged: Vec<usize> = (0..original.stats.len())
        .filter(|&i| {
            let (a, b) = (original.stats[i], copy.stats[i]);
            a[2].max(a[3]) >= FLAT_TEXTURE || b[2].max(b[3]) >= FLAT_TEXTURE
        })
        .collect();
    if judged.len() * 4 < original.stats.len() {
        return inconclusive(Unsupportable::NoStructure);
    }

    let mut d = vec![0.0f32; original.stats.len()];
    for &i in &judged {
        d[i] = tile_distance(original.stats[i], copy.stats[i], s.measure);
    }
    let mut sorted: Vec<f32> = judged.iter().map(|&i| d[i]).collect();
    let baseline = median(&mut sorted);
    if baseline > TOO_FAR_GONE {
        return FrameDifference {
            state: DifferenceState::Inconclusive,
            regions: Vec::new(),
            baseline,
            spread: 0.0,
            inconclusive_because: Some(Unsupportable::WholeFrameMoved),
        };
    }
    let mut devs: Vec<f32> = judged.iter().map(|&i| (d[i] - baseline).abs()).collect();
    // A floor on the spread, or a frame that happens to be nearly identical
    // everywhere would call its own quantisation noise an outlier.
    let spread = median(&mut devs).max(3.0);
    let bar = baseline + s.sensitivity.max(0.5) * spread;

    let standout: Vec<bool> = (0..d.len())
        .map(|i| judged.binary_search(&i).is_ok() && d[i] > bar)
        .collect();
    let regions = group(
        &standout,
        original.cols,
        original.rows,
        &d,
        baseline,
        spread,
        s,
    );

    FrameDifference {
        state: if regions.is_empty() {
            DifferenceState::ConsistentWithRecompression
        } else {
            DifferenceState::LocalizedDifference
        },
        regions,
        baseline,
        spread,
        inconclusive_because: None,
    }
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Flood-fill the standing-out tiles into connected regions.
#[allow(clippy::too_many_arguments)]
fn group(
    standout: &[bool],
    cols: u32,
    rows: u32,
    d: &[f32],
    baseline: f32,
    spread: f32,
    s: &DiffSettings,
) -> Vec<Region> {
    let mut seen = vec![false; standout.len()];
    let mut out = Vec::new();
    let idx = |c: u32, r: u32| (r * cols + c) as usize;

    for r0 in 0..rows {
        for c0 in 0..cols {
            let start = idx(c0, r0);
            if seen[start] || !standout[start] {
                continue;
            }
            let (mut minc, mut maxc, mut minr, mut maxr) = (c0, c0, r0, r0);
            let mut worst = 0.0f32;
            let mut count = 0usize;
            let mut stack = vec![(c0, r0)];
            seen[start] = true;
            while let Some((c, r)) = stack.pop() {
                count += 1;
                minc = minc.min(c);
                maxc = maxc.max(c);
                minr = minr.min(r);
                maxr = maxr.max(r);
                worst = worst.max((d[idx(c, r)] - baseline) / spread);
                let push = |c: u32, r: u32, stack: &mut Vec<(u32, u32)>, seen: &mut Vec<bool>| {
                    let i = idx(c, r);
                    if !seen[i] && standout[i] {
                        seen[i] = true;
                        stack.push((c, r));
                    }
                };
                if c > 0 {
                    push(c - 1, r, &mut stack, &mut seen);
                }
                if c + 1 < cols {
                    push(c + 1, r, &mut stack, &mut seen);
                }
                if r > 0 {
                    push(c, r - 1, &mut stack, &mut seen);
                }
                if r + 1 < rows {
                    push(c, r + 1, &mut stack, &mut seen);
                }
            }
            if count < s.min_region_tiles {
                continue;
            }
            // Back into fractions of the frame, including the skipped band so
            // the rectangle lands where the reader will look.
            let usable = 1.0 - s.skip_top_fraction.clamp(0.0, 0.9);
            out.push(Region {
                x: minc as f32 / cols as f32,
                y: s.skip_top_fraction + (minr as f32 / rows as f32) * usable,
                width: (maxc - minc + 1) as f32 / cols as f32,
                height: ((maxr - minr + 1) as f32 / rows as f32) * usable,
                tiles: count,
                deviations: worst,
            });
        }
    }
    out.sort_by(|a, b| {
        b.deviations
            .partial_cmp(&a.deviations)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A textured frame: diagonal stripes, so every tile has structure.
    fn textured(w: u32, h: u32, phase: u32) -> LumaFrame {
        let mut data = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                data[(y * w + x) as usize] = if ((x + y + phase) / 4) % 2 == 0 {
                    40
                } else {
                    200
                };
            }
        }
        LumaFrame::new(w, h, data, 0, 0).unwrap()
    }

    fn flat(w: u32, h: u32, v: u8) -> LumaFrame {
        LumaFrame::new(w, h, vec![v; (w * h) as usize], 0, 0).unwrap()
    }

    /// Paste a flat patch over a rectangle, in fractions of the frame.
    fn patch(f: &LumaFrame, x: f32, y: f32, w: f32, h: f32, v: u8) -> LumaFrame {
        let mut data = f.data.clone();
        let (x0, y0) = ((x * f.width as f32) as u32, (y * f.height as f32) as u32);
        let (x1, y1) = (
            x0 + (w * f.width as f32) as u32,
            y0 + (h * f.height as f32) as u32,
        );
        for yy in y0..y1.min(f.height) {
            for xx in x0..x1.min(f.width) {
                data[(yy * f.width + xx) as usize] = v;
            }
        }
        LumaFrame::new(f.width, f.height, data, 0, 0).unwrap()
    }

    /// Texture gone, means kept — what a blur does, and what every other
    /// check here mistakes for a recompression.
    fn blur(f: &LumaFrame) -> LumaFrame {
        let (w, h) = (f.width, f.height);
        let mut data = vec![0u8; (w * h) as usize];
        let r = 6i32;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (mut sum, mut n) = (0u32, 0u32);
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (px, py) = (x + dx, y + dy);
                        if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                            sum += f.at(px as u32, py as u32) as u32;
                            n += 1;
                        }
                    }
                }
                data[(y as u32 * w + x as u32) as usize] = (sum / n.max(1)) as u8;
            }
        }
        LumaFrame::new(w, h, data, 0, 0).unwrap()
    }

    /// Everywhere-a-little, the way a codec degrades a picture.
    fn noisy(f: &LumaFrame, amount: i32) -> LumaFrame {
        let mut data = f.data.clone();
        let mut h = 0x2545_F491_4F6C_DD1Du64;
        for v in data.iter_mut() {
            h ^= h << 13;
            h ^= h >> 7;
            h ^= h << 17;
            let n = (h % (2 * amount as u64 + 1)) as i32 - amount;
            *v = (*v as i32 + n).clamp(0, 255) as u8;
        }
        LumaFrame::new(f.width, f.height, data, 0, 0).unwrap()
    }

    fn run(a: &LumaFrame, b: &LumaFrame) -> FrameDifference {
        let s = DiffSettings::default();
        compare(&tile_stats(a, &s), &tile_stats(b, &s), &s)
    }

    #[test]
    fn an_identical_copy_stands_out_nowhere() {
        let f = textured(320, 320, 0);
        let r = run(&f, &f);
        assert_eq!(r.state, DifferenceState::ConsistentWithRecompression);
        assert!(r.regions.is_empty());
    }

    #[test]
    fn noise_everywhere_is_not_a_localized_difference() {
        // This is the case that must never produce an accusation: a copy that
        // has only been through a codec.
        let f = textured(320, 320, 0);
        let r = run(&f, &noisy(&f, 24));
        assert_eq!(
            r.state,
            DifferenceState::ConsistentWithRecompression,
            "regions: {:?}",
            r.regions
        );
    }

    #[test]
    fn a_pasted_patch_is_found_and_located() {
        let f = textured(320, 320, 0);
        let edited = patch(&f, 0.25, 0.5, 0.25, 0.25, 128);
        let r = run(&f, &edited);
        assert_eq!(r.state, DifferenceState::LocalizedDifference);
        let big = &r.regions[0];
        assert!(big.x >= 0.15 && big.x <= 0.35, "x {}", big.x);
        assert!(big.y >= 0.40 && big.y <= 0.60, "y {}", big.y);
        assert!(big.deviations > 1.0);
    }

    #[test]
    fn a_patch_survives_the_noise_a_codec_adds() {
        let f = textured(320, 320, 0);
        let edited = noisy(&patch(&f, 0.5, 0.25, 0.25, 0.25, 128), 16);
        let r = run(&f, &edited);
        assert_eq!(r.state, DifferenceState::LocalizedDifference, "{r:?}");
    }

    #[test]
    fn a_frame_with_no_structure_is_inconclusive_not_clean() {
        // The phone left face-down on a table. Nothing was compared, and
        // saying "consistent" here would be claiming a check that never ran.
        let a = flat(320, 320, 120);
        let b = flat(320, 320, 130);
        let r = run(&a, &b);
        assert_eq!(r.state, DifferenceState::Inconclusive);
        assert_eq!(r.inconclusive_because, Some(Unsupportable::NoStructure));
    }

    #[test]
    fn two_unrelated_pictures_are_inconclusive_not_localized() {
        // Everything differs, so singling out a part of it would be arbitrary.
        // The correspondence path has already said these are different
        // pictures; this module must not dress that up as a located finding.
        let a = textured(320, 320, 0);
        let b = noisy(&flat(320, 320, 200), 90);
        let r = run(&a, &b);
        assert_eq!(r.state, DifferenceState::Inconclusive, "{r:?}");
    }

    #[test]
    fn the_burn_in_band_is_left_out_of_the_picture_measurement() {
        // A difference confined to the top tenth must not be reported: that is
        // where the band lives, its content is checked elsewhere, and its hard
        // edges are the noisiest thing under recompression.
        let f = textured(320, 320, 0);
        let banded = patch(&f, 0.0, 0.0, 1.0, 0.08, 255);
        let r = run(&f, &banded);
        assert_eq!(
            r.state,
            DifferenceState::ConsistentWithRecompression,
            "{r:?}"
        );
    }

    #[test]
    fn sensitivity_is_a_dial_and_lowering_it_finds_more() {
        let f = textured(320, 320, 0);
        let edited = patch(&f, 0.3, 0.3, 0.08, 0.08, 90);
        let strict = DiffSettings {
            sensitivity: 40.0,
            ..Default::default()
        };
        let loose = DiffSettings {
            sensitivity: 2.0,
            ..Default::default()
        };
        let (a, b) = (tile_stats(&f, &strict), tile_stats(&edited, &strict));
        assert!(compare(&a, &b, &strict).regions.len() <= compare(&a, &b, &loose).regions.len());
    }

    #[test]
    fn a_region_that_holds_still_becomes_one_track() {
        let f = textured(320, 320, 0);
        let s = DiffSettings::default();
        let og = tile_stats(&f, &s);
        let located: Vec<Located> = (0..10)
            .map(|i| Located {
                copy_index: i,
                copy_t_us: i as i64 * 33_333,
                original_t_us: i as i64 * 33_333,
                counter: i + 1,
                difference: compare(
                    &og,
                    &tile_stats(&patch(&f, 0.25, 0.5, 0.25, 0.25, 128), &s),
                    &s,
                ),
            })
            .collect();
        let t = tracks(&located, 1);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!(t[0].frames, 10);
        assert_eq!(t[0].first_counter, 1);
        assert_eq!(t[0].last_counter, 10);
    }

    #[test]
    fn regions_that_jump_around_stay_separate_tracks() {
        // What encoder blocking looks like: a different corner each frame.
        let f = textured(320, 320, 0);
        let s = DiffSettings::default();
        let og = tile_stats(&f, &s);
        let spots = [(0.05, 0.15), (0.70, 0.80), (0.10, 0.75), (0.75, 0.20)];
        let located: Vec<Located> = spots
            .iter()
            .enumerate()
            .map(|(i, (x, y))| Located {
                copy_index: i as u64,
                copy_t_us: i as i64 * 33_333,
                original_t_us: i as i64 * 33_333,
                counter: i as u64 + 1,
                difference: compare(&og, &tile_stats(&patch(&f, *x, *y, 0.2, 0.2, 128), &s), &s),
            })
            .collect();
        let t = tracks(&located, 1);
        assert!(t.len() >= 3, "scattered regions were merged: {t:?}");
        assert!(t.iter().all(|k| k.frames <= 2));
    }

    /// A shot of `n` frames, one of which is blurred — the attack the
    /// per-frame comparison cannot see, because the change is even.
    fn shot_with_one_blurred(n: usize, at: usize) -> (Vec<OriginalGrid>, Vec<CopyGrid>) {
        let s = DiffSettings::default();
        let mut orig = Vec::new();
        let mut copy = Vec::new();
        for i in 0..n {
            let f = textured(320, 320, (i % 4) as u32);
            // A blur is a loss of texture everywhere, not a patch somewhere.
            let c = if i == at { blur(&f) } else { noisy(&f, 6) };
            orig.push(OriginalGrid {
                counter: i as u64 + 1,
                t_us: i as i64 * 33_333,
                grid: tile_stats(&f, &s),
            });
            copy.push(CopyGrid {
                index: i as u64,
                t_us: i as i64 * 33_333,
                counter: Some(i as u64 + 1),
                grid: tile_stats(&c, &s),
            });
        }
        (orig, copy)
    }

    fn one_shot(n: usize) -> crate::declared::DeclaredCorrespondence {
        let mut r = crate::declared::build_with(&[], &[], crate::declared::Tuning::default());
        r.segments = vec![crate::declared::DeclaredSegment {
            copy_start_us: 0,
            copy_end_us: n as i64 * 33_333,
            counter_start: 1,
            counter_end: n as u64,
            original_start_us: 0,
            original_end_us: n as i64 * 33_333,
            frames_examined: n,
            frames_confirmed: n,
            differing: vec![],
            inconclusive: vec![],
            worst_confirmed_distance: 2,
        }];
        r
    }

    #[test]
    fn an_evenly_blurred_frame_is_caught_by_its_neighbours() {
        let s = DiffSettings::default();
        let (o, c) = shot_with_one_blurred(60, 30);
        // The per-frame check cannot see it: the change is everywhere at once.
        let alone = compare(&o[30].grid, &c[30].grid, &s);
        assert_ne!(alone.state, DifferenceState::LocalizedDifference);
        // Its neighbours can.
        let found = out_of_place(&one_shot(60), &o, &c, &s);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].counter, 31);
        assert!(found[0].deviations > s.frame_sensitivity);
    }

    #[test]
    fn an_ordinary_recompression_puts_no_frame_out_of_place() {
        // The case that must never fire: nothing was done to this copy.
        let s = DiffSettings::default();
        let mut orig = Vec::new();
        let mut copy = Vec::new();
        for i in 0..60usize {
            let f = textured(320, 320, (i % 4) as u32);
            orig.push(OriginalGrid {
                counter: i as u64 + 1,
                t_us: i as i64 * 33_333,
                grid: tile_stats(&f, &s),
            });
            copy.push(CopyGrid {
                index: i as u64,
                t_us: i as i64 * 33_333,
                counter: Some(i as u64 + 1),
                grid: tile_stats(&noisy(&f, 20), &s),
            });
        }
        assert!(out_of_place(&one_shot(60), &orig, &copy, &s).is_empty());
    }

    #[test]
    fn a_lone_frame_and_a_burst_are_told_apart() {
        // The two shapes measured on real files: one blurred frame alone at
        // 10 s, and three frames inside 0.13 s at the very start, which is
        // x264's rate control settling rather than anything done to the film.
        let mk = |t: i64, dev: f32| OutOfPlace {
            copy_index: (t / 33_333) as u64,
            copy_t_us: t,
            original_t_us: t,
            counter: (t / 33_333) as u64 + 1,
            baseline: 40.0,
            shot_baseline: 2.0,
            deviations: dev,
        };
        let runs = out_of_place_runs(&[
            mk(670_000, 26.0),
            mk(770_000, 16.0),
            mk(800_000, 15.3),
            mk(10_000_000, 26.7),
        ]);
        assert_eq!(runs.len(), 2, "{runs:?}");
        // The lone frame comes first, whatever its peak.
        assert!(runs[0].is_single_frame());
        assert_eq!(runs[0].first_counter, 301);
        assert!(!runs[0].at_file_start);
        assert_eq!(runs[1].frames, 3);
        assert!(runs[1].at_file_start, "the burst is in the opening second");
        assert_eq!(runs[1].duration_us(), 130_000);
    }

    #[test]
    fn a_shot_too_short_to_have_a_habit_is_left_alone() {
        let s = DiffSettings::default();
        let (o, c) = shot_with_one_blurred(8, 4);
        assert!(out_of_place(&one_shot(8), &o, &c, &s).is_empty());
    }

    #[test]
    fn a_rescaled_copy_compares_against_the_original_unresampled() {
        // Tiles are relative, so half-size is not a difference.
        let s = DiffSettings::default();
        let big = textured(320, 320, 0);
        let small = textured(160, 160, 0);
        let r = compare(&tile_stats(&big, &s), &tile_stats(&small, &s), &s);
        assert_ne!(r.state, DifferenceState::LocalizedDifference, "{r:?}");
    }
}
