//! Reading the overlay the phone burns into every frame.
//!
//! ## What is written there, and what it is worth
//!
//! Three right-aligned lines in a band across the top of the picture, drawn
//! identically by both platforms (`FrameCompositor.swift`, `GlRotator.kt`):
//!
//! ```text
//!   2026-06-05T04:53:32Z  f=271
//!   39.86570, 116.38294  ±10m
//!   ƒ/1.8 1/50 ISO64 wide 1.0×
//! ```
//!
//! The second field of line 1 is a **frame counter**, incremented once per
//! composed frame. It is the single most useful thing in the picture for this
//! tool: a copy frame carrying `f=271` is telling us which frame of the
//! original it claims to be, which turns alignment from a search into a check.
//!
//! **None of it is signed.** These are pixels, and pixels can be drawn. Three
//! consequences run through everything below:
//!
//!   * a counter read here is a CLAIM, never a finding. It says where to look
//!     in the original; whether the pictures actually match is a separate
//!     question and the only one that can tie the two videos together;
//!   * the timestamp comes from the device's bare wall clock at the instant
//!     the frame was composed — `Date()` on both platforms. Not NTP, not GPS,
//!     not the notary. It dates nothing. What the recording's time is worth
//!     comes from the `time.v1` clock declarations and the notary chain, both
//!     established by the bundle's own verifier and quoted from it;
//!   * the signed envelope's `phone_time_us` comes from that SAME wall clock,
//!     so agreement between the two is an internal-consistency check — it
//!     catches a band that was re-rendered, and proves nothing about when the
//!     recording happened.
//!
//! ## How it is read
//!
//! The font is monospace and the lines are right-aligned, so the layout is a
//! fixed grid once its pitch and right edge are known. Rather than recognising
//! arbitrary text, this module finds the grid and reads named cells out of it:
//! line 1 is always twenty characters of timestamp, two spaces, `f=`, then the
//! counter's digits.
//!
//! That means only the ten digits need classifying, and even those are not
//! shipped as templates. They are learned from the original itself — see
//! [`DigitBook`] — so nothing here depends on a font file, and a rendering
//! change on either platform is absorbed rather than breaking the reader.

use crate::frame::{LumaFrame, Rect};

/// The overlay geometry, in the pixels the phone draws it at.
///
/// Both platforms use the same numbers, and this module reads them from one
/// place so a divergence shows up as a compile error rather than as a subtly
/// wrong crop. See `FrameCompositor.overlayBandHeight` and
/// `GlRotator.OVERLAY_BAND_HEIGHT`.
pub mod native {
    /// Height of the band, at the top of the frame.
    ///
    /// Ninety-six since the machine-readable strip was added below the three
    /// text lines — see `crate::bitrow`, which owns the strip's own geometry.
    /// The text baselines did not move.
    pub const BAND_HEIGHT: u32 = crate::bitrow::native::BAND_HEIGHT;
    /// Baseline of each line, measured down from the top of the frame.
    ///
    /// Shifted down by 16 when the machine-readable strip moved to the very
    /// top of the frame: the strip owns rows 0…11, the QR starts at 16, and
    /// the text follows. The spacing between the lines is unchanged.
    pub const LINE_BASELINES: [u32; 3] = [36, 64, 92];
    /// Right margin the text stops at.
    pub const PAD_X: u32 = 12;
    /// Point size of the monospace font.
    pub const FONT_SIZE: u32 = 24;
    /// Ascent above the baseline the glyphs occupy, and descent below.
    pub const ASCENT: u32 = 18;
    pub const DESCENT: u32 = 6;
    /// Side of the QR code and its margins. It sits at the top left, below
    /// the machine-readable strip, and still shares rows with the text lines —
    /// which is why the reader has to separate them.
    pub const QR_SIZE: u32 = 87;
    /// Horizontal margin, unchanged.
    pub const QR_MARGIN: u32 = 4;
    /// Vertical margin: pushed below the strip, with four rows of clearance.
    pub const QR_MARGIN_Y: u32 = 16;
}

/// Everything scaled to the frame actually in hand.
///
/// The band is drawn at a fixed pixel size, so a copy rescaled to half width
/// carries a half-height band. The scale is known rather than guessed: the
/// original's width comes from the bundle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub scale: f32,
    pub frame_width: u32,
    pub frame_height: u32,
}

impl Geometry {
    /// For a frame believed to be an unscaled capture.
    pub fn native(width: u32, height: u32) -> Geometry {
        Geometry {
            scale: 1.0,
            frame_width: width,
            frame_height: height,
        }
    }

    /// For a copy whose width is `width`, given the original's width.
    pub fn scaled_from(original_width: u32, width: u32, height: u32) -> Geometry {
        let scale = if original_width == 0 {
            1.0
        } else {
            width as f32 / original_width as f32
        };
        Geometry {
            scale,
            frame_width: width,
            frame_height: height,
        }
    }

    fn s(&self, v: u32) -> u32 {
        ((v as f32) * self.scale).round() as u32
    }

    /// Rows one line's glyphs occupy, generously bounded.
    pub fn line_rows(&self, line: usize) -> Option<(u32, u32)> {
        let baseline = *native::LINE_BASELINES.get(line)?;
        let top = self.s(baseline.saturating_sub(native::ASCENT));
        let bottom = self.s(baseline + native::DESCENT).min(self.frame_height);
        (bottom > top).then_some((top, bottom))
    }

    /// The band as a rectangle, for callers that want to mask it out.
    pub fn band(&self) -> Rect {
        Rect::new(
            0,
            0,
            self.frame_width,
            self.s(native::BAND_HEIGHT).min(self.frame_height),
        )
    }

    /// Nominal character pitch. Measured in practice; this is the fallback and
    /// the sanity bound.
    pub fn nominal_pitch(&self) -> f32 {
        // 14.4 px per cell for a 24 pt monospace face, measured on real
        // captures from both platforms (glyph starts 15, 15, 15, 14, 15 …).
        14.4 * self.scale
    }

    /// Columns the QR occupies, which overlap line 1's rows.
    pub fn qr_right_edge(&self) -> u32 {
        self.s(native::QR_MARGIN + native::QR_SIZE)
    }
}

/// A horizontal run of glyph pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    x0: u32,
    x1: u32,
}

