//! Reading the QR code a Forsheur recording burns into its own picture.
//!
//! ## What this is for, and what it is not for
//!
//! A Forsheur phone burns `<host>/v/<short_id>` into every frame of every
//! camera (see `phone/lib/main.dart` and the two `FrameCompositor` /
//! `GlRotator` implementations). Reading it back tells you which recording a
//! video *claims* to be.
//!
//! Claims, and nothing more. Those pixels are not signed. Anyone can render
//! the same string into their own footage, and the fixture set contains
//! exactly that case. A code found here is a lead — it says which bundle to
//! open — and the answer to "is this that recording" comes from comparing the
//! pictures, never from the code.
//!
//! Symmetrically: **no code is not a finding about the video.** The band sits
//! at the top of the frame, which is the first thing a crop removes and the
//! usual place for a subtitle bar. Absence is reported as absence.
//!
//! ## Why the sweep
//!
//! Handing a frame straight to a QR reader does not work on real footage, and
//! we measured it rather than assuming: on a genuine preprod recording, both
//! OpenCV and rxing return nothing from the raw crop. Three things are working
//! against the decoder at once:
//!
//!   * the code is composited at **80 % opacity** over the scene, so its
//!     "white" is whatever was behind it and its "black" is a blend;
//!   * the burn-in carries **no quiet zone** — both phone implementations
//!     deliberately crop or fill it away, and the QR specification requires
//!     four modules of margin for a decoder to lock on;
//!   * it is 87 px on a 720-wide frame, and H.264 has smoothed every module
//!     edge.
//!
//! So each candidate region is normalised before decoding: convert to a fixed
//! contrast by Otsu, upscale with nearest-neighbour so module edges stay hard,
//! and paste the result onto a white margin that supplies the missing quiet
//! zone. That combination reads the same recording both libraries had refused.

use crate::frame::{LumaFrame, Rect};
use rxing::common::HybridBinarizer;
use rxing::multi::{GenericMultipleBarcodeReader, MultipleBarcodeReader};
use rxing::qrcode::QRCodeReader;
use rxing::{BinaryBitmap, DecodeHints, Luma8LuminanceSource};
use std::collections::BTreeMap;

/// How a candidate region was normalised before the decoder saw it.
///
/// Recorded and reported, not hidden: a reader who wants to check a decode by
/// hand needs the exact recipe that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recipe {
    /// The region of the frame that was cut out, in frame pixels.
    pub region: Rect,
    /// Nearest-neighbour upscale factor applied to that region.
    pub upscale: u32,
    /// How the region was binarised before the decoder saw it.
    pub threshold: Threshold,
    /// Width of the white margin pasted around it, in upscaled pixels.
    pub quiet_zone_px: u32,
}

/// How a region was turned into black and white.
///
/// `Local` is listed first in the sweep because it wins on real footage: the
/// burn-in sits over a moving scene, and a global cut fails whenever that
/// scene carries a gradient behind the code. See `LumaFrame::adaptive_threshold`
/// for the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Threshold {
    /// Handed to the decoder untouched.
    None,
    /// One cut for the whole region, chosen by Otsu. `value` is filled in with
    /// the threshold actually used, so the decode can be reproduced.
    Global { value: u8 },
    /// Each pixel against the mean of a `block × block` box around it.
    Local { block: u32, bias: i32 },
}

/// One code read out of one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub payload: String,
    pub recipe: Recipe,
}

/// What a Forsheur burn-in payload parses to.
///
/// The payload carries no scheme and no version field — it is exactly
/// `<host>/v/<short_id>`, built at `phone/lib/main.dart` from the host of the
/// server the app is pointed at. Anything else is kept verbatim and reported
/// as an unrecognised payload rather than discarded, because "this video
/// carries a QR code that is not a Forsheur reference" is information.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForsheurRef {
    pub host: String,
    pub short_id: String,
}

impl ForsheurRef {
    /// Parse `<host>/v/<short_id>`, tolerating a scheme and a trailing slash
    /// in case a code was ever produced or re-typed with one.
    ///
    /// The short id is 12 base62 characters (`_generateShortId`), and that
    /// shape is enforced: a lax parser here would let `example.com/v/../..`
    /// through and turn a report line into a place to hide a string.
    pub fn parse(payload: &str) -> Option<ForsheurRef> {
        let s = payload.trim();
        let s = s
            .strip_prefix("https://")
            .or_else(|| s.strip_prefix("http://"))
            .unwrap_or(s);
        let s = s.strip_suffix('/').unwrap_or(s);
        let (host, rest) = s.split_once("/v/")?;
        if host.is_empty() || host.contains('/') {
            return None;
        }
        if rest.len() != 12 || !rest.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        Some(ForsheurRef {
            host: host.to_string(),
            short_id: rest.to_string(),
        })
    }
}

