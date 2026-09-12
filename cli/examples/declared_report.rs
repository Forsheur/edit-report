//! Read the copy's burned-in counters, put every one of them to the original,
//! and describe the edit that results.
//!
//! The development driver for the declaration path — the product wiring goes
//! into `edit-report` itself once this is settled. It prints; it never writes a
//! report, and nothing it prints should be shown to a reader as a finding.
//!
//!     cargo run --release -p edit-report --example declared_report -- \
//!         ORIGINAL COPY --t0 <ISO8601 of the original's first frame>
//!
//! `--t0` is only needed to teach the digit shapes. In the product it comes
//! from the bundle's signed `phone_time_us`; here it is given by hand.

use edit_report::decode;
use edit_report_core::declared::{self, Declared, OriginalFrame, CONFIRM_DISTANCE};
use edit_report_core::fingerprint::fingerprint;
use edit_report_core::frame::LumaFrame;
use edit_report_core::normalize::Plan;
use edit_report_core::overlay::{read_counter_any, CounterSource, DigitBook, Geometry};
use std::path::Path;

fn clock(us: i64) -> String {
    let t = us.max(0);
    format!("{}:{:05.2}", t / 60_000_000, (t % 60_000_000) as f64 / 1e6)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for `t0 + seconds`.
fn stamp(t0: &str, add: i64) -> Option<String> {
    if t0.len() < 20 {
        return None;
    }
    let n = |a: usize, z: usize| -> i64 { t0[a..z].parse().unwrap_or(0) };
    let (y, mo, d, h, mi, se) = (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19));
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let total = (era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + se + add;

    let (days, rem) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    Some(format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    ))
}

struct Side {
    profile: edit_report_core::report::MediaProfile,
    plan: Plan,
    geom: Geometry,
}

