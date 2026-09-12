//! Perceptual fingerprints: one 64-bit number per frame, comparable across
//! re-encodings.
//!
//! ## The method, and why this one
//!
//! **pHash**, the DCT-based construction described in Christoph Zauner's 2010
//! thesis *Implementation and Benchmarking of Perceptual Image Hash Functions*
//! (§4.2), which is itself the published form of Neal Krawetz's write-up. The
//! steps are fixed and reproducible:
//!
//!   1. resize the luma plane to 32×32,
//!   2. take the 2-D DCT-II,
//!   3. keep the top-left 8×8 block of low-frequency coefficients,
//!   4. drop the DC term, which carries only overall brightness,
//!   5. threshold the remaining 63 at their median,
//!   6. read the bits out in a fixed order.
//!
//! Two frames are compared by the Hamming distance between their hashes.
//!
//! **No hash is invented here, and that is deliberate.** A fingerprint whose
//! construction is not published cannot be argued with, and the whole output of
//! this tool is meant to be arguable. A reader who thinks the alignment is
//! wrong can recompute these values with any pHash implementation and check.
//!
//! ## What it is robust to, and what it is not
//!
//! Keeping only low frequencies is what survives re-encoding: a codec destroys
//! high-frequency detail first, and the DC term is dropped so a global
//! brightness shift moves nothing. It is also robust to modest scaling and to
//! small changes in a corner of the picture.
//!
//! It is **not** robust to cropping, rotation, or mirroring, and it is not
//! meant to be — those are handled before hashing, by normalisation. It is also
//! weak on frames with almost no structure: a shot of a blank wall or a sky
//! hashes close to every other blank frame. That is a property of the picture
//! rather than a defect of the hash, it is measurable in advance, and
//! [`Fingerprint::is_low_variance`] flags it so the alignment can decline to
//! rest weight on such a frame instead of matching noise to noise.

use crate::frame::LumaFrame;

/// Side of the square the frame is reduced to before the DCT.
const SIDE: usize = 32;
/// Side of the low-frequency block kept from the DCT.
const KEEP: usize = 8;

/// A 64-bit perceptual hash of one frame, plus what it is worth.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    /// Bit `i` is coefficient `i` of the kept block, above its median.
    ///
    /// Bit 0 corresponds to the DC term, which is always cleared: it is
    /// excluded from both the median and the comparison, and kept as a slot
    /// only so bit positions match the coefficient grid.
    pub bits: u64,
    /// Spread of the kept coefficients around their median, in DCT units.
    ///
    /// Contrast, essentially: how much energy sits away from the middle of the
    /// low-frequency block. Useful for spotting a frame with no picture at all
    /// — a fade to black, a covered lens — and useful for nothing else.
    ///
    /// It is emphatically NOT a measure of how distinctive a frame is, and the
    /// measurement says so: over three real recordings it ranges from 34 to
    /// 164, and the highest values belong to a static shot of a roofline whose
    /// every moment looks like every other. A high-contrast picture that never
    /// changes scores at the top of this scale. Distinctiveness is a property
    /// of a frame RELATIVE TO ITS OWN VIDEO, and it is measured in
    /// `align::ambiguous_frames` where it belongs.
    pub spread: f32,
}

/// Below this there is no picture: the kept block fits inside a couple of luma
/// steps, so the bits come out of rounding noise.
///
/// Deliberately far below anything real footage produces — measured, the
/// lowest spread over three recordings was 34. This catches a black frame, a
/// covered lens, a blown-out white, and is not asked to catch anything else.
/// An earlier comment here claimed this threshold made the hash "weigh" flat
/// frames properly; that was wrong twice over, since it never fires on real
/// material and since contrast is not what makes a frame informative.
const LOW_VARIANCE_SPREAD: f32 = 2.0;

impl Fingerprint {
    /// Hamming distance: how many of the 63 meaningful bits differ.
    ///
    /// 0 means the two frames reduce to the same low-frequency picture. It does
    /// NOT mean they are the same frame, and nothing in this crate treats it
    /// that way — a match is a hypothesis that the alignment then has to make
    /// sense of in sequence.
    pub fn distance(self, other: Fingerprint) -> u32 {
        // Bit 0 is the cleared DC slot in both, so it never contributes.
        (self.bits ^ other.bits).count_ones()
    }