/// Mark the pixels that belong to burned-in glyphs.
///
/// Not a plain brightness threshold. The glyphs are white, but so is a bright
/// sky, and a threshold that keeps one keeps the other. What distinguishes the
/// text is that every platform draws a black halo around it — eight offset
/// passes on iOS, a stroke paint on Android — so a glyph pixel is bright AND
/// has something much darker within a pixel or two. Sky has no such
/// neighbour.
fn glyph_mask(band: &LumaFrame, bright: u8, halo: u8, radius: u32) -> Vec<bool> {
    let (w, h) = (band.width as i64, band.height as i64);
    let r = radius as i64;
    let mut out = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let p = band.at(x as u32, y as u32);
            if p < bright {
                continue;
            }
            let mut darkest = 255u8;
            for dy in -r..=r {
                for dx in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    darkest = darkest.min(band.at(nx as u32, ny as u32));
                }
            }
            out[(y * w + x) as usize] = darkest <= halo;
        }
    }
    out
}

/// Columns of one line that carry glyph pixels, as runs.
fn runs_of(mask: &[bool], w: u32, h: u32) -> Vec<Run> {
    let mut out = Vec::new();
    let mut start: Option<u32> = None;
    for x in 0..w {
        let on = (0..h).any(|y| mask[(y * w + x) as usize]);
        match (on, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                out.push(Run { x0: s, x1: x - 1 });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(Run { x0: s, x1: w - 1 });
    }
    out
}

/// The text block of a line: its extent and the grid pitch inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextGrid {
    pub left: u32,
    pub right: u32,
    pub pitch: f32,
    /// How many character cells the block holds.
    pub cells: usize,
    /// Column where the widest gap inside the block ends.
    ///
    /// Line 1 contains exactly one double space, immediately before `f=`, so
    /// this is a structural anchor that does not depend on counting cells from
    /// either end. It matters because counting from the left is fragile: a
    /// leading glyph the mask misses shifts every index by one, and the cells
    /// then straddle their neighbours — which is what left the counter's first
    /// digit unreadable on sixteen of seventeen refusals.
    pub anchor: Option<u32>,
}

impl TextGrid {
    /// Columns of cell `i`, counted from the LEFT of the block.
    pub fn cell(&self, i: usize) -> Option<(u32, u32)> {
        if i >= self.cells {
            return None;
        }
        let x0 = self.left as f32 + self.pitch * i as f32;
        let x1 = x0 + self.pitch;
        Some((x0.round() as u32, (x1.round() as u32).min(self.right)))
    }

    /// Columns of cell `i` counted BACK from the right edge, `0` being the
    /// last. The text is right-aligned, so this end is the one that does not
    /// move when a glyph is missed.
    pub fn cell_from_right(&self, i: usize) -> Option<(u32, u32)> {
        let x1 = self.right as f32 + 1.0 - self.pitch * i as f32;
        let x0 = x1 - self.pitch;
        (x0 >= 0.0 && x1 > x0).then(|| (x0.round() as u32, x1.round() as u32))
    }

    /// How many cells sit to the right of the anchor, `f` and `=` included.
    pub fn cells_after_anchor(&self) -> Option<usize> {
        let a = self.anchor?;
        Some(
            (((self.right + 1 - a) as f32) / self.pitch)
                .round()
                .max(0.0) as usize,
        )
    }
}

/// Gap, in multiples of the pitch, that separates the text from anything else
/// sharing its rows.
///
/// The QR sits in the same rows as line 1 and is hundreds of pixels away; the
/// widest gap INSIDE a line is the two spaces before `f=`, which is two cells.
/// Four is comfortably between them.
const BLOCK_GAP_CELLS: f32 = 4.0;

/// Find the right-aligned text block of one line.
pub fn find_grid(band: &LumaFrame, geom: &Geometry, line: usize) -> Option<TextGrid> {
    let (top, bottom) = geom.line_rows(line)?;
    let strip = band.crop(Rect::new(0, top, band.width, bottom - top))?;
    let mask = glyph_mask(&strip, 190, 90, FIND_RADIUS);
    let runs = runs_of(&mask, strip.width, strip.height);
    if runs.is_empty() {
        return None;
    }

    // Take the rightmost block: the text is right-aligned, and whatever else
    // shares these rows (the QR) is far to the left.
    let nominal = geom.nominal_pitch().max(2.0);
    let mut block: Vec<Run> = vec![*runs.last()?];
    for r in runs.iter().rev().skip(1) {
        let gap = block.first().unwrap().x0 as f32 - r.x1 as f32;
        if gap > nominal * BLOCK_GAP_CELLS {
            break;
        }
        block.insert(0, *r);
    }
    // A single narrow run is a speck, not a line of text.
    if block.len() < 4 {
        return None;
    }

    // The text is right-aligned at a known margin. A block that ends far from
    // it is something else sharing these rows — on a frame drawn before the
    // overlay data arrives, the QR in the top-left corner is the only thing
    // present and was being read as a line of nine characters.
    let margin = geom.frame_width.saturating_sub(geom.s(native::PAD_X));
    let right_edge = block.last()?.x1;
    if (right_edge as i64 - margin as i64).abs() > (nominal * 1.5) as i64 {
        return None;
    }

    // Edges measured again with the TIGHT mask.
    //
    // The block is found with the tolerant mask, which is right for finding —
    // a run that breaks mid-glyph costs a cell — but it bleeds a pixel or two
    // outward. Cells are then indexed from an edge that sits two pixels too
    // far out, every cut slides by 13 % of a cell, and the wide digits arrive
    // holding a slice of their neighbour. The narrow ones survived, which is
    // why the counter's `1` read and its `6` did not.
    let tight = glyph_mask(&strip, 190, 90, CUT_RADIUS);
    let tight_runs = runs_of(&tight, strip.width, strip.height);
    let (bl, br) = (block.first()?.x0, block.last()?.x1);
    let inside: Vec<&Run> = tight_runs
        .iter()
        .filter(|r| r.x1 >= bl && r.x0 <= br)
        .collect();
    let left = inside.first().map(|r| r.x0).unwrap_or(bl);
    let right = inside.last().map(|r| r.x1).unwrap_or(br);

    // Pitch, fitted across the WHOLE block rather than taken from neighbouring
    // glyphs.
    //
    // The neighbour deltas are integers — 15, 15, 15, 14, 15 — so their median
    // is 15 while the true pitch is nearer 14.4. That looks like a rounding
    // detail and is not: the cells are indexed by multiplying the pitch, so a
    // 0.6 px error becomes a whole cell of drift by cell 24, which is exactly
    // where the frame counter lives. Every counter came back a digit short.
    //
    // So: guess from the neighbours, assign each glyph the cell index it must
    // occupy, then fit the pitch to all of them at once and repeat. Two passes
    // are enough — the guess is never off by a whole cell at the near end, and
    // each pass pins the far end tighter.
    let mut deltas: Vec<f32> = block
        .windows(2)
        .map(|w| w[1].x0 as f32 - w[0].x0 as f32)
        .filter(|d| *d > nominal * 0.6 && *d < nominal * 1.6)
        .collect();
    let mut pitch = if deltas.len() >= 3 {
        deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        deltas[deltas.len() / 2]
    } else {
        nominal
    };
    for _ in 0..2 {
        let (mut num, mut den) = (0f64, 0f64);
        for r in &block {
            let dx = (r.x0 - left) as f64;
            let k = (dx / pitch as f64).round();
            if k > 0.0 {
                num += k * dx;
                den += k * k;
            }
        }
        if den > 0.0 {
            let fitted = (num / den) as f32;
            // Refuse a fit that wandered: it would mean the block is not a
            // monospace line at all.
            if fitted > nominal * 0.5 && fitted < nominal * 2.0 {
                pitch = fitted;
            }
        }
    }

    let cells = (((right - left + 1) as f32) / pitch).round().max(1.0) as usize;

    // The widest internal gap. On line 1 that is the double space before `f=`,
    // which is a structural anchor: it does not move when a leading glyph is
    // missed, and counting from the left does.
    let anchor = block
        .windows(2)
        .map(|w| (w[1].x0 - w[0].x1, w[1].x0))
        .filter(|(gap, _)| *gap as f32 > pitch * 1.2)
        .max_by_key(|(gap, _)| *gap)
        .map(|(_, at)| at);

    Some(TextGrid {
        left,
        right,
        pitch,
        cells,
        anchor,
    })
}

