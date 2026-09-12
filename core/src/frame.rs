//! The unit the core works on: one already-decoded frame, 8-bit luma.
//!
//! Everything upstream of this type — containers, codecs, colour conversion —
//! is the adapter's problem (`cli` shells out to a decoder, `wasm` uses the
//! browser's WebCodecs). The core never learns what a codec is, which is what
//! makes it testable against frames drawn by hand in a unit test.
//!
//! Luma only, deliberately. Every measurement this tool makes — QR reading,
//! perceptual hashing, the structure of an image difference — is a luminance
//! measurement. Carrying chroma would triple the memory for a signal none of
//! the analysis reads.

/// One decoded frame plus where it sat in its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LumaFrame {
    pub width: u32,
    pub height: u32,
    /// `width * height` bytes, row-major, top-left origin.
    pub data: Vec<u8>,
    /// Position in the decoded sequence, counted from 0. This is OUR index —
    /// it is not the `f=` counter burned into a Forsheur frame, and the two
    /// must never be conflated: the burned counter comes from the capturing
    /// phone and may be absent, cropped, or forged.
    pub index: u64,
    /// Presentation timestamp in microseconds from the start of the source.
    pub pts_us: i64,
}

/// A rectangle in frame coordinates, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }

    /// Clamp to the bounds of a `width × height` frame. Returns `None` when
    /// the rectangle lands entirely outside, so callers cannot silently work
    /// on an empty crop.
    pub fn clamped_to(self, width: u32, height: u32) -> Option<Rect> {
        if self.x >= width || self.y >= height {
            return None;
        }
        let w = self.w.min(width - self.x);
        let h = self.h.min(height - self.y);
        if w == 0 || h == 0 {
            return None;
        }
        Some(Rect {
            x: self.x,
            y: self.y,
            w,
            h,
        })
    }
}

impl LumaFrame {
    pub fn new(
        width: u32,
        height: u32,
        data: Vec<u8>,
        index: u64,
        pts_us: i64,
    ) -> Result<Self, FrameError> {
        let want = (width as usize)
            .checked_mul(height as usize)
            .ok_or(FrameError::TooLarge)?;
        if data.len() != want {
            return Err(FrameError::SizeMismatch {
                expected: want,
                got: data.len(),
            });
        }
        Ok(LumaFrame {
            width,
            height,
            data,
            index,
            pts_us,
        })
    }

    #[inline]
    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Copy out a sub-rectangle. Returns `None` if the rectangle does not
    /// overlap the frame.
    pub fn crop(&self, r: Rect) -> Option<LumaFrame> {
        let r = r.clamped_to(self.width, self.height)?;
        let mut out = Vec::with_capacity((r.w as usize) * (r.h as usize));
        for y in r.y..r.y + r.h {
            let row = (y as usize) * (self.width as usize);
            out.extend_from_slice(&self.data[row + r.x as usize..row + (r.x + r.w) as usize]);
        }
        Some(LumaFrame {
            width: r.w,
            height: r.h,
            data: out,
            index: self.index,
            pts_us: self.pts_us,
        })
    }

    /// Integer nearest-neighbour upscale. Nearest, not bilinear, and that is
    /// not laziness: this feeds a QR reader, where smoothing across a module
    /// boundary is exactly the information we are trying to preserve.
    pub fn upscale_nearest(&self, factor: u32) -> LumaFrame {
        assert!(factor >= 1, "upscale factor must be >= 1");
        if factor == 1 {
            return self.clone();
        }
        let (w, h) = (self.width * factor, self.height * factor);
        let mut out = vec![0u8; (w as usize) * (h as usize)];
        for y in 0..h {
            let sy = y / factor;
            let srow = (sy as usize) * (self.width as usize);
            let drow = (y as usize) * (w as usize);
            for x in 0..w {
                out[drow + x as usize] = self.data[srow + (x / factor) as usize];
            }
        }
        LumaFrame {
            width: w,
            height: h,
            data: out,
            index: self.index,
            pts_us: self.pts_us,
        }
    }

    /// Surround the frame with a constant-value border.
    pub fn pad(&self, border: u32, value: u8) -> LumaFrame {
        let (w, h) = (self.width + 2 * border, self.height + 2 * border);
        let mut out = vec![value; (w as usize) * (h as usize)];
        for y in 0..self.height {
            let srow = (y as usize) * (self.width as usize);
            let drow = ((y + border) as usize) * (w as usize) + border as usize;
            out[drow..drow + self.width as usize]
                .copy_from_slice(&self.data[srow..srow + self.width as usize]);
        }
        LumaFrame {
            width: w,
            height: h,
            data: out,
            index: self.index,
            pts_us: self.pts_us,
        }
    }

