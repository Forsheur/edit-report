//! The edit report from two video files, without a bundle.
//!
//!     cargo run --release -p edit-report --example edit_report -- \
//!         ORIGINAL COPY [--out report.html] [--short-id XXXX] [--sensitivity N]
//!
//! The development driver. The binary is the product: it takes a bundle, runs
//! its verifier and extracts the media. This takes two paths and skips all of
//! that, which is what you want when iterating on the measurement itself. It
//! prints; nothing it prints should be shown to a reader as a finding.
//!
//! The pass itself lives in `editpass.rs` and is the same one the binary runs.
//! It used to be duplicated here, and a duplicate of the path the whole report
//! rests on is a thing a reader would have to check twice.

use edit_report::{decode, editpass};
use edit_report_core::declared::JoinKind;
use edit_report_core::edit_page::{self, clock, PageInputs};
use edit_report_core::imagediff::{DiffSettings, DifferenceState};
use std::path::{Path, PathBuf};

struct Args {
    original: PathBuf,
    copy: PathBuf,
    out: Option<PathBuf>,
    short_id: String,
    chain_verdict: String,
    chain_passed: bool,
    diff: DiffSettings,
}

fn parse() -> Result<Args, String> {
    let mut a = std::env::args().skip(1);
    let original = a
        .next()
        .ok_or("usage: edit_report ORIGINAL COPY [options]")?;
    let copy = a
        .next()
        .ok_or("usage: edit_report ORIGINAL COPY [options]")?;
    let mut out = None;
    let mut short_id = String::new();
    let mut chain_verdict = "not run here — this driver compares pictures only".to_string();
    let mut chain_passed = true;
    let mut diff = DiffSettings::default();
    while let Some(flag) = a.next() {
        let mut value = || a.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(value()?)),
            "--short-id" => short_id = value()?,
            "--chain" => chain_verdict = value()?,
            "--chain-failed" => chain_passed = false,
            "--sensitivity" => {
                diff.sensitivity = value()?
                    .parse()
                    .map_err(|_| "--sensitivity wants a number")?
            }
            "--diff-grid" => {
                let n: u32 = value()?.parse().map_err(|_| "--diff-grid wants a number")?;
                diff.cols = n;
                diff.rows = n;
            }
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
        diff,
    })
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

    let pass = editpass::run(&args.original, &args.copy, &args.short_id, args.diff)?;
    let r = &pass.correspondence;
    println!(
        "read every frame: original {}, copy {}, in {:.1}s",
        pass.original_frames,
        pass.copy_frames,
        pass.elapsed.as_secs_f64()
    );

    if !pass.original_is_readable() {
        println!(
            "\nNo strip could be read on any frame of the original. This driver reads the \
             machine-readable strip only; a recording made before it existed has to go \
             through the text path instead."
        );
        return Ok(());
    }

    println!("\nsignature");
    match r.tag_expected {
        Some(w) => {
            let mine = r
                .tags_seen
                .iter()
                .find(|(t, _)| *t == w)
                .map_or(0, |(_, n)| *n);
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
    if r.segments.is_empty() {
        println!("  none — no correspondence established");
    }
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

    let count = |w: DifferenceState| {
        pass.located
            .iter()
            .filter(|l| l.difference.state == w)
            .count()
    };
    println!(
        "\npicture compared on {} frame(s): {} even, {} located, {} inconclusive",
        pass.located.len(),
        count(DifferenceState::ConsistentWithRecompression),
        count(DifferenceState::LocalizedDifference),
        count(DifferenceState::Inconclusive),
    );

    if let Some(out) = args.out {
        let page = edit_page::render(
            r,
            &PageInputs {
                short_id: &args.short_id,
                chain_verdict: &args.chain_verdict,
                chain_passed: args.chain_passed,
                chain_checked: false,
                original_src: &editpass::relative(&out, &args.original),
                copy_src: &editpass::relative(&out, &args.copy),
                original_label: &name(&args.original),
                copy_label: &name(&args.copy),
                frames_read: pass.copy_frames,
                seconds_examined: (pass.copy_duration_us.max(0) as f64) / 1e6,
                located: &pass.located,
                diff: pass.diff_settings,
                located_ran: true,
                out_of_place: &pass.out_of_place,
            },
        );
        std::fs::write(&out, page)?;
        println!("\nwrote {}", out.display());
    }
    Ok(())
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
