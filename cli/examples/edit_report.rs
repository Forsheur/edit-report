//! The edit report, every frame of both files.
//!
//!     cargo run --release -p edit-report --example edit_report -- \
//!         ORIGINAL COPY [--out report.html] [--short-id XXXX]
//!
//! Reads the machine-readable strip on **every** frame of both files. Nothing
//! is sampled, and that is the point: sampling one frame in seven on each side
//! meant that after a cut the two grids no longer lined up, a copy frame was
//! compared against a neighbouring original frame, and faithful frames were
//! reported as contradicted.
//!
//! Both files are streamed; neither is ever held in memory. What is kept is
//! one record per frame — a counter, a 64-bit fingerprint and a timestamp,
//! about 32 bytes, so an hour of video costs a few megabytes on each side.

use edit_report::decode;
use edit_report_core::bitrow;
use edit_report_core::declared::{self, Declared, JoinKind, OriginalFrame, Tuning};
use edit_report_core::edit_page::{self, clock, PageInputs};
use edit_report_core::fingerprint::fingerprint;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The two-byte recording signature: the first two bytes of SHA-256 over the
/// short id. Mirrors `bitrow::session_tag`, which is what the phones burn.
fn expected_tag(short_id: &str) -> u16 {
    bitrow::session_tag(Some(short_id))
}

struct Args {
    original: PathBuf,
    copy: PathBuf,
    out: Option<PathBuf>,
    short_id: String,
    chain_verdict: String,
    chain_passed: bool,
}

fn parse() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let original = a.next().ok_or("usage: edit_report ORIGINAL COPY [options]")?;
    let copy = a.next().ok_or("usage: edit_report ORIGINAL COPY [options]")?;
    let mut out = None;
    let mut short_id = String::new();
    let mut chain_verdict =
        "not run here — this driver compares pictures only".to_string();
    let mut chain_passed = true;
    while let Some(flag) = a.next() {
        let mut value = || a.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--short-id" => short_id = value()?,
            "--chain" => chain_verdict = value()?,
            "--chain-failed" => chain_passed = false,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Args {
        original: PathBuf::from(original),
        copy: PathBuf::from(copy),
        out,
        short_id,
        chain_verdict,
        chain_passed,
    })
}

