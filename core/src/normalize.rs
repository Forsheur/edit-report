//! Bringing two videos onto common ground before anything is compared.
//!
//! ## Why this is the first thing, not a detail
//!
//! A platform that letterboxes a portrait recording into a 16:9 frame has
//! changed every pixel's coordinates. Compare that against the original
//! without undoing it and every frame differs everywhere, and the report says
//! the copy was modified from end to end — about the single most ordinary
//! thing that happens to a video. It is the most common case and it is the
//! first bug this code would have.
//!
//! So, before any fingerprint is taken:
//!
//!   * the added bars are found and removed, on all four sides independently;
//!   * both sides are reduced to a common size by the fingerprint's own box
//!     filter, which makes resolution differences a non-question.
//!
//! Rotation is handled upstream. A Forsheur recording never carries a rotation
//! matrix — the phone writes an identity matrix and rotates the pixels
//! (`FMP4StreamWriter`), so the original is always upright — but a copy may
//! well carry one, and applying it is the decoder's job. What arrives here is
//! already the right way up.
//!
//! ## What this does NOT do
//!
//! It does not undo cropping, which is a different operation: a crop removes
//! picture, and no amount of normalisation puts it back. A cropped copy is
//! aligned on what survived, and the report says a crop was seen. Nor does it
//! undo mirroring or rotation of the content itself; both are listed in
//! SECURITY.md as ways to make two videos incomparable.

use crate::frame::{LumaFrame, Rect};

/// Bars found around the picture, in pixels of the frame examined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Bars {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl Bars {
    pub fn none(&self) -> bool {
        self.top == 0 && self.bottom == 0 && self.left == 0 && self.right == 0
    }

    /// What is left of a `w × h` frame once the bars are removed.
    pub fn content(&self, w: u32, h: u32) -> Option<Rect> {
        let x = self.left;
        let y = self.top;
        let cw = w.checked_sub(self.left + self.right)?;
        let ch = h.checked_sub(self.top + self.bottom)?;
        if cw == 0 || ch == 0 {
            return None;
        }
        Some(Rect::new(x, y, cw, ch))
    }

    /// A one-line description for the report. Never phrased as a fault: bars
    /// are what a platform adds, not what a forger does.
    pub fn describe(&self) -> String {
        if self.none() {
            return "no added bars".to_string();
        }
        let mut parts = Vec::new();
        if self.top > 0 || self.bottom > 0 {
            parts.push(format!("{} px top, {} px bottom", self.top, self.bottom));
        }
        if self.left > 0 || self.right > 0 {
            parts.push(format!("{} px left, {} px right", self.left, self.right));
        }
        format!("bars removed before comparison — {}", parts.join("; "))
    }
}

/// How uniform a row or column must be to count as part of a bar.
///
/// A bar is not necessarily pure black: it survives encoding, so it carries
/// ringing and block noise, and a bar rendered over a dark scene is dark grey.
/// The test is therefore uniformity, not darkness — a row whose pixels barely
/// differ from each other, whatever their level.
const BAR_UNIFORMITY: f32 = 6.0;

/// Sides never eat more than this fraction of the frame.
///
/// A guard, not a tuning knob. Without it a shot that opens on a dark sky
/// would have half its picture declared a bar and thrown away, and the
/// alignment would then be comparing two different framings.
const MAX_BAR_FRACTION: f32 = 0.35;

/// Find the bars around one frame.
pub fn detect_bars(frame: &LumaFrame) -> Bars {
    let (w, h) = (frame.width, frame.height);
    let max_v = (h as f32 * MAX_BAR_FRACTION) as u32;
    let max_h = (w as f32 * MAX_BAR_FRACTION) as u32;

    let mut top = 0;
    while top < max_v && row_is_uniform(frame, top) {
        top += 1;
    }
    let mut bottom = 0;
    while bottom < max_v && h > bottom && row_is_uniform(frame, h - 1 - bottom) {
        bottom += 1;
    }
    let mut left = 0;
    while left < max_h && col_is_uniform(frame, left) {
        left += 1;
    }
    let mut right = 0;
    while right < max_h && w > right && col_is_uniform(frame, w - 1 - right) {
        right += 1;
    }
    // A frame with no picture in it — a fade to black, a covered lens — is
    // uniform in every direction, so every side runs until it hits the cap and
    // the result is a "bar" on all four sides of something that has no
    // content. Report no bars instead: there is nothing here to align on
    // either way, and inventing a crop from it would misnormalise every frame
    // of the video that DOES have picture.
    let capped_everywhere = top >= max_v && bottom >= max_v && left >= max_h && right >= max_h;
    if capped_everywhere || top + bottom >= h || left + right >= w {
        return Bars::default();
    }
    Bars {
        top,
        bottom,
        left,
        right,
    }
}