/// Cut the cell `i` places back from the right edge of the block.
pub fn cell_image_from_right(
    band: &LumaFrame,
    geom: &Geometry,
    line: usize,
    grid: &TextGrid,
    i: usize,
) -> Option<CellImage> {
    let (x0, x1) = grid.cell_from_right(i)?;
    cell_image_at(band, geom, line, grid, x0, x1, 0)
}

/// Cut one of the counter's digits.
///
/// The leftmost of them sits against the `=`, whose two heavy bars the halo
/// bridges into it, and that digit is where most refusals still land. Clamping
/// the cut so it cannot reach the `=` was tried and measured: it made things
/// worse, from nineteen reads to five and one of those wrong, because the
/// anchor is itself a cell wide and the clamp then bites into the digit. The
/// widened window and the nearest-ink-group rule do better than a hard edge,
/// so this stays a plain cut — and the remaining refusals stay refusals.
pub fn counter_digit_cell(
    band: &LumaFrame,
    geom: &Geometry,
    grid: &TextGrid,
    i: usize,
) -> Option<CellImage> {
    let (x0, x1) = grid.cell_from_right(i)?;
    cell_image_at(band, geom, 0, grid, x0, x1, 0)
}

/// Where each field of line 1 sits, in cells from the left of the block.
///
/// `2026-06-05T04:53:32Z  f=271` — twenty characters of timestamp, two spaces,
/// `f=`, then the counter. The `R<n>` repeat marker, when present, follows
/// after two more spaces.
pub mod line1 {
    pub const TIMESTAMP_CELLS: usize = 20;
    /// `f` and `=`, the two cells between the double space and the counter.
    pub const FIELD_PREFIX_CELLS: usize = 2;
    /// First cell of the counter's digits.
    pub const COUNTER_FIRST_CELL: usize = 24;
    /// Cells of the seconds field, whose units digit advances once a second
    /// and is what teaches the reader its digits.
    pub const SECONDS_TENS_CELL: usize = 17;
    pub const SECONDS_UNITS_CELL: usize = 18;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32, level: u8) -> LumaFrame {
        LumaFrame::new(w, h, vec![level; (w * h) as usize], 0, 0).unwrap()
    }

    /// A white block with a black halo, like a burned-in glyph.
    fn draw_glyph(f: &mut LumaFrame, x: u32, y: u32, w: u32, h: u32) {
        for yy in y.saturating_sub(1)..(y + h + 1).min(f.height) {
            for xx in x.saturating_sub(1)..(x + w + 1).min(f.width) {
                let i = (yy * f.width + xx) as usize;
                f.data[i] = 0;
            }
        }
        for yy in y..(y + h).min(f.height) {
            for xx in x..(x + w).min(f.width) {
                f.data[(yy * f.width + xx) as usize] = 255;
            }
        }
    }

    #[test]
    fn geometry_scales_with_the_frame() {
        let native = Geometry::native(1280, 720);
        assert_eq!(native.band().h, 98);
        // Line 1's baseline is 36 with an 18-row ascent and a 6-row descent.
        assert_eq!(native.line_rows(0), Some((18, 42)));

        let half = Geometry::scaled_from(1280, 640, 360);
        assert_eq!(half.scale, 0.5);
        assert_eq!(half.band().h, 49);
        assert_eq!(half.line_rows(0), Some((9, 21)));
        assert!((half.nominal_pitch() - 7.2).abs() < 0.01);
    }

    #[test]
    fn the_band_never_exceeds_the_frame() {
        // A very short frame must not produce a band taller than it is.
        let tiny = Geometry::native(320, 40);
        assert_eq!(tiny.band().h, 40);
    }

    #[test]
    fn a_bright_sky_is_not_mistaken_for_text() {
        // The failure this mask exists to prevent: a plain brightness cut
        // turns an overexposed sky into a page of glyphs.
        let sky = blank(200, 24, 245);
        let mask = glyph_mask(&sky, 190, 90, FIND_RADIUS);
        assert!(!mask.iter().any(|m| *m), "sky was read as glyph pixels");
    }

    #[test]
    fn a_haloed_glyph_is_found_on_any_background() {
        for level in [0u8, 128, 245] {
            let mut f = blank(200, 24, level);
            draw_glyph(&mut f, 100, 4, 10, 16);
            let mask = glyph_mask(&f, 190, 90, FIND_RADIUS);
            let runs = runs_of(&mask, f.width, f.height);
            assert_eq!(runs.len(), 1, "background {level}: {runs:?}");
            // The glyph spans 100..109 with a halo at 99..110; the mask may
            // bleed one radius further on a bright background.
            assert!(runs[0].x0 >= 97 && runs[0].x1 <= 113, "{:?}", runs[0]);
            assert!(runs[0].x1 - runs[0].x0 + 1 >= 10);
        }
    }

    #[test]
    fn the_grid_finds_a_right_aligned_line_and_its_pitch() {
        let geom = Geometry::native(600, 200);
        let mut f = blank(600, 200, 60);
        // Ten cells at pitch 14.4, ending near the right margin.
        let right = 600 - native::PAD_X;
        for i in 0..10u32 {
            let x = right - ((10 - i) as f32 * 14.4) as u32;
            draw_glyph(&mut f, x, 4, 12, 16);
        }
        let g = find_grid(&f, &geom, 0).expect("grid");
        // Fitted across the block, so it lands close to the true pitch rather
        // than on the integer its neighbours round to.
        assert!((g.pitch - 14.4).abs() < 0.4, "pitch {}", g.pitch);
        assert_eq!(g.cells, 10, "cells {}", g.cells);
        assert!(
            g.right >= right - 4 && g.right <= right + 2,
            "right {}",
            g.right
        );
    }

    #[test]
    fn the_qr_block_is_not_taken_for_text() {
        // The QR shares line 1's rows. It is hundreds of pixels to the left,
        // and must not extend the text block.
        let geom = Geometry::native(600, 200);
        let mut f = blank(600, 200, 60);
        for i in 0..6u32 {
            draw_glyph(&mut f, 4 + i * 14, 4, 12, 16); // stand-in for the QR
        }
        let right = 600 - native::PAD_X;
        for i in 0..8u32 {
            let x = right - ((8 - i) as f32 * 14.4) as u32;
            draw_glyph(&mut f, x, 4, 12, 16);
        }
        let g = find_grid(&f, &geom, 0).expect("grid");
        assert_eq!(g.cells, 8, "the QR was absorbed into the text block");
        assert!(g.left > 300, "block starts at {}, far too far left", g.left);
    }

    #[test]
    fn a_block_that_is_not_right_aligned_is_refused() {
        // The QR occupies the same rows as lines 1 and 2 and is the only thing
        // on a frame composed before the overlay data is ready. It is not text
        // and must not be read as nine characters of it.
        let geom = Geometry::native(600, 200);
        let mut f = blank(600, 200, 60);
        for i in 0..8u32 {
            draw_glyph(&mut f, 4 + i * 14, 4, 12, 16);
        }
        assert!(find_grid(&f, &geom, 0).is_none());
    }

    #[test]
    fn an_empty_line_yields_no_grid_rather_than_a_guess() {
        let geom = Geometry::native(600, 200);
        assert!(find_grid(&blank(600, 200, 60), &geom, 0).is_none());
        assert!(find_grid(&blank(600, 200, 250), &geom, 0).is_none());
    }

    #[test]
    fn cells_can_be_indexed_from_the_right_edge() {
        // The right edge is the one that does not move: the text is
        // right-aligned, so a leading glyph the mask misses shifts every
        // left-hand index by a cell and leaves the cuts straddling their
        // neighbours. Reading the counter back from this end is what fixed it.
        let g = TextGrid {
            left: 100,
            right: 244,
            pitch: 14.4,
            cells: 10,
            anchor: None,
        };
        let last = g.cell_from_right(0).unwrap();
        assert!(last.1 >= 244 && last.1 <= 246, "{last:?}");
        assert!(last.0 >= 230 && last.0 <= 232, "{last:?}");
        let prev = g.cell_from_right(1).unwrap();
        assert!(
            prev.1 <= last.0 + 1,
            "cells overlap: {prev:?} then {last:?}"
        );
    }

    #[test]
    fn the_widest_internal_gap_is_offered_as_an_anchor() {
        // Line 1 holds exactly one double space, before `f=`. It is the only
        // landmark that does not move when a glyph is missed.
        let geom = Geometry::native(600, 200);
        let mut f = blank(600, 200, 60);
        let right = 600 - native::PAD_X;
        // Six cells, a two-cell gap, then four more.
        for i in 0..6u32 {
            let x = right - ((12 - i) as f32 * 14.4) as u32;
            draw_glyph(&mut f, x, 4, 12, 16);
        }
        for i in 8..12u32 {
            let x = right - ((12 - i) as f32 * 14.4) as u32;
            draw_glyph(&mut f, x, 4, 12, 16);
        }
        let g = find_grid(&f, &geom, 0).expect("grid");
        let anchor = g.anchor.expect("anchor");
        // The gap ends where the second group begins.
        let expected = right - (4.0 * 14.4) as u32;
        assert!(
            (anchor as i64 - expected as i64).abs() <= 4,
            "anchor {anchor}, expected about {expected}"
        );
        assert_eq!(g.cells_after_anchor(), Some(4));
    }

    #[test]
    fn cells_are_indexed_from_the_left_of_the_block() {
        let g = TextGrid {
            left: 100,
            right: 244,
            pitch: 14.4,
            cells: 10,
            anchor: None,
        };
        assert_eq!(g.cell(0).unwrap().0, 100);
        let c9 = g.cell(9).unwrap();
        assert!(c9.0 >= 229 && c9.0 <= 231, "{c9:?}");
        assert!(g.cell(10).is_none());
    }
}

