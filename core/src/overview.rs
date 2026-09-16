//! The whole comparison at a glance: two timelines and the bands between them.
//!
//! The picture a side-by-side diff draws, applied to time. The original runs
//! down the left in its own time, the supplied file down the right in its own,
//! and every shot that corresponds is a ribbon joining the two: horizontal
//! when the offset is nil, slanted when it is not, crossing another when the
//! order was changed. Material with no counterpart sits alone on its column.
//!
//! **Colour is the original's time.** Blue at its first frame, orange at its
//! last; the original's column is that gradient, and each ribbon keeps the
//! hue of where it comes from. A thin strip on the supplied file's column
//! shows where each of its frames came from, so a reordering reads on that
//! column alone — the end's hue above the start's — before the ribbons are
//! even followed. The rest of that column shows each frame's state, in the
//! report's own three states plus "from another recording".
//!
//! Fixed height, each column scaled to its own duration, three pixels minimum
//! for any event so a single frame stays visible at any length. No frame is
//! drawn: the players are the way to look, and a click anywhere here puts
//! them on that moment.
//!
//! The SVG is generated here rather than in the page's JavaScript so it is
//! covered by the same tests as the rest of the report and inspectable as
//! text by anyone reading the page. The script adds only what moves.

use crate::declared::{Declared, DeclaredCorrespondence, FrameVerdict, OriginalFrame};
use crate::edit_page::clock;
use crate::imagediff::{DifferenceState, Located, OutOfPlace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameState {
    /// Corresponds, and differs evenly the way a recompression does.
    Conforming,
    /// A located region, a frame out of place with its neighbours, or a
    /// picture that does not match the frame it declares.
    Differs,
    /// No strip read, or too far to judge either way.
    Inconclusive,
    /// Carries another recording's signature.
    Foreign,
}

/// A run of consecutive supplied-file frames in one state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Run {
    pub start_us: i64,
    pub end_us: i64,
    pub frames: usize,
    pub state: FrameState,
}

/// A stretch of the original with no counterpart in the supplied file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start_us: i64,
    pub end_us: i64,
}

/// One shot as a ribbon: its span on each side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Band {
    pub o0: i64,
    pub o1: i64,
    pub c0: i64,
    pub c1: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Overview {
    pub original_duration_us: i64,
    pub copy_duration_us: i64,
    pub bands: Vec<Band>,
    pub copy_runs: Vec<Run>,
    pub original_absent: Vec<Span>,
}

/// Median interval between consecutive timestamps — one frame, robust to a
/// few dropped or irregular ones.
fn period_us(times: &[i64]) -> i64 {
    let mut d: Vec<i64> = times
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 0)
        .collect();
    if d.is_empty() {
        return 33_333;
    }
    d.sort_unstable();
    d[d.len() / 2]
}