    /// Otsu's method: the threshold that minimises intra-class variance.
    ///
    /// Published in 1979 and unchanged since; we do not invent a thresholding
    /// rule any more than we invent a hash. Returns the chosen threshold so a
    /// caller can report it, because a reader who disagrees with a binarisation
    /// is entitled to see the number that produced it.
    pub fn otsu_threshold(&self) -> u8 {
        let mut hist = [0u32; 256];
        for &p in &self.data {
            hist[p as usize] += 1;
        }
        let total = self.data.len() as f64;
        let sum: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
        let (mut w_bg, mut sum_bg, mut best_var, mut best_t) = (0f64, 0f64, -1f64, 0u8);
        for (t, &count) in hist.iter().enumerate() {
            w_bg += count as f64;
            if w_bg == 0.0 {
                continue;
            }
            let w_fg = total - w_bg;
            if w_fg == 0.0 {
                break;
            }
            sum_bg += t as f64 * count as f64;
            let mean_bg = sum_bg / w_bg;
            let mean_fg = (sum - sum_bg) / w_fg;
            let var = w_bg * w_fg * (mean_bg - mean_fg) * (mean_bg - mean_fg);
            if var > best_var {
                best_var = var;
                best_t = t as u8;
            }
        }
        best_t
    }

    /// Binarise at `threshold`: `<= threshold` becomes 0, the rest 255.
    pub fn binarize(&self, threshold: u8) -> LumaFrame {
        let data = self
            .data
            .iter()
            .map(|&p| if p <= threshold { 0u8 } else { 255u8 })
            .collect();
        LumaFrame {
            width: self.width,
            height: self.height,
            data,
            index: self.index,
            pts_us: self.pts_us,
        }
    }
}

impl LumaFrame {
    /// Adaptive threshold: each pixel against the mean of the `block × block`
    /// box around it, minus `bias`.
    ///
    /// This exists because Otsu is the wrong tool for the actual picture. The
    /// burn-in QR is composited at 80 % opacity over the scene, so its "white"
    /// is the scene lightened and its "black" is the scene darkened — and when
    /// the scene behind it carries a gradient, sky into roofline say, one
    /// global threshold cuts the code in half. Measured over 40 frames of a
    /// real preprod recording, a local threshold read the code in 15 of them
    /// where the best global one managed 12, and no single recipe read more
    /// than 15, which is why the sweep tries several.
    ///
    /// Computed through an integral image, so cost is independent of `block`.
    pub fn adaptive_threshold(&self, block: u32, bias: i32) -> LumaFrame {
        let (w, h) = (self.width as usize, self.height as usize);
        if w == 0 || h == 0 {
            return self.clone();
        }
        // Integral image, one row and column of zeros so every box lookup is
        // four unconditional reads.
        let mut sum = vec![0u64; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row_acc = 0u64;
            for x in 0..w {
                row_acc += self.data[y * w + x] as u64;
                sum[(y + 1) * (w + 1) + (x + 1)] = sum[y * (w + 1) + (x + 1)] + row_acc;
            }
        }
        let r = (block.max(3) / 2) as i64;
        let mut out = vec![0u8; w * h];
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let x0 = (x - r).max(0) as usize;
                let y0 = (y - r).max(0) as usize;
                let x1 = (x + r + 1).min(w as i64) as usize;
                let y1 = (y + r + 1).min(h as i64) as usize;
                let area = ((x1 - x0) * (y1 - y0)) as u64;
                let s = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0]
                    - sum[y0 * (w + 1) + x1]
                    - sum[y1 * (w + 1) + x0];
                let mean = (s / area.max(1)) as i32;
                let p = self.data[(y as usize) * w + (x as usize)] as i32;
                out[(y as usize) * w + (x as usize)] = if p > mean - bias { 255 } else { 0 };
            }
        }
        LumaFrame {
            width: self.width,
            height: self.height,
            data: out,
            index: self.index,
            pts_us: self.pts_us,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    SizeMismatch { expected: usize, got: usize },
    TooLarge,
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::SizeMismatch { expected, got } => {
                write!(f, "luma buffer is {got} bytes, expected {expected}")
            }
            FrameError::TooLarge => write!(f, "frame dimensions overflow usize"),
        }
    }
}