/// Tuning for the sweep. Defaults are the ones measured against real preprod
/// footage; they are a field rather than a constant so a caller analysing very
/// large frames can trade breadth for time.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanConfig {
    /// Side of the top-left square to try, as a fraction of the frame's
    /// shorter edge. The burn-in is 87 px plus a 4 px margin on a 720-wide
    /// frame — 0.132 — but a copy may have been scaled or cropped, so the
    /// span is generous.
    pub corner_fractions: Vec<f32>,
    pub upscales: Vec<u32>,
    /// Block sizes for the local threshold, as a multiple of the upscale
    /// factor — roughly "how many original pixels wide is the neighbourhood".
    pub local_blocks: Vec<u32>,
    /// Inset of the region from the frame corner, as a fraction of its side.
    ///
    /// The burn-in is drawn 4 px in from the corner on a 87 px code — 0.046 —
    /// so a region anchored at the corner drags in a strip of scene on two
    /// sides. That strip is what a threshold sees, and on a dark scene it
    /// pulls the cut far enough to lose the code entirely. Measured on a
    /// fixture whose code reads perfectly at the tight offset and not at all
    /// with 20 px of scene around it.
    pub insets: Vec<f32>,
    /// Try the whole frame first. Cheap, and it is the only thing that finds a
    /// code somewhere other than the burn-in corner — a screenshot pasted into
    /// a montage, say.
    pub try_full_frame: bool,
    /// Try the burn-in's OWN position first, scaled by this factor.
    ///
    /// The QR is not in the corner: it sits at x=4, y=16, below the
    /// machine-readable strip. Anchoring the first attempt on where the phone
    /// actually draws it is both faster and more reliable than sweeping
    /// symmetric corner squares, which is what the fallback below does for a
    /// copy that was cropped or rescaled.
    pub known_position_scale: Option<f32>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        ScanConfig {
            // 0.132 is the burn-in's own ratio (87 px plus a 4 px margin on a
            // 720-wide frame). The span around it covers a copy that was
            // rescaled or cropped, and the measurement that set it found no
            // single width good enough to stand alone.
            corner_fractions: vec![0.121, 0.132, 0.15, 0.167, 0.10, 0.18, 0.22],
            upscales: vec![6, 4],
            local_blocks: vec![8, 12],
            insets: vec![0.046, 0.0],
            try_full_frame: true,
            known_position_scale: Some(1.0),
        }
    }
}

/// A recipe that worked, kept so the next frame can try it first. Consecutive
/// frames of one video share a geometry, so this turns an O(recipes) sweep
/// into one decode for every frame after the first hit.
#[derive(Debug, Clone, Default)]
pub struct ScanCache {
    last_good: Option<Recipe>,
}

/// Read every QR code in one frame.
///
/// Returns each distinct payload once, with the recipe that first produced it.
/// More than one distinct payload in a frame is a legitimate outcome, not an
/// error — a montage can carry two burn-ins at once — and it travels to the
/// report as such.
pub fn scan_frame(frame: &LumaFrame, cfg: &ScanConfig, cache: &mut ScanCache) -> Vec<Decoded> {
    let mut found: BTreeMap<String, Recipe> = BTreeMap::new();

    if let Some(recipe) = cache.last_good {
        collect(frame, recipe, &mut found);
        if !found.is_empty() {
            return into_vec(found);
        }
    }

    for recipe in recipes(frame, cfg) {
        collect(frame, recipe, &mut found);
        if !found.is_empty() {
            cache.last_good = Some(recipe);
            break;
        }
    }
    into_vec(found)
}

fn into_vec(found: BTreeMap<String, Recipe>) -> Vec<Decoded> {
    found
        .into_iter()
        .map(|(payload, recipe)| Decoded { payload, recipe })
        .collect()
}