pub fn build(
    copy: &[Declared],
    originals: &[OriginalFrame],
    r: &DeclaredCorrespondence,
    located: &[Located],
    out_of_place: &[OutOfPlace],
) -> Overview {
    use std::collections::HashSet;

    let copy_times: Vec<i64> = copy.iter().map(|c| c.copy_t_us).collect();
    let cp = period_us(&copy_times);
    let mut orig_times: Vec<i64> = originals.iter().map(|o| o.t_us).collect();
    orig_times.sort_unstable();
    let op = period_us(&orig_times);

    let copy_duration_us = copy_times.last().map_or(0, |t| t + cp);
    let original_duration_us = orig_times.last().map_or(0, |t| t + op);

    // What each supplied frame is, from what the report already established.
    let differs_at: HashSet<u64> = located
        .iter()
        .filter(|l| l.difference.state == DifferenceState::LocalizedDifference)
        .map(|l| l.copy_index)
        .chain(out_of_place.iter().map(|o| o.copy_index))
        .collect();
    let inconclusive_at: HashSet<u64> = located
        .iter()
        .filter(|l| l.difference.state == DifferenceState::Inconclusive)
        .map(|l| l.copy_index)
        .collect();

    let state_of = |i: usize, c: &Declared| -> FrameState {
        match r
            .verdicts
            .get(i)
            .copied()
            .unwrap_or(FrameVerdict::NoDeclaration)
        {
            FrameVerdict::Foreign => FrameState::Foreign,
            FrameVerdict::Contradicted | FrameVerdict::DeclaredOutsideOriginal => {
                FrameState::Differs
            }
            FrameVerdict::NoDeclaration | FrameVerdict::Unsettled => FrameState::Inconclusive,
            FrameVerdict::Confirmed => {
                if differs_at.contains(&c.copy_index) {
                    FrameState::Differs
                } else if inconclusive_at.contains(&c.copy_index) {
                    FrameState::Inconclusive
                } else {
                    FrameState::Conforming
                }
            }
        }
    };

    let mut copy_runs: Vec<Run> = Vec::new();
    for (i, c) in copy.iter().enumerate() {
        let st = state_of(i, c);
        match copy_runs.last_mut() {
            Some(run) if run.state == st => {
                run.end_us = c.copy_t_us + cp;
                run.frames += 1;
            }
            _ => copy_runs.push(Run {
                start_us: c.copy_t_us,
                end_us: c.copy_t_us + cp,
                frames: 1,
                state: st,
            }),
        }
    }

    let mut bands: Vec<Band> = r
        .segments
        .iter()
        .map(|s| Band {
            o0: s.original_start_us,
            o1: s.original_end_us + op,
            c0: s.copy_start_us,
            c1: s.copy_end_us + cp,
        })
        .collect();

    // Original frames no shot accounts for: the gaps between shots in the
    // original's own order, plus anything before the first or after the last.
    let mut by_original = bands.clone();
    by_original.sort_by_key(|b| b.o0);
    let mut original_absent = Vec::new();
    let mut cursor = 0i64;
    for b in &by_original {
        if b.o0 > cursor + op {
            original_absent.push(Span {
                start_us: cursor,
                end_us: b.o0,
            });
        }
        cursor = cursor.max(b.o1);
    }
    if original_duration_us > cursor + op {
        original_absent.push(Span {
            start_us: cursor,
            end_us: original_duration_us,
        });
    }
    bands.sort_by_key(|b| b.c0);

    Overview {
        original_duration_us,
        copy_duration_us,
        bands,
        copy_runs,
        original_absent,
    }
}

// ── Drawing ───────────────────────────────────────────────────────────────

const HEIGHT: f64 = 560.0;
const TOP: f64 = 22.0;
const RIBBON: f64 = 26.0;
const X_ORIGINAL: f64 = 70.0;
const X_COPY: f64 = 300.0;
const WIDTH: f64 = X_COPY + RIBBON + LEADER_LEN + LABEL_ROOM;
/// A single frame keeps this much height at any scale.
const MIN_EVENT_PX: f64 = 3.0;
/// An event — a run that is not conforming, a stretch the copy lacks —
/// keeps this much, more than a frame's share: three pixels of orange
/// inside a 26-pixel column were not found by the eye. The count in the
/// tooltip stays exact; only the height is exaggerated, and the note under
/// the figure says so.
const MIN_ANOMALY_PX: f64 = 8.0;
/// Every event is named beside the drawing, in its own colour, with a
/// leader from the column: a tooltip on an eight-pixel block is a thing a
/// reader has to know to look for, a label is not. Labels that would
/// overprint are pushed down and their leaders slant.
const LEADER_LEN: f64 = 14.0;
const LABEL_STEP: f64 = 12.0;
/// The labels of the supplied file's events go to its right; those of the
/// original's absent stretches go to ITS right too, into the gap the
/// ribbons leave — an absent stretch is exactly a stretch no ribbon
/// leaves from.
const LABEL_ROOM: f64 = 150.0;