impl std::error::Error for FrameError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: u32, h: u32) -> LumaFrame {
        let data = (0..w * h).map(|i| (i % 256) as u8).collect();
        LumaFrame::new(w, h, data, 0, 0).unwrap()
    }

    #[test]
    fn rejects_wrong_sized_buffer() {
        assert_eq!(
            LumaFrame::new(4, 4, vec![0; 15], 0, 0).unwrap_err(),
            FrameError::SizeMismatch {
                expected: 16,
                got: 15
            }
        );
    }

    #[test]
    fn crop_takes_the_named_rectangle() {
        let f = ramp(8, 4);
        let c = f.crop(Rect::new(2, 1, 3, 2)).unwrap();
        assert_eq!((c.width, c.height), (3, 2));
        assert_eq!(
            c.data,
            vec![
                f.at(2, 1),
                f.at(3, 1),
                f.at(4, 1),
                f.at(2, 2),
                f.at(3, 2),
                f.at(4, 2)
            ]
        );
    }

    #[test]
    fn crop_clamps_at_the_edge_and_refuses_a_miss() {
        let f = ramp(8, 4);
        assert_eq!(f.crop(Rect::new(6, 0, 99, 2)).unwrap().width, 2);
        assert!(f.crop(Rect::new(8, 0, 4, 4)).is_none());
    }

    #[test]
    fn upscale_replicates_each_pixel() {
        let f = LumaFrame::new(2, 2, vec![0, 255, 128, 64], 0, 0).unwrap();
        let up = f.upscale_nearest(3);
        assert_eq!((up.width, up.height), (6, 6));
        assert_eq!(up.at(0, 0), 0);
        assert_eq!(up.at(2, 2), 0);
        assert_eq!(up.at(3, 0), 255);
        assert_eq!(up.at(0, 3), 128);
        assert_eq!(up.at(5, 5), 64);
    }

    #[test]
    fn pad_surrounds_without_touching_the_middle() {
        let f = LumaFrame::new(2, 2, vec![1, 2, 3, 4], 0, 0).unwrap();
        let p = f.pad(2, 255);
        assert_eq!((p.width, p.height), (6, 6));
        assert_eq!(p.at(0, 0), 255);
        assert_eq!(p.at(2, 2), 1);
        assert_eq!(p.at(3, 3), 4);
        assert_eq!(p.at(5, 5), 255);
    }

    #[test]
    fn adaptive_threshold_survives_a_gradient_that_defeats_a_global_one() {
        // A 64×64 field with a strong left-to-right ramp, and a small dark
        // square on the RIGHT that is still lighter than the ramp's left end.
        // A global threshold either swallows the square or blacks out the
        // whole left half; a local one sees the square.
        let (w, h) = (64u32, 64u32);
        let mut data = vec![0u8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                data[(y * w + x) as usize] = (x * 4).min(255) as u8;
            }
        }
        for y in 20..30 {
            for x in 48..58 {
                data[(y * w + x) as usize] = 150; // darker than its ~200 surroundings
            }
        }
        let f = LumaFrame::new(w, h, data, 0, 0).unwrap();

        let global = f.binarize(f.otsu_threshold());
        let local = f.adaptive_threshold(11, 4);

        // Local: the square is black, the ramp around it is not.
        assert_eq!(local.at(52, 25), 0, "local threshold missed the square");
        assert_eq!(local.at(30, 5), 255, "local threshold blacked out the ramp");
        // Global: the entire left half goes black, so the square is not what
        // distinguishes anything.
        assert_eq!(global.at(10, 10), 0);
    }

    #[test]
    fn adaptive_threshold_leaves_a_flat_field_alone() {
        let f = LumaFrame::new(16, 16, vec![120; 256], 0, 0).unwrap();
        // Every pixel equals its neighbourhood mean, so with a positive bias
        // everything is above `mean - bias` and nothing is spuriously black.
        assert!(f.adaptive_threshold(7, 4).data.iter().all(|&p| p == 255));
    }

    #[test]
    fn otsu_splits_a_two_peak_image_between_the_peaks() {
        // Half the pixels at 40, half at 200.
        let mut data = vec![40u8; 50];
        data.extend(std::iter::repeat_n(200u8, 50));
        let f = LumaFrame::new(10, 10, data, 0, 0).unwrap();
        let t = f.otsu_threshold();
        assert!(
            (40..200).contains(&(t as u32)),
            "threshold {t} not between the peaks"
        );
        let b = f.binarize(t);
        assert_eq!(b.data.iter().filter(|&&p| p == 0).count(), 50);
    }
}