/// Every normalisation to try, cheapest first.
fn recipes(frame: &LumaFrame, cfg: &ScanConfig) -> Vec<Recipe> {
    let mut out = Vec::new();
    let full = Rect::new(0, 0, frame.width, frame.height);

    // Where the phone actually draws it, when the frame is believed unscaled
    // or the caller knows the scale. Tried first: it costs two recipes and
    // saves the whole sweep on the ordinary case.
    if let Some(scale) = cfg.known_position_scale {
        let s = |v: u32| ((v as f32) * scale).round() as u32;
        let side = s(crate::overlay::native::QR_SIZE);
        let x = s(crate::overlay::native::QR_MARGIN);
        let y = s(crate::overlay::native::QR_MARGIN_Y);
        if side >= 24 {
            let region = Rect::new(x, y, side, side);
            for &up in &cfg.upscales {
                out.push(Recipe {
                    region,
                    upscale: up,
                    threshold: Threshold::Local {
                        block: 8 * up,
                        bias: 6,
                    },
                    quiet_zone_px: 4 * up,
                });
                out.push(Recipe {
                    region,
                    upscale: up,
                    threshold: Threshold::Global { value: 0 },
                    quiet_zone_px: 4 * up,
                });
            }
        }
    }

    if cfg.try_full_frame {
        // Untouched, then binarised. A code rendered cleanly into a montage
        // (full opacity, real quiet zone) reads on the first of these, and
        // paying for the sweep would be waste.
        out.push(Recipe {
            region: full,
            upscale: 1,
            threshold: Threshold::None,
            quiet_zone_px: 0,
        });
        out.push(Recipe {
            region: full,
            upscale: 1,
            threshold: Threshold::Global { value: 0 },
            quiet_zone_px: 16,
        });
    }

    let short = frame.width.min(frame.height) as f32;
    for &f in &cfg.corner_fractions {
        let side = (short * f).round() as u32;
        if side < 24 {
            continue; // below this a 29-module code has under one pixel per module
        }
        let region = Rect::new(0, 0, side, side);
        for &up in &cfg.upscales {
            let quiet_zone_px = 4 * up;
            // Local first: it reads this footage more often than a global cut.
            for &b in &cfg.local_blocks {
                out.push(Recipe {
                    region,
                    upscale: up,
                    threshold: Threshold::Local {
                        block: b * up,
                        bias: 6,
                    },
                    quiet_zone_px,
                });
            }
            out.push(Recipe {
                region,
                upscale: up,
                threshold: Threshold::Global { value: 0 },
                quiet_zone_px,
            });
        }
    }
    out
}

/// Apply one recipe and record whatever the decoder returns.
///
/// `Threshold::Global { value: 0 }` means "compute it"; the computed value is
/// written back into the recipe that gets reported, so the report carries the
/// threshold actually used rather than a placeholder.
fn collect(frame: &LumaFrame, recipe: Recipe, found: &mut BTreeMap<String, Recipe>) {
    let Some(region) = frame.crop(recipe.region) else {
        return;
    };
    let up = region.upscale_nearest(recipe.upscale.max(1));
    let (prepared, used_threshold) = match recipe.threshold {
        Threshold::None => (up, Threshold::None),
        Threshold::Global { .. } => {
            let t = up.otsu_threshold();
            (up.binarize(t), Threshold::Global { value: t })
        }
        Threshold::Local { block, bias } => (
            up.adaptive_threshold(block, bias),
            Threshold::Local { block, bias },
        ),
    };
    let padded = if recipe.quiet_zone_px > 0 {
        prepared.pad(recipe.quiet_zone_px, 255)
    } else {
        prepared
    };
    let reported = Recipe {
        threshold: used_threshold,
        ..recipe
    };

    for payload in decode_all(&padded) {
        found.entry(payload).or_insert(reported);
    }
}

