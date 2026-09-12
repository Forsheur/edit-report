//! The declaration pass: every frame of both files, no sampling.
//!
//! Shared by the binary and by the development driver so there is one
//! implementation of it. A second copy would be a second thing to keep true,
//! and this is the path whose correctness the whole report rests on.
//!
//! Both files are streamed and neither is held in memory. What survives a pass
//! is one record per frame — a counter, a signature, a 64-bit fingerprint and
//! a timestamp — so an hour of video costs a few megabytes a side.

use crate::decode::{self, DecodeError};
use edit_report_core::bitrow;
use edit_report_core::declared::{self, Declared, DeclaredCorrespondence, OriginalFrame, Tuning};
use edit_report_core::fingerprint::{fingerprint, Fingerprint};
use edit_report_core::report::MediaProfile;
use std::path::Path;
use std::time::{Duration, Instant};

/// One frame, as read.
struct Row {
    reading: Option<bitrow::RowReading>,
    fp: Fingerprint,
    t_us: i64,
    index: u64,
}

/// What a pass over both files established, plus what it cost.
pub struct Pass {
    pub correspondence: DeclaredCorrespondence,
    /// Frames read on each side.
    pub original_frames: usize,
    pub copy_frames: usize,
    /// Frames of the ORIGINAL whose strip could be read. Zero means the
    /// original predates the strip, and the declaration path cannot run.
    pub original_declaring: usize,
    pub elapsed: Duration,
    pub copy_duration_us: i64,
}

impl Pass {
    /// Whether the original carries the strip at all. Without it there is
    /// nothing to put the copy's declarations to, and saying so plainly beats
    /// reporting an empty correspondence as though it were a finding.
    pub fn original_is_readable(&self) -> bool {
        self.original_declaring > 0
    }
}

fn scan(path: &Path, profile: &MediaProfile, scale: f32) -> Result<Vec<Row>, DecodeError> {
    let mut rows = Vec::with_capacity(profile.frame_count.max(0) as usize);
    decode::decode_stream(path, profile, 1, |frame| {
        rows.push(Row {
            reading: bitrow::read(&frame, scale).ok(),
            fp: fingerprint(&frame),
            t_us: frame.pts_us,
            index: frame.index,
        });
    })?;
    Ok(rows)
}

/// Read both files whole and put every declaration to the original.
///
/// `expected_tag` is the signature the bundle says these frames should carry,
/// derived from its short id. It is checked, never assumed: a frame carrying
/// somebody else's signature settles a swapped recording in one frame, and a
/// frame carrying the right one proves nothing on its own, since a signature
/// is two bytes and can be drawn.
pub fn run(
    original: &Path,
    copy: &Path,
    short_id: &str,
) -> Result<Pass, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let op = decode::probe(original, None)?;
    let cp = decode::probe(copy, None)?;

    // The copy may have been rescaled. The strip's geometry is expressed in
    // the original's pixels, so the reader is told the ratio rather than
    // guessing it from the frame it is handed.
    let scale = if op.width > 0 {
        cp.width as f32 / op.width as f32
    } else {
        1.0
    };

    let orows = scan(original, &op, 1.0)?;
    let crows = scan(copy, &cp, scale)?;

    let originals: Vec<OriginalFrame> = orows
        .iter()
        .filter_map(|r| {
            r.reading.map(|k| OriginalFrame {
                counter: k.counter,
                index: r.index,
                t_us: r.t_us,
                fp: r.fp,
            })
        })
        .collect();
    let copies: Vec<Declared> = crows
        .iter()
        .map(|r| Declared {
            copy_index: r.index,
            copy_t_us: r.t_us,
            counter: r.reading.map(|k| k.counter),
            tag: r.reading.map(|k| k.session_tag),
            fp: r.fp,
        })
        .collect();

    // Frames per second of the COPY: the continuity floor has to absorb one
    // step between examined frames, and with every frame read that step is one
    // frame period of the copy.
    let fps = if cp.frame_count > 0 && cp.duration_us > 0 {
        cp.frame_count as f64 / (cp.duration_us as f64 / 1e6)
    } else {
        30.0
    };
    let tuning = Tuning {
        expected_tag: (!short_id.is_empty()).then(|| bitrow::session_tag(Some(short_id))),
        ..Tuning::every_frame(fps)
    };

    Ok(Pass {
        correspondence: declared::build_with(&copies, &originals, tuning),
        original_frames: orows.len(),
        copy_frames: crows.len(),
        original_declaring: originals.len(),
        elapsed: started.elapsed(),
        copy_duration_us: cp.duration_us,
    })
}

/// A path the report can load, relative to where the report is written.
pub fn relative(page: &Path, target: &Path) -> String {
    let page_dir = page.parent().unwrap_or(Path::new("."));
    let (a, b) = (
        std::fs::canonicalize(page_dir).unwrap_or_else(|_| page_dir.to_path_buf()),
        std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf()),
    );
    let mut ai = a.components().peekable();
    let mut bi = b.components().peekable();
    while ai.peek().is_some() && ai.peek() == bi.peek() {
        ai.next();
        bi.next();
    }
    let ups = ai.count();
    let rest: std::path::PathBuf = bi.collect();
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    out.push_str(&rest.to_string_lossy());
    out
}