/// One streamed pass: read the strip and fingerprint every frame.
fn scan(
    path: &Path,
    profile: &edit_report_core::report::MediaProfile,
    scale_against: Option<u32>,
) -> Result<(Vec<(Option<bitrow::RowReading>, u64, i64, u64)>, f64, usize), Box<dyn std::error::Error>>
{
    // The caller already probed; probing again costs another ffprobe launch,
    // which on these short files was most of the wall clock.
    let scale = match scale_against {
        Some(w) if w > 0 => profile.width as f32 / w as f32,
        _ => 1.0,
    };
    let mut rows = Vec::with_capacity(profile.frame_count.max(0) as usize);
    let t0 = Instant::now();
    decode::decode_stream(path, profile, 1, |frame| {
        let reading = bitrow::read(&frame, scale).ok();
        let fp = fingerprint(&frame);
        rows.push((reading, fp.bits, frame.pts_us, frame.index));
        // `spread` is carried by the fingerprint itself; keep only what the
        // comparison needs so the per-frame record stays small.
        let _ = fp.spread;
    })?;
    let secs = t0.elapsed().as_secs_f64();
    Ok((rows, secs, profile.width as usize))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let op = decode::probe(&args.original, None)?;
    let cp = decode::probe(&args.copy, None)?;
    println!(
        "original {}×{} {} frames   copy {}×{} {} frames",
        op.width, op.height, op.frame_count, cp.width, cp.height, cp.frame_count
    );

    let started = Instant::now();
    let (orows, osecs, _) = scan(&args.original, &op, None)?;
    let (crows, csecs, _) = scan(&args.copy, &cp, Some(op.width))?;
    println!(
        "read every frame: original {} in {:.1}s, copy {} in {:.1}s",
        orows.len(),
        osecs,
        crows.len(),
        csecs
    );

    let original: Vec<OriginalFrame> = orows
        .iter()
        .filter_map(|(r, bits, t_us, index)| {
            r.map(|r| OriginalFrame {
                counter: r.counter,
                index: *index,
                t_us: *t_us,
                fp: edit_report_core::fingerprint::Fingerprint {
                    bits: *bits,
                    spread: 0.0,
                },
            })
        })
        .collect();
    let copy: Vec<Declared> = crows
        .iter()
        .map(|(r, bits, t_us, index)| Declared {
            copy_index: *index,
            copy_t_us: *t_us,
            counter: r.map(|r| r.counter),
            tag: r.map(|r| r.session_tag),
            fp: edit_report_core::fingerprint::Fingerprint {
                bits: *bits,
                spread: 0.0,
            },
        })
        .collect();

    if original.is_empty() {
        println!(
            "\nNo strip could be read on any frame of the original. This driver reads the \
             machine-readable strip only; a recording made before it existed has to go \
             through the text path instead."
        );
        return Ok(());
    }

    let fps = if cp.frame_count > 0 && cp.duration_us > 0 {
        cp.frame_count as f64 / (cp.duration_us as f64 / 1e6)
    } else {
        30.0
    };
    let tuning = Tuning {
        expected_tag: (!args.short_id.is_empty()).then(|| expected_tag(&args.short_id)),
        ..Tuning::every_frame(fps)
    };
    let r = declared::build_with(&copy, &original, tuning);

    // ── Terminal summary, in the report's own order ──────────────────────
    println!("\nsignature");
    match tuning.expected_tag {
        Some(w) => {
            let mine = r.tags_seen.iter().find(|(t, _)| *t == w).map_or(0, |(_, n)| *n);
            println!("  expected 0x{w:04X} — carried by {mine} frame(s)");
            for (t, n) in r.tags_seen.iter().filter(|(t, _)| *t != w) {
                println!("  FOREIGN  0x{t:04X} on {n} frame(s)");
            }
        }
        None => {
            for (t, n) in &r.tags_seen {
                println!("  0x{t:04X} on {n} frame(s)");
            }
        }
    }

    println!("\nshots");
    for (i, s) in r.segments.iter().enumerate() {
        println!(
            "  {:>2}  copy {} → {}   original {} → {}   f={}..{}",
            i + 1,
            clock(s.copy_start_us),
            clock(s.copy_end_us),
            clock(s.original_start_us),
            clock(s.original_end_us),
            s.counter_start,
            s.counter_end
        );
        println!(
            "      {} frames · {} conforming · {} differ · {} inconclusive · worst confirmed {}/63",
            s.frames_examined,
            s.frames_confirmed,
            s.differing.len(),
            s.inconclusive.len(),
            s.worst_confirmed_distance
        );
    }
    if r.segments.is_empty() {
        println!("  none — no correspondence established");
    }

    println!("\ncuts");
    if r.cuts.is_empty() {
        println!("  none");
    }
    for c in &r.cuts {
        let what = match c.kind() {
            JoinKind::Backwards => format!(
                "the copy goes back {} frames",
                c.counters_skipped().unsigned_abs()
            ),
            JoinKind::Removal => format!(
                "{} frames of the original absent ({:.2}s)",
                c.counters_skipped(),
                (-c.inserted_us()) as f64 / 1e6
            ),
            JoinKind::Insertion => format!(
                "{:.2}s here that the original does not account for",
                c.inserted_us() as f64 / 1e6
            ),
        };
        println!(
            "  {}  f={} → f={} — {}",
            clock(c.copy_at_us),
            c.counter_before,
            c.counter_after,
            what
        );
    }

    println!("\nstretches corresponding to nothing in the original");
    if r.unconfirmed.is_empty() {
        println!("  none");
    }
    for u in &r.unconfirmed {
        println!(
            "  {} → {}  {} frame(s), {} with a frame number, {} contradicted",
            clock(u.copy_start_us),
            clock(u.copy_end_us),
            u.frames_examined,
            u.frames_declaring,
            u.frames_contradicted
        );
    }

    let differing: usize = r.segments.iter().map(|s| s.differing.len()).sum();
    let inconclusive: usize = r.segments.iter().map(|s| s.inconclusive.len()).sum();
    println!("\nframes whose picture differs: {differing}");
    for (i, s) in r.segments.iter().enumerate() {
        for n in s.differing.iter().take(40) {
            println!(
                "  shot {}  {}  f={}  {}",
                i + 1,
                clock(n.copy_t_us),
                n.counter.map(|c| c.to_string()).unwrap_or("?".into()),
                match n.distance {
                    Some(d) => format!("{d}/63"),
                    None => "not in the original".to_string(),
                }
            );
        }
    }
    println!("frames nothing could be established about: {inconclusive}");

    if let Some(out) = args.out {
        let seconds = (cp.duration_us.max(0) as f64) / 1e6;
        let page = edit_page::render(
            &r,
            &PageInputs {
                short_id: &args.short_id,
                chain_verdict: &args.chain_verdict,
                chain_passed: args.chain_passed,
                original_src: &relative(&out, &args.original),
                copy_src: &relative(&out, &args.copy),
                original_label: &name(&args.original),
                copy_label: &name(&args.copy),
                frames_read: copy.len(),
                seconds_examined: seconds,
            },
        );
        std::fs::write(&out, page)?;
        println!("\nwrote {}", out.display());
    }

    println!("total {:.1}s", started.elapsed().as_secs_f64());
    Ok(())
}

fn name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A path the page can load, relative to where the page is written.
fn relative(page: &Path, target: &Path) -> String {
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
    let rest: PathBuf = bi.collect();
    let mut out = String::new();
    for _ in 0..ups {
        out.push_str("../");
    }
    out.push_str(&rest.to_string_lossy());
    out
}