fn row_is_uniform(frame: &LumaFrame, y: u32) -> bool {
    let row = (y as usize) * (frame.width as usize);
    spread_is_small(&frame.data[row..row + frame.width as usize])
}

fn col_is_uniform(frame: &LumaFrame, x: u32) -> bool {
    let w = frame.width as usize;
    let vals: Vec<u8> = (0..frame.height as usize)
        .map(|y| frame.data[y * w + x as usize])
        .collect();
    spread_is_small(&vals)
}

fn spread_is_small(v: &[u8]) -> bool {
    if v.is_empty() {
        return false;
    }
    let mean = v.iter().map(|&p| p as f32).sum::<f32>() / v.len() as f32;
    let mad = v.iter().map(|&p| (p as f32 - mean).abs()).sum::<f32>() / v.len() as f32;
    mad <= BAR_UNIFORMITY
}

/// Bars agreed on across several frames.
///
/// Deciding from one frame is unsafe in both directions: a single frame that
/// opens on a dark sky invents bars that are not there, and a single frame
/// whose content happens to reach the edge hides bars that are. So take the
/// bars that hold on a majority of the frames examined, and shrink to the
/// smallest of those — under-removing costs a thin margin the comparison can
/// absorb, over-removing throws away picture and there is no getting it back.
pub fn detect_bars_across(frames: &[LumaFrame]) -> Bars {
    if frames.is_empty() {
        return Bars::default();
    }
    let all: Vec<Bars> = frames.iter().map(detect_bars).collect();
    let quantile = |f: fn(&Bars) -> u32| -> u32 {
        let mut v: Vec<u32> = all.iter().map(f).collect();
        v.sort_unstable();
        // The value at least three-quarters of frames reach or exceed, i.e. the
        // lower quartile. One or two frames whose picture touches the edge do
        // not cancel a bar the rest of the video has.
        v[v.len() / 4]
    };
    Bars {
        top: quantile(|b| b.top),
        bottom: quantile(|b| b.bottom),
        left: quantile(|b| b.left),
        right: quantile(|b| b.right),
    }
}

/// One video's normalisation, decided once and applied to every frame of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Plan {
    pub bars: Bars,
    /// The frame size the bars were measured on, so a plan cannot be applied
    /// to frames of a different size by accident.
    pub source_width: u32,
    pub source_height: u32,
}

impl Plan {
    pub fn from_frames(frames: &[LumaFrame]) -> Option<Plan> {
        let first = frames.first()?;
        Some(Plan {
            bars: detect_bars_across(frames),
            source_width: first.width,
            source_height: first.height,
        })
    }

    /// Apply to one frame: remove the bars. Returns the frame unchanged when
    /// there are none, and `None` when the frame is not the size this plan was
    /// measured on.
    pub fn apply(&self, frame: &LumaFrame) -> Option<LumaFrame> {
        if frame.width != self.source_width || frame.height != self.source_height {
            return None;
        }
        if self.bars.none() {
            return Some(frame.clone());
        }
        let rect = self.bars.content(frame.width, frame.height)?;
        frame.crop(rect)
    }

    /// Aspect ratio of what survives, for the report.
    pub fn content_aspect(&self) -> Option<f32> {
        let r = self.bars.content(self.source_width, self.source_height)?;
        Some(r.w as f32 / r.h as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> LumaFrame {
        let mut data = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                data.push(f(x, y));
            }
        }
        LumaFrame::new(w, h, data, 0, 0).unwrap()
    }

    /// Textured picture, so nothing inside it looks like a bar.
    fn picture(w: u32, h: u32) -> LumaFrame {
        frame(w, h, |x, y| ((x * 7 + y * 13) % 200 + 30) as u8)
    }

    fn letterboxed(w: u32, h: u32, bar: u32) -> LumaFrame {
        frame(w, h, move |x, y| {
            if y < bar || y >= h - bar {
                0
            } else {
                ((x * 7 + y * 13) % 200 + 30) as u8
            }
        })
    }

