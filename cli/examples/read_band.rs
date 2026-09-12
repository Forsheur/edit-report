//! Show what the overlay reader finds in a video's burn-in band.
//!
//! A development aid for the band reader, the way `compare_videos` is one for
//! the alignment. It prints the grid it located and the cells it cut, so a
//! wrong reading can be seen rather than inferred from a wrong report.
//!
//!     cargo run --release -p edit-report --example read_band -- VIDEO [ORIGINAL_WIDTH]
//!
//! With `--t0 <ISO8601>` it also learns the digit shapes and reads the frame
//! counters back, checking them against what the counter must be. That check
//! is the point: a band reader that is confidently wrong is worse than one
//! that refuses, so it has to be measured against a known answer.

use edit_report::decode;
use edit_report_core::frame::LumaFrame;
use edit_report_core::overlay::{find_grid, line1, read_counter, DigitBook, Geometry};
use std::path::Path;

/// `YYYY-MM-DDTHH:MM:SSZ` for `t0 + seconds`, the way the phone formats it.
fn stamp(t0: &str, add_secs: i64) -> Option<String> {
    // Parsed and re-emitted by hand: this is a dev aid and the core takes no
    // date dependency it would then carry into wasm.
    let b = t0.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let num = |a: usize, z: usize| -> i64 { t0[a..z].parse().ok().unwrap_or(0) };
    let (y, mo, d) = (num(0, 4), num(5, 7), num(8, 10));
    let (h, mi, se) = (num(11, 13), num(14, 16), num(17, 19));
    // Days since epoch, Howard Hinnant's days_from_civil.
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total = days * 86_400 + h * 3600 + mi * 60 + se + add_secs;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: read_band <video> [original_width]");
        std::process::exit(2);
    }
    let t0 = args
        .iter()
        .position(|a| a == "--t0")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let args: Vec<String> = args.into_iter().filter(|a| a != "--t0").collect();
    let args: Vec<String> = match &t0 {
        Some(v) => args.into_iter().filter(|a| a != v).collect(),
        None => args,
    };
    let path = Path::new(&args[0]);
    let profile = decode::probe(path, None)?;
    let geom = match args.get(1).and_then(|w| w.parse::<u32>().ok()) {
        Some(ow) => Geometry::scaled_from(ow, profile.width, profile.height),
        None => Geometry::native(profile.width, profile.height),
    };
    println!(
        "{}: {}×{}  scale {:.3}  band {} rows  nominal pitch {:.2}",
        path.display(),
        profile.width,
        profile.height,
        geom.scale,
        geom.band().h,
        geom.nominal_pitch(),
    );

    if let Some(t0) = &t0 {
        return learn_and_check(path, &profile, &geom, t0);
    }

    // A handful of frames spread across the video is enough to see whether the
    // grid is found consistently.
    let stride = (profile.frame_count / 8).max(1);
    let mut n = 0;
    decode::decode_stream(path, &profile, stride, |f| {
        if n >= 8 {
            return;
        }
        n += 1;
        print!("  frame {:>6}: ", f.index);
        for line in 0..3 {
            match find_grid(&f, &geom, line) {
                Some(g) => print!(
                    "L{line}[{} cells, pitch {:.1}, x {}..{}]  ",
                    g.cells, g.pitch, g.left, g.right
                ),
                None => print!("L{line}[none]  "),
            }
        }
        if let Some(g) = find_grid(&f, &geom, 0) {
            let expect_ts = line1::TIMESTAMP_CELLS;
            print!(
                "→ counter cells {}..{}",
                line1::COUNTER_FIRST_CELL,
                g.cells.saturating_sub(1)
            );
            if g.cells < expect_ts + 4 {
                print!("  (line 1 shorter than a timestamp + counter)");
            }
        }
        println!();
    })?;
    Ok(())
}

/// Learn the digit shapes from the timestamp, then read the counters back and
/// check them against what they must be.
fn learn_and_check(
    path: &Path,
    profile: &edit_report_core::report::MediaProfile,
    geom: &Geometry,
    t0: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let fps = profile.nominal_fps.unwrap_or(30.0);

    // Learn from frames spread across a minute, so the seconds field walks
    // through every digit.
    let stride = ((fps * 2.0) as u64).max(1);
    let mut learn: Vec<(LumaFrame, String)> = Vec::new();
    decode::decode_stream(path, profile, stride, |f| {
        if learn.len() < 40 {
            if let Some(s) = stamp(t0, (f.index as f64 / fps) as i64) {
                learn.push((f, s));
            }
        }
    })?;
    let refs: Vec<(&LumaFrame, String)> = learn.iter().map(|(f, s)| (f, s.clone())).collect();
    let (book, offset) = DigitBook::learn_two_pass(&refs, geom);
    println!(
        "learned {} of 10 digit shapes from {} frame(s); counter − frame index fitted at {}",
        book.learned_digits(),
        refs.len(),
        offset
            .map(|k| format!("{k:+}"))
            .unwrap_or_else(|| "unknown".into()),
    );
    if !book.is_complete() {
        println!("  (an incomplete book still reads, it just refuses more cells)");
    }

    // Read counters back. The counter must be the frame index plus a constant,
    // so the spread of (counter - index) is the whole check: one value means
    // every read agrees.
    let mut offsets: std::collections::BTreeMap<i64, usize> = Default::default();
    let mut why: std::collections::BTreeMap<String, usize> = Default::default();
    let (mut read, mut total) = (0usize, 0usize);
    decode::decode_stream(path, profile, (fps as u64).max(1), |f| {
        total += 1;
        let r = read_counter(&f, geom, &book);
        match r.counter {
            Some(c) => {
                read += 1;
                *offsets.entry(c as i64 - f.index as i64).or_default() += 1;
            }
            None => {
                if std::env::var_os("EDIT_REPORT_DUMP_FAIL").is_some() && why.is_empty() {
                    if let Some(grid) = find_grid(&f, geom, 0) {
                        println!(
                            "\n  FAILING frame {} — grid {} cells, pitch {:.2}",
                            f.index, grid.cells, grid.pitch
                        );
                        println!(
                            "  anchor {:?}, cells after anchor {:?}",
                            grid.anchor,
                            grid.cells_after_anchor()
                        );
                        for i in 0..4usize {
                            if let Some(c) =
                                edit_report_core::overlay::counter_digit_cell(&f, geom, &grid, i)
                            {
                                println!(
                                    "  cell -{i} from right: read {:?}  ink {:.2}",
                                    book.classify(&c),
                                    c.ink_fraction()
                                );
                                for row in c.rows() {
                                    println!("      {row}");
                                }
                            }
                        }
                    }
                }
                let k = format!(
                    "{:?} ({}/{} digits)",
                    r.refusal, r.counter_digits_read, r.counter_digits_total
                );
                *why.entry(k).or_default() += 1;
            }
        }
    })?;
    println!("counter read on {read} of {total} frame(s) examined");
    for (k, n) in &why {
        println!("  refused: {k} × {n}");
    }
    for (off, n) in &offsets {
        println!("  counter − frame index = {off:+}  on {n} frame(s)");
    }
    match offsets.len() {
        0 => println!("  VERDICT: no counter read"),
        1 => println!("  VERDICT: every read agrees — the counter tracks the frame index exactly"),
        _ => println!("  VERDICT: reads disagree; at least one is wrong"),
    }
    Ok(())
}
