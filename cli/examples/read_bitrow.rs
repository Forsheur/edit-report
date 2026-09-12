//! Read the machine-readable strip out of a video and check it against what
//! the counter must be.
//!
//!     cargo run --release -p edit-report --example read_bitrow -- VIDEO [ORIGINAL_WIDTH]
//!
//! The counter must be the frame index plus a constant, so the spread of
//! (counter − index) is the whole check: one value means every read agrees.

use edit_report::decode;
use edit_report_core::bitrow::read;
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: read_bitrow <video> [original_width]");
        std::process::exit(2);
    }
    let path = Path::new(&args[0]);
    let profile = decode::probe(path, None)?;
    let scale = match args.get(1).and_then(|w| w.parse::<u32>().ok()) {
        Some(ow) if ow > 0 => profile.width as f32 / ow as f32,
        _ => 1.0,
    };
    println!(
        "{}: {}×{}, {} frames, scale {:.3}",
        path.display(),
        profile.width,
        profile.height,
        profile.frame_count,
        scale
    );

    let mut offsets: BTreeMap<i64, usize> = BTreeMap::new();
    let mut tags: BTreeMap<u16, usize> = BTreeMap::new();
    let mut refusals: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    decode::decode_stream(
        path,
        &profile,
        decode::stride_for_rate(&profile, decode::SAMPLES_PER_SECOND),
        |f| {
            total += 1;
            match read(&f, scale) {
                Ok(r) => {
                    *offsets
                        .entry(r.counter as i64 - f.index as i64)
                        .or_default() += 1;
                    *tags.entry(r.session_tag).or_default() += 1;
                }
                Err(e) => *refusals.entry(format!("{e:?}")).or_default() += 1,
            }
        },
    )?;

    let read_ok: usize = offsets.values().sum();
    println!("  strip read on {read_ok} of {total} frame(s)");
    for (k, n) in &offsets {
        println!("    counter − frame index = {k:+}  on {n} frame(s)");
    }
    for (t, n) in &tags {
        println!("    session tag 0x{t:04X}  on {n} frame(s)");
    }
    for (why, n) in &refusals {
        println!("    refused: {why} × {n}");
    }
    println!(
        "  VERDICT: {}",
        match (offsets.len(), tags.len()) {
            (0, _) => "no strip read".to_string(),
            (1, 1) => format!("every read agrees ({read_ok}/{total} frames)"),
            _ => "reads disagree; at least one is wrong".to_string(),
        }
    );
    Ok(())
}