// ---------------------------------------------------------------------------
// Reading the cells
// ---------------------------------------------------------------------------

/// Halo search radius when LOCATING the band, and when CUTTING a cell.
///
/// Two different jobs wanting opposite things. Finding the grid wants
/// tolerance: a re-encode blurs the halo, and a run that breaks in the middle
/// of a glyph costs a cell. Cutting a cell wants precision: glyphs sit two
/// pixels apart, so a two-pixel bleed closes the gap and every cell arrives
/// carrying a slice of its neighbours — which is what it did, and every digit
/// came back wrong.
const FIND_RADIUS: u32 = 2;
const CUT_RADIUS: u32 = 1;

/// Size every cell is normalised to before it is compared.
///
/// Small enough to be scale-free — the glyphs are 24 pt at capture and may
/// arrive at half that after a re-encode, so both sides are reduced to the same
/// grid — and large enough to keep the strokes that separate similar digits.
///
/// 10×16 was measured against 14×22 and won, which is the opposite of the
/// intuition: the finer grid resolves more of the encoder's noise as well as
/// more of the glyph, and agreement scores fall across the board. Read rate
/// went from 26 frames in 36 down to 20. Resolution is not the lever here —
/// the margin between the best and second-best match is.
const CELL_W: usize = 10;
const CELL_H: usize = 16;

/// One cell of the grid, reduced to a comparable shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellImage {
    bits: Vec<bool>,
}

impl CellImage {
    /// How much of the cell is glyph. A cell far below this is blank.
    fn ink(&self) -> f32 {
        self.bits.iter().filter(|b| **b).count() as f32 / self.bits.len() as f32
    }

