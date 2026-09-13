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