fn hue(t_us: i64, duration_us: i64) -> String {
    let f = if duration_us > 0 {
        (t_us as f64 / duration_us as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Blue at the start, orange at the end, through the greens between.
    format!("hsl({} 70% 55%)", (210.0 - 180.0 * f).round() as i64)
}

fn px(v: f64) -> String {
    format!("{:.1}", v)
}

fn state_fill(s: FrameState) -> &'static str {
    match s {
        FrameState::Conforming => "#9fb4c8",
        FrameState::Differs => "#e08a2e",
        FrameState::Inconclusive => "#c9c9c9",
        FrameState::Foreign => "#a061c9",
    }
}

/// Whole seconds, for the axis ticks. The tooltips use the report's own
/// clock, to the hundredth, so a moment reads the same here and in the
/// tables.
fn tick(us: i64) -> String {
    let s = us.max(0) / 1_000_000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The figure. Every coordinate the script needs to map a click back to a
/// moment is on the root element as a data attribute.
pub fn svg(o: &Overview) -> String {
    let dmax = o.original_duration_us.max(o.copy_duration_us).max(1) as f64;
    let scale = HEIGHT / dmax; // px per µs
    let y = |us: i64| TOP + us as f64 * scale;
    let h = |a: i64, b: i64| ((b - a) as f64 * scale).max(MIN_EVENT_PX);
    let ha = |a: i64, b: i64| ((b - a) as f64 * scale).max(MIN_ANOMALY_PX);
    // Labels: laid out per side, top to bottom, each at its event's middle
    // unless the previous label is in the way, in which case just below it.
    // `items` are (y_mid, colour, text, title); the leader leaves the column
    // at `x_edge`, the text starts LEADER_LEN + 4 further right.
    let labels = |x_edge: f64, items: &[(f64, &str, String, String)]| -> String {
        let mut out = String::new();
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by(|&i, &j| items[i].0.total_cmp(&items[j].0));
        let mut last = f64::NEG_INFINITY;
        for i in order {
            let (ym, colour, text, title) = &items[i];
            let yl = ym.max(last + LABEL_STEP);
            last = yl;
            out.push_str(&format!(
                r##"<line class="ov-leader" x1="{}" x2="{}" y1="{}" y2="{}" stroke="{}" stroke-width="2"/><text class="ov-label" x="{}" y="{}" fill="{}"><title>{}</title>{}</text>"##,
                px(x_edge), px(x_edge + LEADER_LEN), px(*ym), px(yl), colour,
                px(x_edge + LEADER_LEN + 4.0), px(yl + 3.5), colour, title, text
            ));
        }
        out
    };
    let do_ = o.original_duration_us;

    let mut defs = String::new();
    let mut body = String::new();

    // Original column: the time gradient, then what the copy lacks.
    defs.push_str(&format!(
        r##"<linearGradient id="ov-o" gradientUnits="userSpaceOnUse" x1="0" y1="{}" x2="0" y2="{}"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></linearGradient>"##,
        px(y(0)), px(y(do_)), hue(0, do_), hue(do_, do_)
    ));
    body.push_str(&format!(
        r##"<rect class="ov-orig" x="{}" y="{}" width="{}" height="{}" fill="url(#ov-o)"/>"##,
        px(X_ORIGINAL),
        px(y(0)),
        px(RIBBON),
        px(h(0, do_))
    ));
    let mut absent_labels: Vec<(f64, &str, String, String)> = Vec::new();
    for a in &o.original_absent {
        let hh = ha(a.start_us, a.end_us);
        let title = format!(
            "original {} → {}: no counterpart in the supplied file",
            clock(a.start_us),
            clock(a.end_us)
        );
        body.push_str(&format!(
            r##"<rect class="ov-absent" x="{}" y="{}" width="{}" height="{}" fill="#fff" stroke="#999" stroke-dasharray="3,2"><title>{}</title></rect>"##,
            px(X_ORIGINAL), px(y(a.start_us)), px(RIBBON), px(hh), title
        ));
        absent_labels.push((
            y(a.start_us) + hh / 2.0,
            "#666",
            format!("{:.2} s absent", (a.end_us - a.start_us) as f64 / 1e6),
            title,
        ));
    }

    // Ribbons, each in the hue of where it comes from, curved so two that
    // cross read as two objects rather than two overlapping trapezoids.
    let x1 = X_ORIGINAL + RIBBON;
    let x2 = X_COPY;
    let xm = (x1 + x2) / 2.0;
    for b in &o.bands {
        let c = hue((b.o0 + b.o1) / 2, do_);
        body.push_str(&format!(
            r##"<path class="ov-band" d="M{x1},{oy0} C{xm},{oy0} {xm},{cy0} {x2},{cy0} L{x2},{cy1} C{xm},{cy1} {xm},{oy1} {x1},{oy1} Z" fill="{c}" fill-opacity=".45" stroke="{c}" stroke-width=".8"><title>original {} → {} ↔ supplied {} → {}</title></path>"##,
            clock(b.o0), clock(b.o1), clock(b.c0), clock(b.c1),
            x1 = px(x1), x2 = px(x2), xm = px(xm),
            oy0 = px(y(b.o0)), oy1 = px(y(b.o1)), cy0 = px(y(b.c0)), cy1 = px(y(b.c1)), c = c
        ));
    }

    // Supplied column: state per run, then the provenance strip on its edge.
    // Two passes: the conforming background first, the events on top —
    // an event grown past its true height would otherwise be painted over
    // by the run that follows it.
    let (ground, events): (Vec<&Run>, Vec<&Run>) = o
        .copy_runs
        .iter()
        .partition(|r| r.state == FrameState::Conforming);
    let mut run_labels: Vec<(f64, &str, String, String)> = Vec::new();
    for r in ground.into_iter().chain(events) {
        let event = r.state != FrameState::Conforming;
        let hh = if event {
            ha(r.start_us, r.end_us)
        } else {
            h(r.start_us, r.end_us)
        };
        let word = match r.state {
            FrameState::Conforming => "conforming",
            FrameState::Differs => "differs",
            FrameState::Inconclusive => "inconclusive",
            FrameState::Foreign => "from another recording",
        };
        let title = format!(
            "supplied {} → {}: {} frame(s), {}",
            clock(r.start_us),
            clock(r.end_us),
            r.frames,
            word
        );
        body.push_str(&format!(
            r##"<rect class="ov-run" x="{}" y="{}" width="{}" height="{}" fill="{}"><title>{}</title></rect>"##,
            px(X_COPY), px(y(r.start_us)), px(RIBBON), px(hh), state_fill(r.state), title
        ));
        if event {
            let short = match r.state {
                FrameState::Foreign => "foreign",
                _ => word,
            };
            run_labels.push((
                y(r.start_us) + hh / 2.0,
                // The inconclusive grey is right for a block and too faint
                // for text on white.
                match r.state {
                    FrameState::Inconclusive => "#8a8a8a",
                    _ => state_fill(r.state),
                },
                format!(
                    "{} frame{} {}",
                    r.frames,
                    if r.frames == 1 { "" } else { "s" },
                    short
                ),
                title,
            ));
        }
    }
    for (i, b) in o.bands.iter().enumerate() {
        defs.push_str(&format!(
            r##"<linearGradient id="ov-p{i}" gradientUnits="userSpaceOnUse" x1="0" y1="{}" x2="0" y2="{}"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></linearGradient>"##,
            px(y(b.c0)), px(y(b.c1)), hue(b.o0, do_), hue(b.o1, do_)
        ));
        body.push_str(&format!(
            r##"<rect class="ov-from" x="{}" y="{}" width="7" height="{}" fill="url(#ov-p{i})"/>"##,
            px(X_COPY),
            px(y(b.c0)),
            px(h(b.c0, b.c1))
        ));
    }

    // Ticks, spaced so there are never more than about twelve. One scale,
    // so one set of numbers, on the left; the supplied column keeps the
    // marks and gives its right side to the labels.
    let step_s = [5i64, 10, 30, 60, 120, 300, 600, 1800, 3600]
        .into_iter()
        .find(|s| dmax / (*s as f64 * 1e6) <= 12.0)
        .unwrap_or(3600);
    let mut t = 0i64;
    while (t as f64) * 1e6 <= dmax {
        let us = t * 1_000_000;
        if us <= o.original_duration_us {
            body.push_str(&format!(
                r##"<text x="{}" y="{}" text-anchor="end" fill="#666">{}</text><line x1="{}" x2="{}" y1="{}" y2="{}" stroke="#666"/>"##,
                px(X_ORIGINAL - 8.0), px(y(us) + 3.0), tick(us),
                px(X_ORIGINAL - 4.0), px(X_ORIGINAL), px(y(us)), px(y(us))
            ));
        }
        if us <= o.copy_duration_us {
            body.push_str(&format!(
                r##"<line x1="{}" x2="{}" y1="{}" y2="{}" stroke="#666"/>"##,
                px(X_COPY + RIBBON),
                px(X_COPY + RIBBON + 4.0),
                px(y(us)),
                px(y(us))
            ));
        }
        t += step_s;
    }

    // Labels last, over everything: the absent ones into the gap right of
    // the original, the event ones right of the supplied file.
    body.push_str(&labels(X_ORIGINAL + RIBBON, &absent_labels));
    body.push_str(&labels(X_COPY + RIBBON, &run_labels));

    // Playheads, moved by the script.
    for (id, x) in [("ov-ph-o", X_ORIGINAL), ("ov-ph-c", X_COPY)] {
        body.push_str(&format!(
            r##"<line id="{id}" x1="{}" x2="{}" y1="{}" y2="{}" stroke="#000" stroke-width="1.5"/>"##,
            px(x - 10.0), px(x + RIBBON + 10.0), px(TOP), px(TOP)
        ));
    }

    format!(
        r##"<svg class="overview" width="{}" height="{}" viewBox="0 0 {} {}" font-family="-apple-system,BlinkMacSystemFont,sans-serif" font-size="10" data-top="{}" data-scale="{}" data-xo="{}" data-xc="{}" data-w="{}" data-dur-o="{}" data-dur-c="{}"><defs>{}</defs><text x="{}" y="12" font-weight="600">original</text><text x="{}" y="12" font-weight="600">supplied file</text>{}</svg>"##,
        px(WIDTH),
        px(HEIGHT + TOP + 20.0),
        px(WIDTH),
        px(HEIGHT + TOP + 20.0),
        px(TOP),
        scale * 1e6,
        px(X_ORIGINAL),
        px(X_COPY),
        px(RIBBON),
        o.original_duration_us as f64 / 1e6,
        o.copy_duration_us as f64 / 1e6,
        defs,
        px(X_ORIGINAL),
        px(X_COPY),
        body
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declared::{DeclaredSegment, Tuning};
    use crate::fingerprint::Fingerprint;

    fn fp() -> Fingerprint {
        Fingerprint {
            bits: 0,
            spread: 1.0,
        }
    }

    /// n copy frames at 30 fps, all confirmed, one shot mapping them onto the
    /// original starting at `o_start_us`.
    fn one_shot(
        n: usize,
        o_start_us: i64,
        c_start_us: i64,
    ) -> (Vec<Declared>, DeclaredCorrespondence) {
        let copy: Vec<Declared> = (0..n)
            .map(|i| Declared {
                copy_index: i as u64,
                copy_t_us: c_start_us + i as i64 * 33_333,
                counter: Some(i as u64 + 1),
                tag: None,
                fp: fp(),
            })
            .collect();
        let mut r = crate::declared::build_with(&[], &[], Tuning::default());
        r.verdicts = vec![FrameVerdict::Confirmed; n];
        r.segments = vec![DeclaredSegment {
            copy_start_us: c_start_us,
            copy_end_us: c_start_us + (n as i64 - 1) * 33_333,
            counter_start: 1,
            counter_end: n as u64,
            original_start_us: o_start_us,
            original_end_us: o_start_us + (n as i64 - 1) * 33_333,
            frames_examined: n,
            frames_confirmed: n,
            differing: vec![],
            inconclusive: vec![],
            worst_confirmed_distance: 2,
        }];
        (copy, r)
    }

    fn originals(n: usize) -> Vec<OriginalFrame> {
        (0..n)
            .map(|i| OriginalFrame {
                counter: i as u64 + 1,
                index: i as u64,
                t_us: i as i64 * 33_333,
                fp: fp(),
            })
            .collect()
    }

    #[test]
    fn a_faithful_copy_is_one_band_and_one_run() {
        let (copy, r) = one_shot(300, 0, 0);
        let o = build(&copy, &originals(300), &r, &[], &[]);
        assert_eq!(o.bands.len(), 1);
        assert_eq!(o.copy_runs.len(), 1);
        assert_eq!(o.copy_runs[0].state, FrameState::Conforming);
        assert!(o.original_absent.is_empty());
        let s = svg(&o);
        assert!(s.contains("class=\"ov-band\""));
        assert!(!s.contains("class=\"ov-absent\""));
    }

    #[test]
    fn removed_material_is_an_absent_block_on_the_original() {
        // The copy has frames 1..100 then 281..300 of a 300-frame original.
        let (mut copy, mut r) = one_shot(100, 0, 0);
        let (tail, r2) = one_shot(20, 280 * 33_333, 100 * 33_333);
        copy.extend(tail);
        r.verdicts.extend(r2.verdicts);
        r.segments.extend(r2.segments);
        let o = build(&copy, &originals(300), &r, &[], &[]);
        assert_eq!(o.bands.len(), 2);
        assert_eq!(o.original_absent.len(), 1, "{:?}", o.original_absent);
        let a = o.original_absent[0];
        assert!((a.start_us - 100 * 33_333).abs() < 40_000);
        assert!((a.end_us - 280 * 33_333).abs() < 40_000);
        assert!(svg(&o).contains("no counterpart in the supplied file"));
    }

    #[test]
    fn a_reordering_is_two_bands_that_cross() {
        // Copy plays original frames 151..300 first, then 1..150.
        let (mut copy, mut r) = one_shot(150, 150 * 33_333, 0);
        let (second, r2) = one_shot(150, 0, 150 * 33_333);
        copy.extend(second);
        r.verdicts.extend(r2.verdicts);
        r.segments.extend(r2.segments);
        let o = build(&copy, &originals(300), &r, &[], &[]);
        assert_eq!(o.bands.len(), 2);
        // Sorted by copy time: the first band comes from LATER in the original.
        assert!(o.bands[0].o0 > o.bands[1].o0, "{:?}", o.bands);
        assert!(o.original_absent.is_empty());
        // And the two ribbons carry different hues, or the crossing is
        // unreadable — the exact complaint on the first mock.
        let s = svg(&o);
        let hues: Vec<&str> = s
            .match_indices("class=\"ov-band\"")
            .map(|(i, _)| &s[i..i + 200])
            .collect();
        assert_eq!(hues.len(), 2);
        assert_ne!(hues[0], hues[1]);
    }

    #[test]
    fn a_single_frame_keeps_three_pixels() {
        let (copy, r) = one_shot(3000, 0, 0);
        let located = vec![Located {
            copy_index: 1500,
            copy_t_us: 1500 * 33_333,
            original_t_us: 1500 * 33_333,
            counter: 1501,
            difference: crate::imagediff::FrameDifference {
                state: DifferenceState::LocalizedDifference,
                regions: vec![],
                baseline: 30.0,
                spread: 2.0,
                inconclusive_because: None,
            },
        }];
        let o = build(&copy, &originals(3000), &r, &located, &[]);
        assert_eq!(o.copy_runs.len(), 3);
        assert_eq!(o.copy_runs[1].frames, 1);
        assert_eq!(o.copy_runs[1].state, FrameState::Differs);
        let s = svg(&o);
        // At this scale one frame is under 0.2 px; an event is drawn at 8,
        // and named beside the column so nobody has to find it by hovering.
        assert!(s.contains(r##"height="8.0" fill="#e08a2e""##), "{s}");
        assert!(s.contains(r##"class="ov-leader""##), "{s}");
        assert!(s.contains("1 frame differs</text>"), "{s}");
        // The count is not exaggerated with the height.
        assert!(s.contains("1 frame(s), differs"), "{s}");
    }

    #[test]
    fn foreign_frames_are_their_own_colour() {
        let (mut copy, mut r) = one_shot(30, 0, 0);
        for c in copy.iter_mut().take(10) {
            c.tag = Some(0x66AF);
        }
        for v in r.verdicts.iter_mut().take(10) {
            *v = FrameVerdict::Foreign;
        }
        let o = build(&copy, &originals(30), &r, &[], &[]);
        assert_eq!(o.copy_runs[0].state, FrameState::Foreign);
        assert!(svg(&o).contains("from another recording"));
    }

    #[test]
    fn the_figure_carries_what_a_click_needs() {
        let (copy, r) = one_shot(30, 0, 0);
        let s = svg(&build(&copy, &originals(30), &r, &[], &[]));
        for attr in [
            "data-top",
            "data-scale",
            "data-xo",
            "data-xc",
            "data-dur-o",
            "data-dur-c",
        ] {
            assert!(s.contains(attr), "{attr} missing");
        }
        assert!(s.contains("id=\"ov-ph-o\"") && s.contains("id=\"ov-ph-c\""));
    }
}
