//! The machine-readable strip burned in below the overlay text.
//!
//! Format and rationale live in `BURN_IN_BIT_ROW.md`; this is the reference
//! implementation of both ends of it. The encoder exists so the phone
//! implementations have vectors to match and so the reader can be tested
//! against something other than itself.
//!
//! **Not a security feature.** The strip is unsigned pixels, as forgeable as
//! the text above it. It buys legibility: the text reader manages half the
//! frames and cannot tell a misread from a good one, while this is read at
//! nearly every frame and a corrupted read fails its checksum rather than
//! returning a wrong number. A forged counter still points at a frame of the
//! original, and the pictures still decide.

use crate::frame::LumaFrame;

/// Geometry, in the pixels the phone composes at. Mirrors the constants in
/// `FrameCompositor.swift` and `GlRotator.kt`; a divergence is a bug in one of
/// the three and the test vectors are what catch it.
pub mod native {
    /// Overlay band height once the strip is included.
    pub const BAND_HEIGHT: u32 = 98;
    /// First and last row of the strip, inclusive.
    ///
    /// At the very TOP of the frame, above everything else. It was below the
    /// three text lines, which worked but read badly — and putting it first
    /// buys more than tidiness: with nothing sharing its rows it can run the
    /// full width instead of starting after the QR, so every cell is wider and
    /// survives more of what a re-encode does to it.
    pub const ROW_TOP: u32 = 0;
    pub const ROW_BOTTOM: u32 = 11;
    /// Left edge. Symmetric with the right margin now that the QR has moved
    /// below the strip.
    pub const LEFT: u32 = 12;
    /// Right margin, the same the text uses.
    pub const PAD_X: u32 = 12;
}

/// The strip must sit inside the band, clear of line 3's descenders and clear
/// of the QR. Checked at COMPILE time rather than in a test: these are the
/// numbers three implementations have to agree on, and a build that cannot
/// happen is a stronger guarantee than a test someone can skip.
const _: () = {
    assert!(native::ROW_TOP < native::ROW_BOTTOM);
    assert!(native::ROW_BOTTOM < native::BAND_HEIGHT);
    // The strip is first; everything else starts below it, with a gap.
    assert!(crate::overlay::native::QR_MARGIN_Y > native::ROW_BOTTOM);
    assert!(crate::overlay::native::LINE_BASELINES[0] > native::ROW_BOTTOM);
    // Line 3's descenders must still land inside the band.
    assert!(crate::overlay::native::LINE_BASELINES[2] + 6 <= native::BAND_HEIGHT);
};

pub const CELLS: usize = 76;
/// Framing. Both alternate, so a reader recovers the cell pitch from them
/// without being told it — a transition every cell is a clock.
///
/// The stop is the START's complement rather than its mirror, and that is not
/// cosmetic: the first choice was `1101010101`, whose reverse is exactly the
/// start pattern, so a strip read right-to-left would have framed perfectly and
/// yielded a reversed payload. A test caught it before either phone had drawn
/// one.
pub const START: [bool; 10] = [
    true, false, true, false, true, false, true, false, true, true,
];
pub const STOP: [bool; 10] = [
    false, true, false, true, false, true, false, true, false, false,
];
const TAG_BITS: usize = 16;
const COUNTER_BITS: usize = 24;
const CRC_BITS: usize = 16;

/// Counter values wrap here. 24 bits is 155 hours at 30 fps.
pub const COUNTER_MODULUS: u64 = 1 << COUNTER_BITS;

/// CRC-16/CCITT-FALSE: polynomial 0x1021, init 0xFFFF, no reflection, no final
/// xor. Chosen for being unambiguous to reimplement — the same eight lines in
/// Swift, Kotlin and Rust, with no table and no endianness question.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The two payload bytes that distinguish one recording from another.
///
/// The first two bytes of SHA-256 over the short id. Not an identifier: it
/// tells recordings apart, it does not name them, and two bytes could not
/// name them even if that were wanted.
pub fn session_tag(short_id: Option<&str>) -> u16 {
    match short_id {
        None => 0,
        Some(s) => {
            let h = sha256(s.as_bytes());
            ((h[0] as u16) << 8) | h[1] as u16
        }
    }
}