    /// Whether this frame carries no picture at all.
    ///
    /// A far weaker claim than "this frame is uninformative" — see [`Self::spread`].
    pub fn is_low_variance(self) -> bool {
        self.spread < LOW_VARIANCE_SPREAD
    }
}

/// Compute the pHash of one frame.
pub fn fingerprint(frame: &LumaFrame) -> Fingerprint {
    let small = box_resize(frame, SIDE, SIDE);
    let coeffs = dct2d(&small, SIDE);

    // The kept block, DC first. The DC term is collected so the indices line
    // up with the grid, and excluded from every decision below.
    let mut kept = [0f32; KEEP * KEEP];
    for v in 0..KEEP {
        for u in 0..KEEP {
            kept[v * KEEP + u] = coeffs[v * SIDE + u];
        }
    }

    let mut ac: Vec<f32> = kept[1..].to_vec();
    let median = median_of(&mut ac);
    let spread = mean_abs_deviation(&kept[1..], median);

    let mut bits = 0u64;
    for (i, &c) in kept.iter().enumerate().skip(1) {
        if c > median {
            bits |= 1u64 << i;
        }
    }
    Fingerprint { bits, spread }
}

/// Area-average resize to `w × h`.
///
/// Box filtering, not nearest-neighbour: this feeds a frequency transform, and
/// point-sampling a 1280-wide frame down to 32 columns would alias detail into
/// exactly the low frequencies the hash reads. The QR path wants the opposite
/// (see `LumaFrame::upscale_nearest`) — different question, different filter.
fn box_resize(frame: &LumaFrame, w: usize, h: usize) -> Vec<f32> {
    let (fw, fh) = (frame.width as usize, frame.height as usize);
    let mut out = vec![0f32; w * h];
    if fw == 0 || fh == 0 {
        return out;
    }
    for y in 0..h {
        let y0 = y * fh / h;
        let y1 = (((y + 1) * fh).div_ceil(h)).min(fh).max(y0 + 1);
        for x in 0..w {
            let x0 = x * fw / w;
            let x1 = (((x + 1) * fw).div_ceil(w)).min(fw).max(x0 + 1);
            let mut sum = 0u32;
            let mut n = 0u32;
            for sy in y0..y1 {
                let row = sy * fw;
                for sx in x0..x1 {
                    sum += frame.data[row + sx] as u32;
                    n += 1;
                }
            }
            out[y * w + x] = sum as f32 / n.max(1) as f32;
        }
    }
    out
}

/// Separable 2-D DCT-II, computed directly.
///
/// O(n³) for an n×n block, which at n=32 is about 65k multiply-adds per frame —
/// negligible next to decoding the frame, and worth far more than the speed a
/// fast transform would buy: this is a short, checkable definition rather than
/// a butterfly nobody reviewing the tool would read.
fn dct2d(input: &[f32], n: usize) -> Vec<f32> {
    let cos_table = cos_table(n);

    // Rows, then columns.
    let mut rows = vec![0f32; n * n];
    for y in 0..n {
        for u in 0..n {
            let mut acc = 0f32;
            for x in 0..n {
                acc += input[y * n + x] * cos_table[x * n + u];
            }
            rows[y * n + u] = acc * alpha(u, n);
        }
    }
    let mut out = vec![0f32; n * n];
    for u in 0..n {
        for v in 0..n {
            let mut acc = 0f32;
            for y in 0..n {
                acc += rows[y * n + u] * cos_table[y * n + v];
            }
            out[v * n + u] = acc * alpha(v, n);
        }
    }
    out
}

fn alpha(k: usize, n: usize) -> f32 {
    if k == 0 {
        (1.0 / n as f32).sqrt()
    } else {
        (2.0 / n as f32).sqrt()
    }
}

/// `cos((2x+1) k π / 2n)` for every `(x, k)`, built once per call.
fn cos_table(n: usize) -> Vec<f32> {
    let mut t = vec![0f32; n * n];
    for x in 0..n {
        for k in 0..n {
            t[x * n + k] =
                (((2 * x + 1) as f32) * (k as f32) * std::f32::consts::PI / (2.0 * n as f32)).cos();
        }
    }
    t
}