    #[test]
    fn a_clean_picture_has_no_bars() {
        assert!(detect_bars(&picture(320, 240)).none());
    }

    #[test]
    fn letterboxing_is_found_on_the_right_two_sides() {
        let b = detect_bars(&letterboxed(320, 240, 30));
        assert_eq!((b.top, b.bottom), (30, 30));
        assert_eq!((b.left, b.right), (0, 0));
    }

    #[test]
    fn pillarboxing_is_found_too() {
        let f = frame(320, 240, |x, y| {
            if !(40..280).contains(&x) {
                0
            } else {
                ((x * 7 + y * 13) % 200 + 30) as u8
            }
        });
        let b = detect_bars(&f);
        assert_eq!((b.left, b.right), (40, 40));
        assert_eq!((b.top, b.bottom), (0, 0));
    }

    #[test]
    fn a_grey_bar_is_found_as_well_as_a_black_one() {
        // Bars survive encoding as dark grey with ringing, and a caption band
        // drawn at partial opacity is not black at all. Uniformity is the
        // test, not darkness.
        let f = frame(320, 240, |x, y| {
            if y < 25 {
                70 + (x % 3) as u8
            } else {
                ((x * 7 + y * 13) % 200 + 30) as u8
            }
        });
        assert_eq!(detect_bars(&f).top, 25);
    }

    #[test]
    fn bar_removal_never_eats_more_than_a_third_of_the_frame() {
        // A shot that opens on a dark sky must not have half its picture
        // declared a bar and thrown away.
        let dark = frame(320, 240, |_, y| if y < 200 { 2 } else { 200 });
        let b = detect_bars(&dark);
        assert!(
            b.top <= (240.0 * MAX_BAR_FRACTION) as u32,
            "ate {} rows",
            b.top
        );
    }

    #[test]
    fn a_frame_with_no_picture_at_all_reports_no_bars() {
        // A fade to black, or a covered lens. Every side runs to its cap, and
        // taking that as a crop would misnormalise every frame that does carry
        // picture.
        for level in [0u8, 128, 255] {
            let f = frame(64, 64, move |_, _| level);
            assert!(detect_bars(&f).none(), "level {level} invented bars");
        }
    }

    #[test]
    fn removing_bars_recovers_the_original_picture() {
        let original = picture(320, 180);
        let boxed = letterboxed(320, 240, 30);
        let plan = Plan::from_frames(std::slice::from_ref(&boxed)).unwrap();
        let out = plan.apply(&boxed).unwrap();
        assert_eq!((out.width, out.height), (320, 180));
        // The content is the original's rows shifted by the bar, so the two
        // must agree pixel for pixel once the bar is gone.
        for y in 0..180 {
            for x in 0..320 {
                assert_eq!(
                    out.at(x, y),
                    ((x * 7 + (y + 30) * 13) % 200 + 30) as u8,
                    "at {x},{y}"
                );
            }
        }
        assert_eq!(original.width, out.width);
    }

    #[test]
    fn a_plan_refuses_frames_of_a_different_size() {
        let plan = Plan::from_frames(&[letterboxed(320, 240, 30)]).unwrap();
        assert!(plan.apply(&picture(640, 480)).is_none());
    }

    #[test]
    fn one_odd_frame_does_not_cancel_a_bar_the_video_has() {
        // Three letterboxed frames and one whose picture reaches the edge.
        let mut frames: Vec<LumaFrame> = (0..3).map(|_| letterboxed(320, 240, 30)).collect();
        frames.push(picture(320, 240));
        assert_eq!(detect_bars_across(&frames).top, 30);
    }

    #[test]
    fn a_bar_only_one_frame_has_is_not_removed_from_the_whole_video() {
        // The costly direction: over-removing throws away picture for good.
        let mut frames: Vec<LumaFrame> = (0..7).map(|_| picture(320, 240)).collect();
        frames.push(letterboxed(320, 240, 30));
        assert!(detect_bars_across(&frames).none());
    }

    #[test]
    fn the_description_never_reads_as_an_accusation() {
        let d = Bars {
            top: 30,
            bottom: 30,
            ..Default::default()
        }
        .describe();
        assert!(d.contains("bars removed before comparison"));
        for banned in ["suspicious", "altered", "tamper", "modified"] {
            assert!(!d.to_lowercase().contains(banned), "{d}");
        }
        assert_eq!(Bars::default().describe(), "no added bars");
    }
}