/// Hand a prepared buffer to rxing and return every payload it reports.
fn decode_all(img: &LumaFrame) -> Vec<String> {
    let Ok(source) = Luma8LuminanceSource::new(img.data.clone(), img.width, img.height) else {
        return Vec::new();
    };
    let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
    // TryHarder costs time and buys exactly the cases this tool cares about:
    // a code that is small, rotated a little, or blurred by re-encoding.
    let hints = DecodeHints {
        TryHarder: Some(true),
        ..DecodeHints::default()
    };
    let mut reader = GenericMultipleBarcodeReader::new(QRCodeReader);
    match reader.decode_multiple_with_hints(&mut bitmap, &hints) {
        Ok(results) => results
            .into_iter()
            .map(|r| r.getText().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_burn_in_payload() {
        let r = ForsheurRef::parse("preprod.forsheur.com/v/3gvqtL7Y7HZL").unwrap();
        assert_eq!(r.host, "preprod.forsheur.com");
        assert_eq!(r.short_id, "3gvqtL7Y7HZL");
    }

    #[test]
    fn tolerates_a_scheme_and_a_trailing_slash() {
        let r = ForsheurRef::parse("https://forsheur.com/v/aB3xY9zQ1234/").unwrap();
        assert_eq!(r.host, "forsheur.com");
        assert_eq!(r.short_id, "aB3xY9zQ1234");
    }

    #[test]
    fn rejects_anything_that_is_not_a_12_char_base62_id() {
        // A short id is exactly 12 base62 characters. Everything else is an
        // unrecognised payload, reported verbatim, never coerced into a
        // reference the report would then present as a Forsheur recording.
        for bad in [
            "forsheur.com/v/short",
            "forsheur.com/v/waytoolongforanid",
            "forsheur.com/v/../../etc/passwd",
            "forsheur.com/v/abc-def-ghi12",
            "forsheur.com/x/aB3xY9zQ1234",
            "not a url at all",
            "",
        ] {
            assert!(
                ForsheurRef::parse(bad).is_none(),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn a_blank_frame_yields_nothing_and_says_so_by_being_empty() {
        let f = LumaFrame::new(320, 240, vec![128; 320 * 240], 0, 0).unwrap();
        let got = scan_frame(&f, &ScanConfig::default(), &mut ScanCache::default());
        assert!(got.is_empty());
    }

    #[test]
    fn recipe_sweep_skips_regions_too_small_to_hold_a_code() {
        let f = LumaFrame::new(120, 120, vec![0; 120 * 120], 0, 0).unwrap();
        let cfg = ScanConfig {
            corner_fractions: vec![0.05, 0.5],
            upscales: vec![4],
            local_blocks: vec![8],
            insets: vec![0.0],
            try_full_frame: false,
            known_position_scale: None,
        };
        let rs = recipes(&f, &cfg);
        // 0.05 * 120 = 6 px, far below one pixel per module — dropped. The
        // surviving width yields one local recipe and one global one.
        assert_eq!(rs.len(), 2);
        assert!(rs.iter().all(|r| r.region.w == 60));
        assert!(
            matches!(rs[0].threshold, Threshold::Local { .. }),
            "local must be tried first"
        );
    }
}

// ---------------------------------------------------------------------------
// Aggregation across a whole video
// ---------------------------------------------------------------------------

/// Running tally for one distinct payload seen across a video.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CodeSighting {
    pub payload: String,
    /// A Forsheur reference when the payload has that shape; `null` otherwise.
    /// An unrecognised payload keeps its `payload` string and gets no
    /// interpretation put on it.
    pub reference: Option<ForsheurRef>,
    /// How many of the examined frames carried this payload.
    pub frames: u32,
    pub first_frame: u64,
    pub first_pts_us: i64,
    pub last_frame: u64,
    pub last_pts_us: i64,
    /// The normalisation that first read it, so the decode can be reproduced.
    pub recipe: Recipe,
}

/// What a QR sweep over a video found. Counts and sightings — no verdict.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct QrFindings {
    /// How many frames were actually examined. Reported because "no code
    /// found" means something different after 4 frames than after 400.
    pub frames_examined: u32,
    /// How many of them carried at least one code.
    pub frames_with_code: u32,
    /// Every distinct payload, most-seen first. Empty is a normal outcome.
    pub codes: Vec<CodeSighting>,
}

impl QrFindings {
    /// True when nothing was read. Named for what it is — no code was read —
    /// and not for anything it might be taken to imply about the video.
    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// Distinct Forsheur short ids among the sightings, in first-seen order.
    /// More than one is a fact to display, never an error.
    pub fn distinct_short_ids(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for c in &self.codes {
            if let Some(r) = &c.reference {
                if !seen.contains(&r.short_id.as_str()) {
                    seen.push(&r.short_id);
                }
            }
        }
        seen
    }
}

/// Accumulates sightings frame by frame.
///
/// Deliberately not "scan the whole video and return": the caller decides
/// which frames to spend time on, because that choice belongs to whoever
/// knows the video's length and the user's patience.
#[derive(Debug, Clone)]
pub struct Survey {
    cfg: ScanConfig,
    cache: ScanCache,
    frames_examined: u32,
    frames_with_code: u32,
    codes: BTreeMap<String, CodeSighting>,
}

impl Survey {
    pub fn new(cfg: ScanConfig) -> Self {
        Survey {
            cfg,
            cache: ScanCache::default(),
            frames_examined: 0,
            frames_with_code: 0,
            codes: BTreeMap::new(),
        }
    }

    pub fn examine(&mut self, frame: &LumaFrame) {
        self.frames_examined += 1;
        let hits = scan_frame(frame, &self.cfg, &mut self.cache);
        if !hits.is_empty() {
            self.frames_with_code += 1;
        }
        for hit in hits {
            match self.codes.get_mut(&hit.payload) {
                Some(existing) => {
                    existing.frames += 1;
                    existing.last_frame = frame.index;
                    existing.last_pts_us = frame.pts_us;
                }
                None => {
                    self.codes.insert(
                        hit.payload.clone(),
                        CodeSighting {
                            reference: ForsheurRef::parse(&hit.payload),
                            payload: hit.payload,
                            frames: 1,
                            first_frame: frame.index,
                            first_pts_us: frame.pts_us,
                            last_frame: frame.index,
                            last_pts_us: frame.pts_us,
                            recipe: hit.recipe,
                        },
                    );
                }
            }
        }
    }

    pub fn finish(self) -> QrFindings {
        let mut codes: Vec<CodeSighting> = self.codes.into_values().collect();
        // Most-seen first, then earliest, then payload — a total order, so the
        // report is byte-identical across runs on the same input.
        codes.sort_by(|a, b| {
            b.frames
                .cmp(&a.frames)
                .then(a.first_frame.cmp(&b.first_frame))
                .then(a.payload.cmp(&b.payload))
        });
        QrFindings {
            frames_examined: self.frames_examined,
            frames_with_code: self.frames_with_code,
            codes,
        }
    }
}

/// Pick which frames to examine: `count` positions spread evenly across
/// `total_frames`, first and last included.
///
/// Spread, not "the first N". A burn-in can be missing from the opening
/// seconds of a copy that was trimmed, present in the middle, and gone again
/// under a subtitle bar at the end; sampling only the head would report the
/// video's first accident as the video's answer.
pub fn sample_positions(total_frames: u64, count: u32) -> Vec<u64> {
    if total_frames == 0 || count == 0 {
        return Vec::new();
    }
    let count = (count as u64).min(total_frames);
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|i| (i * (total_frames - 1) + (count - 1) / 2) / (count - 1))
        .collect()
}

#[cfg(test)]
mod survey_tests {
    use super::*;

    #[test]
    fn sampling_spans_the_whole_video_ends_included() {
        let p = sample_positions(1000, 5);
        assert_eq!(p.first(), Some(&0));
        assert_eq!(p.last(), Some(&999));
        assert_eq!(p.len(), 5);
        // strictly increasing, so no frame is examined twice
        assert!(p.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn sampling_never_asks_for_more_frames_than_exist() {
        assert_eq!(sample_positions(3, 10), vec![0, 1, 2]);
        assert_eq!(sample_positions(0, 10), Vec::<u64>::new());
        assert_eq!(sample_positions(10, 0), Vec::<u64>::new());
        assert_eq!(sample_positions(10, 1), vec![0]);
    }

    #[test]
    fn an_empty_survey_reports_zero_examined_and_no_codes() {
        let f = Survey::new(ScanConfig::default()).finish();
        assert_eq!(f.frames_examined, 0);
        assert!(f.is_empty());
        assert!(f.distinct_short_ids().is_empty());
    }

    #[test]
    fn distinct_short_ids_lists_each_once_in_first_seen_order() {
        let mk = |id: &str, frames: u32, first: u64| CodeSighting {
            payload: format!("forsheur.com/v/{id}"),
            reference: ForsheurRef::parse(&format!("forsheur.com/v/{id}")),
            frames,
            first_frame: first,
            first_pts_us: 0,
            last_frame: first,
            last_pts_us: 0,
            recipe: Recipe {
                region: Rect::new(0, 0, 95, 95),
                upscale: 4,
                threshold: Threshold::Global { value: 120 },
                quiet_zone_px: 16,
            },
        };
        let f = QrFindings {
            frames_examined: 20,
            frames_with_code: 20,
            codes: vec![
                mk("aaaaaaaaaaaa", 12, 0),
                mk("bbbbbbbbbbbb", 8, 13),
                mk("aaaaaaaaaaaa", 3, 30),
            ],
        };
        assert_eq!(f.distinct_short_ids(), vec!["aaaaaaaaaaaa", "bbbbbbbbbbbb"]);
    }
}