    /// ASCII rendering, for looking at what was cut out.
    pub fn rows(&self) -> Vec<String> {
        (0..CELL_H)
            .map(|y| {
                (0..CELL_W)
                    .map(|x| if self.bits[y * CELL_W + x] { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    /// How much of the cell is glyph. A cell far below this is blank.
    pub fn ink_fraction(&self) -> f32 {
        self.ink()
    }

    /// Fraction of pixels that agree, each weighted by how much it separates
    /// one digit from another.
    ///
    /// Unweighted agreement was the wrong measure and the failure it produced
    /// was specific: `5` read as `0`. Those two shapes share their outline and
    /// differ in a handful of strokes, so most of the grid agrees whichever one
    /// it is, and the few pixels that actually decide are outvoted by the many
    /// that cannot. Weighting by how often a pixel differs ACROSS the ten
    /// learned digits gives the decisive pixels their say — a pixel every digit
    /// has is worth nothing, and is now worth nothing.
    fn weighted_agreement(&self, other: &CellImage, weights: &[f32], total: f32) -> f32 {
        if total <= 0.0 {
            return 0.0;
        }
        let mut same = 0.0;
        for (i, (a, b)) in self.bits.iter().zip(&other.bits).enumerate() {
            if a == b {
                same += weights[i];
            }
        }
        same / total
    }
}

/// Cut one cell out of a line and reduce it to [`CellImage`].
pub fn cell_image(
    band: &LumaFrame,
    geom: &Geometry,
    line: usize,
    grid: &TextGrid,
    i: usize,
) -> Option<CellImage> {
    let (x0, x1) = grid.cell(i)?;
    cell_image_at(band, geom, line, grid, x0, x1, 0)
}

/// Cut the cell spanning `x0..x1`, whichever end it was indexed from.
fn cell_image_at(
    band: &LumaFrame,
    geom: &Geometry,
    line: usize,
    grid: &TextGrid,
    x0: u32,
    x1: u32,
    floor: u32,
) -> Option<CellImage> {
    let (top, bottom) = geom.line_rows(line)?;
    if x1 <= x0 {
        return None;
    }
    // Cut a window WIDER than the cell, then centre on the glyph inside it.
    //
    // Laying cells on the arithmetic grid alone does not work: the pitch is
    // fractional, the fit has a residual, and a cell that is a pixel or two off
    // arrives carrying a slice of its neighbour. Every digit came back wrong
    // that way, and the dumped cells showed exactly why — a clean `2` with a
    // stray stroke from the glyph before it.
    //
    // The glyph itself says where it is. Widening by half a pitch on each side
    // guarantees it is inside the window whatever the residual, and the ink's
    // own centre of mass then places the cell on it.
    let pad = (grid.pitch * 0.5).round() as u32;
    let wx0 = x0.saturating_sub(pad).max(floor);
    let wx1 = (x1 + pad).min(band.width);
    let window = band.crop(Rect::new(wx0, top, wx1 - wx0, bottom - top))?;
    let wmask = glyph_mask(&window, 190, 90, CUT_RADIUS);

    // Ink centred on the nominal middle of the cell, in window coordinates.
    let nominal_mid = ((x0 + x1) / 2).saturating_sub(wx0) as f32;
    // Falling back to the arithmetic centre when no group qualifies, rather
    // than giving up on the cell. The cell that forced this is the counter's
    // first digit: it sits against the `=`, whose bars the halo bridges into
    // one over-wide group, and refusing left that digit unread on sixteen of
    // seventeen refusals. A slightly off-centre cell may still classify, and
    // when it does not it is refused exactly as before — so the fallback can
    // only add reads, never wrong ones.
    let centre = ink_centre(&wmask, window.width, window.height, nominal_mid, grid.pitch)
        .unwrap_or(nominal_mid);
    let half = grid.pitch * 0.5;
    let cx0 = (centre - half).round().max(0.0) as u32;
    let cx1 = ((centre + half).round() as u32).min(window.width);
    if cx1 <= cx0 {
        return None;
    }
    let strip = window.crop(Rect::new(cx0, 0, cx1 - cx0, window.height))?;
    let mask = glyph_mask(&strip, 190, 90, CUT_RADIUS);

    // Nearest-neighbour down to the comparison grid. The shapes involved are
    // digits; a smoother reduction would blur the stroke that separates a 3
    // from an 8.
    let mut bits = vec![false; CELL_W * CELL_H];
    for y in 0..CELL_H {
        let sy = y * strip.height as usize / CELL_H;
        for x in 0..CELL_W {
            let sx = x * strip.width as usize / CELL_W;
            bits[y * CELL_W + x] = mask[sy * strip.width as usize + sx];
        }
    }
    Some(CellImage { bits })
}

/// Centre of the ink GROUP nearest `nominal`.
///
/// A weighted centre of everything in the window does not work, and the cell
/// that proved it was the counter's first digit: it sits immediately after the
/// `=`, whose two heavy bars are close enough to drag the centre left and pull
/// a slice of them into the crop. The digit was then unrecognisable while its
/// neighbours read fine.
///
/// Contiguous ink is one glyph. Choosing the nearest group instead of averaging
/// makes a neighbour irrelevant however much ink it has, which is the property
/// actually wanted.
fn ink_centre(mask: &[bool], w: u32, h: u32, nominal: f32, pitch: f32) -> Option<f32> {
    let ink: Vec<bool> = (0..w)
        .map(|x| (0..h).any(|y| mask[(y * w + x) as usize]))
        .collect();

    // Contiguous runs, tolerating a one-column break: a digit drawn with a thin
    // waist can lose a column to the encoder without becoming two glyphs.
    let mut groups: Vec<(u32, u32)> = Vec::new();
    let mut start: Option<u32> = None;
    let mut gap = 0u32;
    for x in 0..w {
        if ink[x as usize] {
            if start.is_none() {
                start = Some(x);
            }
            gap = 0;
        } else if let Some(s) = start {
            gap += 1;
            if gap > 1 {
                groups.push((s, x - gap));
                start = None;
                gap = 0;
            }
        }
    }
    if let Some(s) = start {
        groups.push((s, w - 1));
    }

    groups
        .into_iter()
        // A group wider than a cell is two glyphs the mask merged; centring on
        // it would straddle both.
        .filter(|(a, b)| (b - a + 1) as f32 <= pitch * 1.1)
        .map(|(a, b)| (a as f32 + b as f32) / 2.0)
        .min_by(|p, q| {
            (p - nominal)
                .abs()
                .partial_cmp(&(q - nominal).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Digit shapes learned from a recording rather than shipped with the tool.
///
/// **Why learned.** Shipping templates would mean shipping a font, pinning the
/// exact weight and rasteriser two mobile platforms happen to use today, and
/// breaking silently the day either changes. The original is in hand and its
/// band is drawn by the device that made it, so its own glyphs are the right
/// templates by construction — and they arrive already carrying whatever the
/// encoder did to them.
///
/// **How they are labelled.** The timestamp in line 1 is predictable: the band
/// shows the device wall clock, and the signed envelope's `phone_time_us` comes
/// from that same clock, so the manifest says what each frame's timestamp
/// should read. Labelling the original's cells against that prediction costs
/// nothing and, when it fails, says something worth knowing — see
/// [`DigitBook::learn`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DigitBook {
    /// Votes per pixel per digit, and how many examples voted. A template is
    /// the per-pixel majority.
    ///
    /// Averaging rather than keeping the first example, because one example is
    /// one encoder's rendering of one instant: a digit that happened to land on
    /// a bright background, or between two I-frames, becomes the standard every
    /// later cell is measured against. Ten examples average that away, and the
    /// pixels they disagree on are exactly the ones that should not be voting.
    votes: Vec<[u16; 10]>,
    counts: [u16; 10],
}

/// How much of a cell must agree with a template for the read to count, and
/// how far ahead of the runner-up it must be.
///
/// Both matter. A high agreement on its own is reachable by two digits that
/// share most of their strokes — 8 and 9, 3 and 8 — and a read that cannot
/// separate them is not a read. Refusing is always available and always
/// correct here: an unread counter is reported as unread.
const MIN_AGREEMENT: f32 = 0.80;
/// Swept against two real recordings, reading the counter back on every frame
/// and checking it against what it must be:
///
/// ```text
///   margin   recording A      recording B
///   0.06     27/36            15/23, one WRONG
///   0.08     27/36            13/23, all correct
///   0.10     24/36            13/23, all correct
///   0.12     23/36             0/23
/// ```
///
/// 0.08 is where the last wrong read disappears without costing recall. Two
/// frames of recall bought the single wrong read at 0.06, and that is not a
/// trade worth making: a refused counter is reported as refused, while a wrong
/// one points confidently at the wrong frame of the original and nothing
/// downstream can tell.
const MIN_MARGIN: f32 = 0.08;

/// Below this a cell carries no glyph at all.
const MIN_INK: f32 = 0.04;

impl DigitBook {
    pub fn is_complete(&self) -> bool {
        self.counts.iter().all(|c| *c > 0)
    }

    pub fn learned_digits(&self) -> usize {
        self.counts.iter().filter(|c| **c > 0).count()
    }

    /// How many examples were seen of each digit.
    pub fn examples(&self, digit: u8) -> u16 {
        self.counts.get(digit as usize).copied().unwrap_or(0)
    }

    /// Teach one digit from a cell known to hold it.
    pub fn teach(&mut self, digit: u8, cell: CellImage) {
        if digit >= 10 || cell.ink() < MIN_INK {
            return;
        }
        if self.votes.is_empty() {
            self.votes = vec![[0u16; 10]; CELL_W * CELL_H];
        }
        for (i, b) in cell.bits.iter().enumerate() {
            if *b {
                self.votes[i][digit as usize] += 1;
            }
        }
        self.counts[digit as usize] += 1;
    }

    /// How much each pixel separates the digits from each other.
    ///
    /// One minus how lopsided the pixel is across the ten templates: a pixel
    /// set in all of them, or in none, decides nothing and scores zero.
    fn discrimination(&self) -> (Vec<f32>, f32) {
        let templates: Vec<CellImage> = (0..10u8).filter_map(|d| self.template(d)).collect();
        let n = templates.len() as f32;
        if n < 2.0 {
            let w = vec![1.0; CELL_W * CELL_H];
            let t = w.len() as f32;
            return (w, t);
        }
        let mut weights = Vec::with_capacity(CELL_W * CELL_H);
        for i in 0..CELL_W * CELL_H {
            let set = templates.iter().filter(|t| t.bits[i]).count() as f32;
            let p = set / n;
            // Peaks at 1.0 for a pixel half the digits have, zero for one they
            // all share.
            weights.push(4.0 * p * (1.0 - p));
        }
        let total: f32 = weights.iter().sum();
        (weights, total)
    }

    /// The per-pixel majority shape for one digit.
    pub fn template(&self, digit: u8) -> Option<CellImage> {
        let n = *self.counts.get(digit as usize)?;
        if n == 0 {
            return None;
        }
        let bits = (0..CELL_W * CELL_H)
            .map(|i| self.votes[i][digit as usize] * 2 > n)
            .collect();
        Some(CellImage { bits })
    }

    /// Learn the counter's own digits, in the place they will be read from.
    ///
    /// **A second pass, and it is the one that matters.** The first pass learns
    /// from the timestamp, whose cells sit between other timestamp glyphs. The
    /// counter's leftmost digit sits against the `=`, whose two heavy bars the
    /// halo bridges into it — so it arrives shaped differently from anything
    /// the first pass saw, and it was the digit that refused on essentially
    /// every failed read.
    ///
    /// Learning it where it lives fixes that by construction: the example and
    /// the subject are cut from the same position, with the same neighbour, by
    /// the same code.
    ///
    /// `counter_of` gives the counter a frame must be showing. In the product
    /// that comes from the original's own frame index plus the offset fitted
    /// from a handful of first-pass reads; nothing here has to be assumed.
    pub fn learn_counters<F>(&mut self, frames: &[&LumaFrame], geom: &Geometry, counter_of: F)
    where
        F: Fn(&LumaFrame) -> Option<u64>,
    {
        for frame in frames {
            let Some(counter) = counter_of(frame) else {
                continue;
            };
            let Some(grid) = find_grid(frame, geom, 0) else {
                continue;
            };
            let digits: Vec<u8> = counter
                .to_string()
                .bytes()
                .map(|b| b - b'0')
                .rev()
                .collect();
            // Only when the field is exactly as long as the number: a grid
            // that disagrees is a grid we have misread, and teaching from it
            // would poison every later read.
            let Some(after) = grid.cells_after_anchor() else {
                continue;
            };
            if after != digits.len() + line1::FIELD_PREFIX_CELLS {
                continue;
            }
            for (i, d) in digits.iter().enumerate() {
                if let Some(cell) = counter_digit_cell(frame, geom, &grid, i) {
                    self.teach(*d, cell);
                }
            }
        }
    }

    /// The whole learning procedure: timestamp first, then the counter's own
    /// digits, iterated.
    ///
    /// Returns the book and the offset between the counter and the frame index,
    /// fitted from what could be read rather than assumed. `None` for the
    /// offset means nothing was readable at all, and the book is whatever the
    /// first pass managed.
    ///
    /// Three rounds because it settles there: measured on a real recording the
    /// readable share of the learning frames went 9 of 18, then 14, then 14.
    /// The read rate over the whole video went from 19 frames in 36 to 27.
    pub fn learn_two_pass(
        samples: &[(&LumaFrame, String)],
        geom: &Geometry,
    ) -> (DigitBook, Option<i64>) {
        let mut book = DigitBook::learn(samples, geom);

        // Offset between the counter and the frame index, by majority of what
        // the first pass could read.
        let mut votes: std::collections::BTreeMap<i64, usize> = Default::default();
        for (f, _) in samples {
            if let Some(c) = read_counter(f, geom, &book).counter {
                *votes.entry(c as i64 - f.index as i64).or_default() += 1;
            }
        }
        let Some((&offset, _)) = votes.iter().max_by_key(|(_, n)| **n) else {
            return (book, None);
        };

        let frames: Vec<&LumaFrame> = samples.iter().map(|(f, _)| *f).collect();
        for _ in 0..3 {
            book.learn_counters(&frames, geom, |f| {
                let c = f.index as i64 + offset;
                (c > 0).then_some(c as u64)
            });
        }
        (book, Some(offset))
    }

    /// Learn from frames whose line-1 text is known.
    ///
    /// `samples` pairs a frame with the exact string its line 1 should read.
    /// Only the twenty timestamp cells are used: their content is predictable,
    /// while the counter's is the thing being read.
    pub fn learn(samples: &[(&LumaFrame, String)], geom: &Geometry) -> DigitBook {
        let mut book = DigitBook::default();
        for (frame, text) in samples {
            let Some(grid) = find_grid(frame, geom, 0) else {
                continue;
            };
            // The grid must be long enough to hold a timestamp and a counter,
            // or the line is not what we think it is.
            if grid.cells < line1::COUNTER_FIRST_CELL + 1 {
                continue;
            }
            for (i, ch) in text.chars().take(line1::TIMESTAMP_CELLS).enumerate() {
                let Some(d) = ch.to_digit(10) else { continue };
                if let Some(cell) = cell_image(frame, geom, 0, &grid, i) {
                    book.teach(d as u8, cell);
                }
            }
        }
        book
    }

    /// Read one cell. `None` when it is blank, unlike anything learned, or too
    /// close a call between two digits.
    pub fn classify(&self, cell: &CellImage) -> Option<u8> {
        if cell.ink() < MIN_INK {
            return None;
        }
        let (weights, total) = self.discrimination();
        let mut best: (f32, u8) = (0.0, 0);
        let mut second = 0.0f32;
        for d in 0..10u8 {
            let Some(t) = self.template(d) else { continue };
            let a = cell.weighted_agreement(&t, &weights, total);
            if a > best.0 {
                second = best.0;
                best = (a, d);
            } else if a > second {
                second = a;
            }
        }
        (best.0 >= MIN_AGREEMENT && best.0 - second >= MIN_MARGIN).then_some(best.1)
    }
}

/// Why a band could not be read. Counting these is how the read rate gets
/// improved: "19 of 36" says nothing about what to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// No right-aligned block of text was found on line 1.
    NoGrid,
    /// A block was found but is too short to hold a timestamp and a counter.
    GridTooShort,
    /// A counter cell could not be told from another digit with confidence.
    CellUnreadable,
    /// The digits stopped short of the end of the field.
    IncompleteRead,
}

/// What one frame's band declared about itself.
///
/// Every field is a claim the picture makes, and none of it is signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BandReading {
    /// The `f=` counter: which frame of its recording this frame says it is.
    pub counter: Option<u64>,
    /// How many cells of the counter were read, against how many it has.
    pub counter_digits_read: usize,
    pub counter_digits_total: usize,
    /// Why, when there is no counter. Counted rather than shrugged at: "19 of
    /// 36 frames read" says nothing about what to improve, and the four
    /// reasons want four different fixes.
    pub refusal: Option<Refusal>,
}

impl BandReading {
    pub fn unread(why: Refusal) -> BandReading {
        BandReading {
            counter: None,
            counter_digits_read: 0,
            counter_digits_total: 0,
            refusal: Some(why),
        }
    }
}

/// Where a frame counter came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterSource {
    /// The machine-readable strip: checksummed, so a corrupted read is refused
    /// rather than returned wrong.
    BitRow,
    /// The `f=` field of the text band, read glyph by glyph. The fallback for
    /// recordings made before the strip existed.
    TextBand,
}

/// What one frame declared, and how it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CounterRead {
    pub counter: Option<u64>,
    pub source: Option<CounterSource>,
    /// The recording the strip named, when it was the strip that answered.
    pub session_tag: Option<u16>,
}

/// Read the counter by whichever means the frame offers.
///
/// The strip first, always. It carries a checksum, so it either returns the
/// right number or nothing — measured at 100 % of frames on four real
/// recordings across both platforms and both orientations, against 50–75 % for
/// the text with no way to tell a misread from a good one.
///
/// The text remains the fallback rather than being dropped: recordings made
/// before the strip existed still have to be readable, and the text is also
/// what a person reads when they have no tool at all.
///
/// `frame` must already be NORMALISED — bars removed — because both readers
/// measure from the top-left of the picture, and a letterboxed copy has
/// neither where the phone drew it.
pub fn read_counter_any(frame: &LumaFrame, geom: &Geometry, book: &DigitBook) -> CounterRead {
    if let Ok(row) = crate::bitrow::read(frame, geom.scale) {
        return CounterRead {
            counter: Some(row.counter),
            source: Some(CounterSource::BitRow),
            session_tag: Some(row.session_tag),
        };
    }
    match read_counter(frame, geom, book).counter {
        Some(c) => CounterRead {
            counter: Some(c),
            source: Some(CounterSource::TextBand),
            session_tag: None,
        },
        None => CounterRead {
            counter: None,
            source: None,
            session_tag: None,
        },
    }
}

/// Read the frame counter out of one frame's band.
///
/// Refuses a partial read rather than reporting a number missing a digit: a
/// counter read as 27 when it says 271 would point at the wrong frame of the
/// original with total confidence.
pub fn read_counter(frame: &LumaFrame, geom: &Geometry, book: &DigitBook) -> BandReading {
    let Some(grid) = find_grid(frame, geom, 0) else {
        return BandReading::unread(Refusal::NoGrid);
    };
    // How many cells the counter has: everything past the double space, less
    // the `f` and the `=`.
    //
    // Read off the ANCHOR, not counted from the left edge. Counting from the
    // left assumes the leftmost glyph found is the timestamp's first
    // character; when the mask misses a faint one, every index shifts by a
    // cell and the cuts straddle their neighbours. A dump of a failing frame
    // showed exactly that — the cell that should have held `=` held one of its
    // two bars — and it accounted for sixteen of seventeen refusals.
    let total = match grid.cells_after_anchor() {
        Some(n) if n > line1::FIELD_PREFIX_CELLS => n - line1::FIELD_PREFIX_CELLS,
        // No anchor found: fall back to counting, which is right whenever the
        // left edge is.
        _ if grid.cells > line1::COUNTER_FIRST_CELL => grid.cells - line1::COUNTER_FIRST_CELL,
        _ => return BandReading::unread(Refusal::GridTooShort),
    };

    // Right to left, because that edge is the one that does not move, then put
    // the digits back in reading order.
    let mut digits: Vec<u8> = Vec::new();
    for i in 0..total {
        let Some(cell) = counter_digit_cell(frame, geom, &grid, i) else {
            break;
        };
        match book.classify(&cell) {
            Some(d) => digits.insert(0, d),
            None => break,
        }
    }
    // Every cell of the counter, or none of it. A partial read is the one
    // outcome worse than no read: "271" seen as "27" points confidently at a
    // frame ten times too early, and nothing downstream could tell.
    //
    // The exception is a trailing `R<n>` repeat marker, which sits after two
    // spaces — the blank cell stops the loop, and what came before it is the
    // whole counter. So a short read is only trusted when what stopped it was
    // a blank cell rather than an unrecognised one.
    let complete = digits.len() == total || stopped_at_blank(frame, geom, &grid, digits.len());
    let ok = !digits.is_empty() && complete;
    BandReading {
        counter: ok.then(|| digits.iter().fold(0u64, |acc, d| acc * 10 + *d as u64)),
        counter_digits_read: digits.len(),
        counter_digits_total: total,
        refusal: (!ok).then_some(if digits.is_empty() {
            Refusal::CellUnreadable
        } else {
            Refusal::IncompleteRead
        }),
    }
}

/// Whether the cell just past the digits read is blank — the space before an
/// `R<n>` marker — rather than a glyph that could not be classified.
fn stopped_at_blank(frame: &LumaFrame, geom: &Geometry, grid: &TextGrid, read: usize) -> bool {
    // Counted from the right, like the digits themselves.
    match cell_image_from_right(frame, geom, 0, grid, read) {
        Some(c) => c.ink() < MIN_INK,
        None => false,
    }
}

#[cfg(test)]
mod reading_tests {
    use super::*;

    #[test]
    fn an_empty_book_reads_nothing_rather_than_guessing() {
        let book = DigitBook::default();
        let cell = CellImage {
            bits: vec![true; CELL_W * CELL_H],
        };
        assert_eq!(book.classify(&cell), None);
        assert_eq!(book.learned_digits(), 0);
        assert!(!book.is_complete());
    }

    #[test]
    fn a_blank_cell_is_never_a_digit() {
        let mut book = DigitBook::default();
        book.teach(
            7,
            CellImage {
                bits: vec![true; CELL_W * CELL_H],
            },
        );
        let blank = CellImage {
            bits: vec![false; CELL_W * CELL_H],
        };
        assert_eq!(book.classify(&blank), None);
    }

    #[test]
    fn a_cell_between_two_digits_is_refused_rather_than_picked() {
        // The failure that matters: a counter read as the wrong number points
        // at the wrong frame of the original with complete confidence.
        //
        // Note what "alike" has to mean now. Weighting by discrimination gives
        // the pixels two digits differ on the whole say, so two templates that
        // differ anywhere at all are told apart cleanly — that is the point of
        // the weighting. What must still be refused is a cell that sits BETWEEN
        // them on those very pixels.
        let base: Vec<bool> = (0..CELL_W * CELL_H).map(|i| i % 3 == 0).collect();
        let mut a = base.clone();
        let mut b = base.clone();
        // Four pixels that separate the two digits, two each.
        for (i, k) in [(1usize, true), (2, true), (4, false), (5, false)] {
            a[i] = k;
            b[i] = !k;
        }
        let mut book = DigitBook::default();
        book.teach(3, CellImage { bits: a.clone() });
        book.teach(8, CellImage { bits: b.clone() });

        // Half of each: no margin either way.
        let mut between = base;
        between[1] = true;
        between[2] = false;
        between[4] = false;
        between[5] = true;
        assert_eq!(book.classify(&CellImage { bits: between }), None);

        // And each template still reads as itself.
        assert_eq!(book.classify(&CellImage { bits: a }), Some(3));
        assert_eq!(book.classify(&CellImage { bits: b }), Some(8));
    }

    #[test]
    fn a_clear_match_is_read() {
        let mut shape = vec![false; CELL_W * CELL_H];
        for (i, b) in shape.iter_mut().enumerate() {
            *b = i % 4 == 0;
        }
        let mut other = vec![false; CELL_W * CELL_H];
        for (i, b) in other.iter_mut().enumerate() {
            *b = i % 2 == 0;
        }
        let mut book = DigitBook::default();
        book.teach(
            5,
            CellImage {
                bits: shape.clone(),
            },
        );
        book.teach(1, CellImage { bits: other });
        assert_eq!(book.classify(&CellImage { bits: shape }), Some(5));
    }

    #[test]
    fn a_partial_read_is_refused_rather_than_reported_short() {
        // "271" read as "27" points at a frame ten times too early, with
        // nothing downstream able to notice. Refusing is the only safe answer.
        let r = BandReading {
            counter: None,
            counter_digits_read: 2,
            counter_digits_total: 3,
            refusal: Some(Refusal::IncompleteRead),
        };
        assert_eq!(r.counter, None);
        assert!(r.counter_digits_read < r.counter_digits_total);
    }

    #[test]
    fn a_frame_with_no_band_reads_as_unread_not_as_zero() {
        let geom = Geometry::native(600, 200);
        let blank = LumaFrame::new(600, 200, vec![60; 600 * 200], 0, 0).unwrap();
        let r = read_counter(&blank, &geom, &DigitBook::default());
        assert_eq!(r.counter, None);
        assert_eq!(r.counter_digits_read, 0);
        assert_eq!(r.refusal, Some(Refusal::NoGrid));
    }
}