/// The 76 cells for one frame.
pub fn encode(tag: u16, counter: u64) -> [bool; CELLS] {
    let counter = (counter % COUNTER_MODULUS) as u32;
    let payload = [
        (tag >> 8) as u8,
        tag as u8,
        (counter >> 16) as u8,
        (counter >> 8) as u8,
        counter as u8,
    ];
    let crc = crc16(&payload);

    let mut cells = [false; CELLS];
    let mut i = 0;
    for b in START {
        cells[i] = b;
        i += 1;
    }
    for k in (0..TAG_BITS).rev() {
        cells[i] = tag >> k & 1 == 1;
        i += 1;
    }
    for k in (0..COUNTER_BITS).rev() {
        cells[i] = counter >> k & 1 == 1;
        i += 1;
    }
    for k in (0..CRC_BITS).rev() {
        cells[i] = crc >> k & 1 == 1;
        i += 1;
    }
    for b in STOP {
        cells[i] = b;
        i += 1;
    }
    debug_assert_eq!(i, CELLS);
    cells
}

/// What one frame's strip carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowReading {
    pub session_tag: u16,
    pub counter: u64,
}

/// Why a strip could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowRefusal {
    /// The rows carry no strip: too little contrast to threshold.
    NoStrip,
    /// The start pattern was not found.
    NoStart,
    /// The stop pattern was not where the cell count says it should be.
    BadStop,
    /// Framing found, checksum failed. The read is discarded, never guessed.
    BadCrc,
}

/// Decode the 76 cells, checking framing and checksum.
pub fn decode(cells: &[bool]) -> Result<RowReading, RowRefusal> {
    if cells.len() != CELLS {
        return Err(RowRefusal::NoStart);
    }
    if cells[..10] != START {
        return Err(RowRefusal::NoStart);
    }
    if cells[66..] != STOP {
        return Err(RowRefusal::BadStop);
    }
    let bits = |from: usize, n: usize| -> u64 {
        (0..n).fold(0u64, |acc, k| acc << 1 | cells[from + k] as u64)
    };
    let tag = bits(10, TAG_BITS) as u16;
    let counter = bits(26, COUNTER_BITS);
    let crc = bits(50, CRC_BITS) as u16;
    let payload = [
        (tag >> 8) as u8,
        tag as u8,
        (counter >> 16) as u8,
        (counter >> 8) as u8,
        counter as u8,
    ];
    if crc16(&payload) != crc {
        return Err(RowRefusal::BadCrc);
    }
    Ok(RowReading {
        session_tag: tag,
        counter,
    })
}