fn open(path: &Path, original_width: Option<u32>) -> Result<Side, Box<dyn std::error::Error>> {
    let profile = decode::probe(path, None)?;
    let bars = decode::decode_spread(path, &profile, 24)?;
    let plan = Plan::from_frames(&bars).ok_or("no frames decoded")?;
    let geom = match original_width {
        Some(ow) => Geometry::scaled_from(ow, profile.width, profile.height),
        None => Geometry::native(profile.width, profile.height),
    };
    Ok(Side {
        profile,
        plan,
        geom,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let t0 = args
        .iter()
        .position(|a| a == "--t0")
        .and_then(|i| args.get(i + 1).cloned());
    let paths: Vec<&String> = args
        .iter()
        .filter(|a| *a != "--t0" && Some(*a) != t0.as_ref())
        .collect();
    if paths.len() != 2 {
        eprintln!("usage: declared_report <original> <copy> --t0 <ISO8601>");
        std::process::exit(2);
    }
    let (op, cp) = (Path::new(paths[0]), Path::new(paths[1]));
    let t0 = t0.ok_or("--t0 is required to teach the digit shapes")?;

    let orig = open(op, None)?;
    let copy = open(cp, Some(orig.profile.width))?;
    let fps = orig.profile.nominal_fps.unwrap_or(30.0);
    println!(
        "original {}×{} {} frames   copy {}×{} {} frames (scale {:.3})",
        orig.profile.width,
        orig.profile.height,
        orig.profile.frame_count,
        copy.profile.width,
        copy.profile.height,
        copy.profile.frame_count,
        copy.geom.scale,
    );

    // ── Teach the digits from the original's own timestamps ────────────────
    let mut learn: Vec<(LumaFrame, String)> = Vec::new();
    decode::decode_stream(op, &orig.profile, ((fps * 2.0) as u64).max(1), |f| {
        if learn.len() < 40 {
            if let Some(s) = stamp(&t0, (f.index as f64 / fps) as i64) {
                learn.push((f, s));
            }
        }
    })?;
    let refs: Vec<(&LumaFrame, String)> = learn.iter().map(|(f, s)| (f, s.clone())).collect();
    let (book, offset) = DigitBook::learn_two_pass(&refs, &orig.geom);
    println!(
        "digit shapes learned: {} of 10; counter − frame index fitted at {}",
        book.learned_digits(),
        offset
            .map(|k| format!("{k:+}"))
            .unwrap_or_else(|| "unknown".into()),
    );

    // ── Both sides, sampled at the same rate in time ───────────────────────
    let rate = decode::SAMPLES_PER_SECOND;
    let mut originals: Vec<OriginalFrame> = Vec::new();
    let mut orig_from_strip = 0usize;
    let mut copy_from_strip = 0usize;
    let mut copy_tags: std::collections::BTreeMap<u16, usize> = Default::default();
    decode::decode_stream(
        op,
        &orig.profile,
        decode::stride_for_rate(&orig.profile, rate),
        |f| {
            // Normalise BEFORE reading: both readers measure from the top-left
            // of the picture, and a letterboxed frame has neither where the
            // phone drew it.
            let Some(n) = orig.plan.apply(&f) else { return };
            let r = read_counter_any(&n, &orig.geom, &book);
            if let Some(counter) = r.counter {
                if r.source == Some(CounterSource::BitRow) {
                    orig_from_strip += 1;
                }
                originals.push(OriginalFrame {
                    counter,
                    index: f.index,
                    t_us: f.pts_us,
                    fp: fingerprint(&n),
                });
            }
        },
    )?;

    let mut copies: Vec<Declared> = Vec::new();
    decode::decode_stream(
        cp,
        &copy.profile,
        decode::stride_for_rate(&copy.profile, rate),
        |f| {
            let Some(n) = copy.plan.apply(&f) else { return };
            let r = read_counter_any(&n, &copy.geom, &book);
            if r.source == Some(CounterSource::BitRow) {
                copy_from_strip += 1;
            }
            if let Some(t) = r.session_tag {
                *copy_tags.entry(t).or_insert(0usize) += 1;
            }
            copies.push(Declared {
                copy_index: f.index,
                copy_t_us: f.pts_us,
                counter: r.counter,
                fp: fingerprint(&n),
            });
        },
    )?;
    println!(
        "original: {} frame(s) with a readable counter ({orig_from_strip} from the strip)   \
         copy: {} frame(s) examined ({copy_from_strip} from the strip)",
        originals.len(),
        copies.len()
    );
    if copy_tags.len() > 1 {
        let list: Vec<String> = copy_tags
            .iter()
            .map(|(t, n)| format!("0x{t:04X}×{n}"))
            .collect();
        println!(
            "  the copy's strip names {} different recordings: {}",
            copy_tags.len(),
            list.join(", ")
        );
    }

    // ── What the declarations establish ────────────────────────────────────
    let r = declared::build(&copies, &originals, CONFIRM_DISTANCE);
    println!(
        "\ndeclared {} of {} · confirmed {} · contradicted {}{}",
        r.frames_declaring,
        r.frames_examined,
        r.frames_confirmed,
        r.frames_contradicted,
        if r.counters_repeated > 0 {
            format!(" · {} counter(s) seen twice", r.counters_repeated)
        } else {
            String::new()
        }
    );
    for (i, s) in r.segments.iter().enumerate() {
        println!(
            "  seg{:<2} copy {} → {}   original {} → {}   f={}..{}   {} frames, mean {:.1}/63",
            i + 1,
            clock(s.copy_start_us),
            clock(s.copy_end_us),
            clock(s.original_start_us),
            clock(s.original_end_us),
            s.counter_start,
            s.counter_end,
            s.frames_confirmed,
            s.mean_distance,
        );
    }
    for c in &r.cuts {
        println!(
            "  cut    copy {}   f={} → f={}   {} frame(s) of the original {}",
            clock(c.copy_at_us),
            c.counter_before,
            c.counter_after,
            c.counters_skipped().abs(),
            if c.goes_backwards() {
                "earlier — the copy goes back"
            } else {
                "skipped"
            },
        );
    }
    for u in &r.unconfirmed {
        println!(
            "  gap    copy {} → {}   {} frame(s) examined, {} declared, {} contradicted",
            clock(u.copy_start_us),
            clock(u.copy_end_us),
            u.frames_examined,
            u.frames_declaring,
            u.frames_contradicted,
        );
    }
    Ok(())
}
