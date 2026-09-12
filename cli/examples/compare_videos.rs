//! Compare two plain video files, with no bundle and no cryptography.
//!
//! **A development tool, not a second product.** The real tool starts from an
//! evidence bundle, because a comparison against an original nobody has
//! established says nothing. This one exists for two jobs the product path
//! cannot do:
//!
//!   * measuring a candidate recording's self-similarity before adopting it as
//!     test material — a near-static shot makes every alignment result mush,
//!     and it is worth knowing that before building a corpus on one;
//!   * debugging the alignment itself, which needs varied footage. The
//!     footage that has variety — a trip, a street, a building site — sits in
//!     MIGRATED archives that carry no notary proof and therefore, by design,
//!     no evidence bundle at all.
//!
//! It prints, it does not produce a report. Nothing here writes the language
//! the report is held to, and nothing here should ever be shown to a reader as
//! a finding.
//!
//!     cargo run --release -p edit-report --example compare_videos -- A.mp4 B.mp4
//!     cargo run --release -p edit-report --example compare_videos -- A.mp4

use edit_report::decode;
use edit_report_core::align::{self, Sample};
use edit_report_core::fingerprint::fingerprint;
use edit_report_core::normalize::Plan;
use std::path::Path;

fn clock(us: i64) -> String {
    let t = us.max(0);
    format!("{}:{:05.2}", t / 60_000_000, (t % 60_000_000) as f64 / 1e6)
}

fn sample(path: &Path, rate: f64) -> Result<(Vec<Sample>, Plan), Box<dyn std::error::Error>> {
    let profile = decode::probe(path, None)?;
    let bar_frames = decode::decode_spread(path, &profile, 24)?;
    let plan = Plan::from_frames(&bar_frames).ok_or("no frames decoded")?;
    eprintln!(
        "{}: {}×{}, {} frames, {:.1}s — {}",
        path.display(),
        profile.width,
        profile.height,
        profile.frame_count,
        profile.duration_us as f64 / 1e6,
        plan.bars.describe(),
    );

    let mut out = Vec::new();
    decode::decode_stream(
        path,
        &profile,
        decode::stride_for_rate(&profile, rate),
        |f| {
            if let Some(n) = plan.apply(&f) {
                out.push(Sample {
                    index: f.index,
                    t_us: f.pts_us,
                    fp: fingerprint(&n),
                });
            }
        },
    )?;
    Ok((out, plan))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.len() > 2 {
        eprintln!("usage: compare_videos <original> [copy]");
        std::process::exit(2);
    }
    let rate = std::env::var("EDIT_REPORT_RATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(decode::SAMPLES_PER_SECOND);

    let (original, _) = sample(Path::new(&args[0]), rate)?;
    let sim = align::self_similarity(&original);
    println!(
        "\nself-similarity: {} of {} pairs >{}s apart are within the segment gate ({:.1}%)",
        sim.pairs_within_segment_gate,
        sim.pairs_examined,
        align::SELF_SIMILARITY_MIN_GAP_US / 1_000_000,
        100.0 * sim.fraction_within_gate(),
    );
    println!(
        "  {} within the looser proposal threshold ({:.1}%)",
        sim.pairs_within_threshold,
        100.0 * sim.pairs_within_threshold as f32 / sim.pairs_examined.max(1) as f32,
    );
    println!(
        "  verdict for test material: {}",
        match 100.0 * sim.fraction_within_gate() {
            f if f < 5.0 => "good — moments are distinguishable",
            f if f < 15.0 => "usable — offsets will be approximate",
            _ => "poor — a near-static shot, offsets will be weakly determined",
        }
    );

    let Some(copy_path) = args.get(1) else {
        return Ok(());
    };
    let (copy, _) = sample(Path::new(copy_path), rate)?;

    let c = align::align(&copy, &original);
    println!(
        "\n{} segment(s), {} cut(s), {} unmatched; {}/{} copy frames matched, {} too flat to anchor",
        c.segments.len(),
        c.cuts.len(),
        c.unmatched.len(),
        c.copy_frames_matched,
        c.copy_frames_examined,
        c.copy_frames_low_variance,
    );
    for (i, s) in c.segments.iter().enumerate() {
        println!(
            "  seg{:<2} copy {} → {}   original {} → {}   offset {:+.2}s   {} frames, mean {:.1}/63",
            i + 1,
            clock(s.copy_start_us),
            clock(s.copy_end_us),
            clock(s.original_start_us),
            clock(s.original_end_us),
            s.offset_us as f64 / 1e6,
            s.frames_agreeing,
            s.mean_distance,
        );
    }
    for u in &c.unmatched {
        println!(
            "  gap    copy {} → {}   ({} frames examined, none matched)",
            clock(u.copy_start_us),
            clock(u.copy_end_us),
            u.frames_examined,
        );
    }
    Ok(())
}