/// Read the strip out of a decoded frame.
///
/// `scale` is the frame's width over the original's, and places the strip's
/// ROWS. It does not place its columns: the pitch is measured from the start
/// pattern's own transitions, so a copy at any width reads without being told
/// which.
///
/// That is what the format promised and what the first implementation did not
/// do — it computed the pitch by integer division of the geometry it assumed,
/// and on a copy scaled to half width the true pitch was 4.5 while the reader
/// stepped by 4. By the last cell it was thirty-eight pixels adrift and found
/// nothing at all. Re-scaling is the ordinary fate of a copy, so this is the
/// case that mattered most.
pub fn read(frame: &LumaFrame, scale: f32) -> Result<RowReading, RowRefusal> {
    let s = |v: u32| ((v as f32) * scale).round() as u32;
    let top = s(native::ROW_TOP);
    let bottom = s(native::ROW_BOTTOM).min(frame.height.saturating_sub(1));
    if bottom <= top || frame.width < CELLS as u32 {
        return Err(RowRefusal::NoStrip);
    }

    // One value per column, from the middle rows only so the strip's own top
    // and bottom edges cannot drag a column either way.
    let y0 = top + (bottom - top) / 4;
    let y1 = bottom - (bottom - top) / 4;
    let col: Vec<u16> = (0..frame.width)
        .map(|x| {
            let sum: u32 = (y0..=y1).map(|y| frame.at(x, y) as u32).sum();
            (sum / (y1 - y0 + 1).max(1)) as u16
        })
        .collect();

    let lo = *col.iter().min().unwrap_or(&0);
    let hi = *col.iter().max().unwrap_or(&0);
    // Solid black against solid white. Anything flatter is not a strip, and
    // guessing at it would invent cells out of noise.
    if hi.saturating_sub(lo) < 60 {
        return Err(RowRefusal::NoStrip);
    }
    let mid = (lo + hi) / 2;
    let on: Vec<bool> = col.iter().map(|v| *v > mid).collect();

    let candidates = find_clock(&on).ok_or(RowRefusal::NoStart)?;

    // Try each candidate clock and let the CHECKSUM decide.
    //
    // Two estimates of the pitch are available — one from the eight cells at
    // the start, one measured across the whole strip against the stop pattern
    // — and neither wins everywhere: the short baseline is exact at full size
    // and drifts on a rescaled copy, the long one fixes the rescale and
    // occasionally locks onto the wrong run at full size. Picking between them
    // by any rule would be guessing. Trying both costs a few hundred
    // comparisons and the CRC refuses whichever is wrong, which is the job it
    // was put there to do.
    let mut last = RowRefusal::BadStop;
    for (start_x, pitch) in candidates {
        match sample_cells(&on, start_x, pitch).and_then(|c| decode(&c)) {
            Ok(r) => return Ok(r),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Read the 76 cells at one candidate clock.
fn sample_cells(on: &[bool], start_x: f32, pitch: f32) -> Result<[bool; CELLS], RowRefusal> {
    let mut cells = [false; CELLS];
    for (k, cell) in cells.iter_mut().enumerate() {
        // The middle half of each cell: the edges are where a rescale smears
        // one cell into the next.
        let a = start_x + pitch * (k as f32 + 0.25);
        let b = start_x + pitch * (k as f32 + 0.75);
        let (ia, ib) = (a.round() as i64, b.round().max(a.round() + 1.0) as i64);
        let (mut lit, mut seen) = (0i64, 0i64);
        for x in ia..ib {
            if x < 0 || x as usize >= on.len() {
                continue;
            }
            seen += 1;
            if on[x as usize] {
                lit += 1;
            }
        }
        if seen == 0 {
            return Err(RowRefusal::BadStop);
        }
        *cell = lit * 2 > seen;
    }
    Ok(cells)
}

/// Recover where the strip begins and how wide one cell is, from the start
/// pattern's own transitions.
///
/// The start is `1 0 1 0 1 0 1 0 1 1`: eight runs of one cell each,
/// alternating and opening lit, then a run of two because cells 8 and 9 are
/// both lit. That double run is the distinctive part, and it is what this
/// anchors on.
///
/// Anchoring on the FIRST run instead does not work, and the reason is
/// mundane: twelve columns of picture sit to the left of the strip, and when
/// they happen to threshold lit they merge into cell 0 and make it wider than
/// a cell. Anchoring at the far end of the run of alternations and stepping
/// back eight cells is immune to that.
/// Pitch measured across the whole strip, using the alternations at the far
/// end as the second reference point.
///
/// The stop pattern starts at cell 66 with `0 1 0 1 …`, so the first LIT run of
/// that group is cell 67. Finding it and dividing the distance by 67 gives a
/// pitch averaged over the entire strip.
fn refine_from_stop(runs: &[(usize, usize, bool)], start: f32, pitch: f32) -> Option<f32> {
    let want = start + pitch * 67.0;
    let tolerance = pitch * 2.0;
    let hit = runs
        .iter()
        .filter(|(_, len, lit)| *lit && (*len as f32) < pitch * 1.6)
        .min_by(|a, b| {
            let da = (a.0 as f32 - want).abs();
            let db = (b.0 as f32 - want).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })?;
    if (hit.0 as f32 - want).abs() > tolerance {
        return None;
    }
    let refined = (hit.0 as f32 - start) / 67.0;
    // Only if it is a small correction: a big one means the run found is not
    // the cell believed, and trusting it would be worse than not refining.
    (refined > pitch * 0.8 && refined < pitch * 1.2).then_some(refined)
}

fn find_clock(on: &[bool]) -> Option<Vec<(f32, f32)>> {
    let mut runs: Vec<(usize, usize, bool)> = Vec::new(); // start, len, value
    let mut i = 0usize;
    while i < on.len() {
        let v = on[i];
        let j = (i..on.len()).take_while(|&k| on[k] == v).count() + i;
        runs.push((i, j - i, v));
        i = j;
    }

    /// Single-cell runs needed before the double. Six rather than eight so a
    /// contaminated first cell costs nothing.
    const MIN_ALTERNATIONS: usize = 6;

    for d in 0..runs.len() {
        // The double: lit, and preceded by a decent run of alternations.
        if !runs[d].2 || d < MIN_ALTERNATIONS {
            continue;
        }
        // Take the alternations immediately before it, back to front, while
        // their lengths agree with the FIRST one collected. Comparing each to
        // its neighbour instead lets the length drift a little at every step
        // and eventually accept a run that is nothing like a cell.
        let reference = runs[d - 1].1 as f32;
        let mut lens: Vec<f32> = Vec::new();
        let mut k = d;
        while k > 0 {
            let len = runs[k - 1].1 as f32;
            if (len - reference).abs() > reference * 0.4 {
                break;
            }
            lens.insert(0, len);
            k -= 1;
            if lens.len() >= 12 {
                break;
            }
        }
        if lens.len() < MIN_ALTERNATIONS {
            continue;
        }
        // Two estimates of the pitch, both offered to the checksum.
        //
        // Run lengths are whole pixels, so the MEDIAN of them is exact when
        // the pitch is a whole number and half a pixel out when it is not — on
        // a copy at half width the true pitch is 4.5, and half a pixel per cell
        // is eight cells of drift across the strip.
        //
        // The SPAN the alternations cover averages that rounding away, but its
        // first run is the one whose left edge bleeds into the picture beside
        // it: measured, cell 0 began at column 11 instead of 12 and ran ten
        // pixels instead of nine, which stretched the estimate enough to lose
        // a cell by the end. So the span is measured over the INTERIOR runs,
        // skipping the first.
        let median = {
            let mut v = lens.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let interior = if lens.len() >= 3 {
            (runs[d].0 as f32 - runs[k + 1].0 as f32) / (lens.len() - 1) as f32
        } else {
            median
        };
        let pitch = interior;
        if pitch < 1.0 || median < 1.0 {
            continue;
        }
        // The run after the alternations must be WIDER than one cell — that is
        // what separates a start pattern from any other stripe in the picture.
        //
        // Wider, with no upper bound, and the reason is in the format: the
        // start ends `1 1`, so cells 8 and 9 are both lit and merge. If the
        // session tag's top bit is also 1 — half the time — cell 10 joins them
        // and the run is three cells, not two. A version of this that demanded
        // "about two" refused every such frame, which on a real recording
        // meant every frame.
        let dl = runs[d].1 as f32;
        if dl < pitch * 1.4 {
            continue;
        }
        // Cells 8 and 9 ARE that run's first two, so cell 0 begins eight cells
        // before it. Measured from its own left edge rather than accumulated
        // from the first run, so no rounding piles up.
        let start = runs[d].0 as f32 - pitch * 8.0;
        if start < -pitch {
            continue;
        }
        let start = start.max(0.0);

        // Refine against the far end. The stop pattern is `0 1 0 1 0 1 0 1 0 0`
        // — alternating too — so the strip carries a clock at BOTH ends, and
        // the distance between them is sixty-six cells. Measured over that
        // baseline the pitch is far more precise than over the eight cells at
        // one end, which is what the last cells of a rescaled strip need.
        // Every candidate clock, cheapest and most likely first. The CRC picks.
        let mut out: Vec<(f32, f32)> = vec![(start, pitch)];
        if (median - pitch).abs() > 0.01 {
            out.push((runs[d].0 as f32 - median * 8.0, median));
        }
        if let Some(refined) = refine_from_stop(&runs, start, pitch) {
            out.push((start, refined));
        }
        // A last spread of near misses. Across seventy-six cells a pitch off
        // by half a percent walks a third of a cell, which is enough to lose
        // the last few on a rescaled copy; the exact value is not recoverable
        // from eight runs of whole pixels, so a handful of neighbours is
        // offered instead. They cost a few hundred comparisons each and the
        // checksum refuses every wrong one — the alternative is a reader that
        // is right about the pitch and wrong about the frame.
        for k in [-1.0f32, 1.0, -2.0, 2.0] {
            out.push((start, pitch * (1.0 + k * 0.004)));
        }
        return Some(out);
    }
    None
}

// ---------------------------------------------------------------------------
// SHA-256, for the session tag only.
// ---------------------------------------------------------------------------

/// Written out rather than pulled in: the core takes no dependency it does not
/// need, this one is needed for two bytes, and the phone implementations use
/// their platform's own. Straight from FIPS 180-4.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (i, v) in [a, b, c, d, e, f, g, hh].into_iter().enumerate() {
            h[i] = h[i].wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_published_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn crc_matches_the_published_check_value() {
        // CRC-16/CCITT-FALSE over "123456789" is 0x29B1, in every catalogue.
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn a_row_round_trips() {
        for (tag, counter) in [
            (0u16, 0u64),
            (0xABCD, 1),
            (0x1234, 16_777_215),
            (0xFFFF, 999_999),
        ] {
            let cells = encode(tag, counter);
            assert_eq!(
                decode(&cells),
                Ok(RowReading {
                    session_tag: tag,
                    counter
                })
            );
        }
    }

    #[test]
    fn the_counter_wraps_rather_than_overflowing_its_field() {
        let cells = encode(1, COUNTER_MODULUS + 7);
        assert_eq!(decode(&cells).unwrap().counter, 7);
    }

    #[test]
    fn a_single_flipped_cell_is_refused_not_misread() {
        // The property the checksum is there for: a corrupted strip yields no
        // counter rather than a wrong one, which would point confidently at
        // the wrong frame of the original.
        let good = encode(0x4A2B, 12_345);
        for i in 10..66 {
            let mut bad = good;
            bad[i] = !bad[i];
            assert_eq!(
                decode(&bad),
                Err(RowRefusal::BadCrc),
                "flipping cell {i} was not caught"
            );
        }
    }

    #[test]
    fn broken_framing_is_named_for_what_broke() {
        let mut c = encode(1, 1);
        c[0] = !c[0];
        assert_eq!(decode(&c), Err(RowRefusal::NoStart));

        let mut c = encode(1, 1);
        c[75] = !c[75];
        assert_eq!(decode(&c), Err(RowRefusal::BadStop));
    }

    #[test]
    fn start_and_stop_cannot_be_confused_with_each_other() {
        // So a strip cannot be read backwards, and so a reader that finds one
        // knows which end it is at.
        assert_ne!(START, STOP);
        let mut reversed = STOP;
        reversed.reverse();
        assert_ne!(
            START, reversed,
            "a strip read backwards would frame perfectly"
        );
        let mut reversed = START;
        reversed.reverse();
        assert_ne!(STOP, reversed);
    }

    /// Draw a strip the way the phone does, so the reader is tested against
    /// pixels rather than against the encoder's own array.
    fn frame_with_row(width: u32, height: u32, tag: u16, counter: u64, scale: f32) -> LumaFrame {
        let cells = encode(tag, counter);
        let s = |v: u32| ((v as f32) * scale).round() as u32;
        let mut data = vec![90u8; (width * height) as usize];
        let (top, bottom) = (s(native::ROW_TOP), s(native::ROW_BOTTOM));
        let left = s(native::LEFT);
        let right = width - s(native::PAD_X);
        let pitch = (right - left) / CELLS as u32;
        for (k, on) in cells.iter().enumerate() {
            let x0 = left + k as u32 * pitch;
            for y in top..=bottom {
                for x in x0..x0 + pitch {
                    data[(y * width + x) as usize] = if *on { 255 } else { 0 };
                }
            }
        }
        LumaFrame::new(width, height, data, 0, 0).unwrap()
    }

    #[test]
    fn the_reader_finds_the_row_in_a_frame() {
        let f = frame_with_row(1280, 720, 0x4A2B, 12_345, 1.0);
        assert_eq!(
            read(&f, 1.0),
            Ok(RowReading {
                session_tag: 0x4A2B,
                counter: 12_345
            })
        );
    }

    #[test]
    fn the_reader_survives_a_frame_at_half_size() {
        let f = frame_with_row(640, 360, 0x4A2B, 12_345, 0.5);
        assert_eq!(
            read(&f, 0.5),
            Ok(RowReading {
                session_tag: 0x4A2B,
                counter: 12_345
            })
        );
    }

    /// Draw the strip at an arbitrary pixel pitch, which is what a rescaled
    /// copy carries: the phone draws whole-pixel cells, and a platform then
    /// scales the frame by whatever it likes.
    fn frame_with_fractional_pitch(width: u32, tag: u16, counter: u64, pitch: f32) -> LumaFrame {
        let cells = encode(tag, counter);
        let height = 200u32;
        let mut data = vec![90u8; (width * height) as usize];
        let left = 12.0f32;
        for (k, on) in cells.iter().enumerate() {
            let x0 = (left + k as f32 * pitch).round() as u32;
            let x1 = (left + (k + 1) as f32 * pitch).round().min(width as f32) as u32;
            for y in 0..=5u32 {
                for x in x0..x1 {
                    data[(y * width + x) as usize] = if *on { 255 } else { 0 };
                }
            }
        }
        LumaFrame::new(width, height, data, 0, 0).unwrap()
    }

    #[test]
    fn a_fractional_pitch_is_recovered_from_the_strip_itself() {
        // The failure this covers: the reader used to compute the pitch by
        // integer division of the geometry it assumed. On a copy at half width
        // the true pitch was 4.5 and it stepped by 4 — thirty-eight pixels
        // adrift by the last cell, and nothing read at all. Rescaling is the
        // ordinary fate of a copy, so this is the case that mattered most.
        for pitch in [4.5f32, 5.5, 6.25, 7.75, 9.0, 11.3] {
            let width = (12.0 + pitch * CELLS as f32).ceil() as u32 + 12;
            let f = frame_with_fractional_pitch(width, 0x4A2B, 12_345, pitch);
            assert_eq!(
                read(&f, 0.5),
                Ok(RowReading {
                    session_tag: 0x4A2B,
                    counter: 12_345
                }),
                "pitch {pitch} was not recovered"
            );
        }
    }

    #[test]
    fn a_frame_with_no_strip_is_refused_rather_than_read() {
        let flat = LumaFrame::new(1280, 720, vec![128; 1280 * 720], 0, 0).unwrap();
        assert_eq!(read(&flat, 1.0), Err(RowRefusal::NoStrip));
    }

    #[test]
    fn two_recordings_get_different_tags() {
        let a = session_tag(Some("NElG7hqbvvV8"));
        let b = session_tag(Some("IF9kZhHt0TJA"));
        assert_ne!(a, b);
        assert_eq!(session_tag(None), 0);
    }
}