fn median_of(v: &mut [f32]) -> f32 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn mean_abs_deviation(v: &[f32], centre: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().map(|c| (c - centre).abs()).sum::<f32>() / v.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_from<F: Fn(u32, u32) -> u8>(w: u32, h: u32, f: F) -> LumaFrame {
        let mut data = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                data.push(f(x, y));
            }
        }
        LumaFrame::new(w, h, data, 0, 0).unwrap()
    }

    /// Diagonal stripes defined in NORMALISED coordinates, so the same call at
    /// two resolutions is the same picture. Defining a test pattern in pixels
    /// makes a rescale a genuinely different image, and the test then measures
    /// the fixture rather than the hash.
    fn diagonal(w: u32, h: u32, phase: u32) -> LumaFrame {
        frame_from(w, h, move |x, y| {
            let u = x as f32 / w as f32 + y as f32 / h as f32 + phase as f32 / 16.0;
            if (u * 8.0) as u32 % 2 == 0 {
                30
            } else {
                230
            }
        })
    }

    #[test]
    fn identical_frames_hash_identically() {
        let a = diagonal(320, 240, 0);
        let b = diagonal(320, 240, 0);
        assert_eq!(fingerprint(&a).distance(fingerprint(&b)), 0);
    }

    #[test]
    fn a_global_brightness_shift_changes_nothing() {
        // The DC term is dropped for exactly this reason: a copy graded a
        // little brighter is the same picture.
        let a = frame_from(160, 120, |x, y| ((x * 3 + y * 5) % 200) as u8);
        let b = frame_from(160, 120, |x, y| (((x * 3 + y * 5) % 200) + 40) as u8);
        assert_eq!(fingerprint(&a).distance(fingerprint(&b)), 0);
    }

    #[test]
    fn rescaling_moves_the_hash_very_little() {
        // A platform re-encode at a different resolution must still match.
        let big = diagonal(640, 480, 0);
        let small = diagonal(320, 240, 0);
        let d = fingerprint(&big).distance(fingerprint(&small));
        // Hard-edged synthetic diagonals are the worst case for this: a 2×
        // rescale aliases every stripe boundary at once, which real footage
        // never does. The bound here says "a rescale must not scramble the
        // hash"; the threshold the alignment actually uses is measured on real
        // footage, not on this, and lives in `align.rs`.
        assert!(d <= 10, "rescale moved the hash by {d} bits");
    }

    #[test]
    fn different_pictures_are_far_apart() {
        let a = diagonal(320, 240, 0);
        let b = frame_from(320, 240, |x, y| ((x * x + y * 3) % 251) as u8);
        let d = fingerprint(&a).distance(fingerprint(&b));
        assert!(d >= 12, "unrelated pictures only {d} bits apart");
    }

    #[test]
    fn a_frame_with_no_picture_is_flagged() {
        // Only a frame with nothing in it. A high-contrast frame that happens
        // to be uninformative is NOT caught here and is not meant to be.
        let flat = frame_from(160, 120, |_, _| 128);
        let fp = fingerprint(&flat);
        assert!(fp.is_low_variance(), "spread {}", fp.spread);

        let textured = diagonal(160, 120, 0);
        assert!(!fingerprint(&textured).is_low_variance());
    }

    #[test]
    fn the_dc_slot_is_never_set() {
        // Bit 0 is a placeholder so bit positions match the coefficient grid.
        // If it ever carried a value it would make brightness count again.
        for phase in 0..8 {
            assert_eq!(fingerprint(&diagonal(64, 64, phase)).bits & 1, 0);
        }
    }

    #[test]
    fn the_dct_matches_its_definition_on_a_known_input() {
        // A constant block has all its energy in the DC term and nothing
        // anywhere else. Checks the normalisation factors, which are the part
        // of a hand-written DCT that goes wrong silently.
        let n = 8;
        let flat = vec![100f32; n * n];
        let c = dct2d(&flat, n);
        assert!((c[0] - 100.0 * n as f32).abs() < 0.01, "DC was {}", c[0]);
        for (i, v) in c.iter().enumerate().skip(1) {
            assert!(v.abs() < 0.01, "coefficient {i} should be 0, was {v}");
        }
    }

    #[test]
    fn box_resize_averages_rather_than_samples() {
        // A checkerboard averages to a uniform grey. Point-sampling would
        // return one colour or the other and alias the pattern into the low
        // frequencies the hash reads.
        let check = frame_from(64, 64, |x, y| if (x + y) % 2 == 0 { 0 } else { 255 });
        let small = box_resize(&check, 4, 4);
        for v in small {
            assert!((v - 127.5).abs() < 1.0, "resized to {v}, expected ~127.5");
        }
    }
}
