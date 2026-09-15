//! The edit report as a page a reader can act on, with the original and the
//! supplied file side by side.
//!
//! Everything in this page that names a moment is a control: clicking it puts
//! both players on that moment — the original on the left, the supplied file
//! on the right — so a reader judges the picture with their own eyes instead
//! of taking a number's word for it. That is the point of the whole document.
//! A line that says a frame differs and cannot be looked at is an assertion;
//! the same line with the two frames on screen is evidence.
//!
//! The page states what was established and lets the reader see it. It reaches
//! no verdict, gives the recording no score, and says "no correspondence was
//! established" where it has nothing — never "falsification".

use crate::declared::{DeclaredCorrespondence, DeclaredSegment, FrameNote, JoinKind, NoteReason};
use crate::imagediff::{self, DiffSettings, DifferenceState, Located, OutOfPlace};

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// `m:ss.cc` — short enough to read in a table, precise enough to seek.
pub fn clock(us: i64) -> String {
    let t = us.max(0);
    format!("{}:{:05.2}", t / 60_000_000, (t % 60_000_000) as f64 / 1e6)
}

/// A span in seconds, for prose rather than for seeking.
fn duration(us: i64) -> String {
    format!("{:.2}s", (us.abs() as f64) / 1e6)
}

fn secs(us: i64) -> String {
    format!("{:.3}", (us.max(0) as f64) / 1e6)
}

/// A moment, as a control that seeks both players.
///
/// `original_us` may be absent: a frame that declares nothing has no known
/// place in the original, and the control then moves only the right-hand
/// player rather than pretending to a correspondence it does not have.
fn at(copy_us: i64, original_us: Option<i64>, label: &str) -> String {
    match original_us {
        Some(o) => format!(
            r#"<button class="at" data-copy="{}" data-orig="{}">{}</button>"#,
            secs(copy_us),
            secs(o),
            esc(label)
        ),
        None => format!(
            r#"<button class="at" data-copy="{}">{}</button>"#,
            secs(copy_us),
            esc(label)
        ),
    }
}

/// What the page needs beyond the correspondence itself.
pub struct PageInputs<'a> {
    /// Short id of the recording the bundle is for.
    pub short_id: &'a str,
    /// Verdict line from the bundle's own verifier, quoted, not re-derived.
    /// Ignored when `chain_checked` is false.
    pub chain_verdict: &'a str,
    /// Whether a verifier was run at all.
    ///
    /// The browser build never runs one — there is no Python in a browser and
    /// this project does not re-implement the check — so it says so in its own
    /// words rather than passing a sentence through. "not established here"
    /// was that sentence, and it told a reader neither what had not been done
    /// nor what to do about it.
    pub chain_checked: bool,
    /// Whether that verifier passed. The comparison is only worth reading
    /// under a chain that holds, so the page says so before anything else.
    pub chain_passed: bool,
    /// Path or URL the left-hand player loads: the original from the bundle.
    pub original_src: &'a str,
    /// Path or URL the right-hand player loads: the file being examined.
    pub copy_src: &'a str,
    pub original_label: &'a str,
    pub copy_label: &'a str,
    /// Frames read per second of copy, so the reader knows the resolution of
    /// every statement below.
    pub frames_read: usize,
    pub seconds_examined: f64,
    /// Frames whose picture was compared region by region. Empty when that
    /// pass was not run, and the report then says so rather than implying
    /// nothing was found.
    pub located: &'a [Located],
    /// The settings that comparison ran with, printed so a reader knows what
    /// dial position produced what they are looking at.
    pub diff: DiffSettings,
    /// Whether the localised comparison ran at all.
    pub located_ran: bool,
    /// Frames whose difference from the original is unusual for their own
    /// shot. The only signal here that sees a change spread evenly over a
    /// whole frame; every other one reads that as recompression.
    pub out_of_place: &'a [OutOfPlace],
}

/// The whole thing as a standalone document, for the native binary.
pub fn render(r: &DeclaredCorrespondence, inputs: &PageInputs) -> String {
    let mut h = String::with_capacity(20_000);
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    h.push_str(&format!(
        "<title>Edit report — {}</title>\n",
        esc(inputs.short_id)
    ));
    h.push_str(STYLE);
    h.push_str("</head><body>\n");
    h.push_str(&fragment(r, inputs));
    h.push_str("<script>");
    h.push_str(SCRIPT);
    h.push_str("</script>\n</body></html>\n");
    h
}

/// The report's markup on its own, for a host page that already has a head.
///
/// The browser build injects this into a page it controls, so the style and
/// the behaviour are handed over separately (`STYLE`, `SCRIPT`) rather than
/// baked in: a `<script>` arriving through `innerHTML` never runs, and a
/// second copy of either would be a second thing to keep true.
pub fn fragment(r: &DeclaredCorrespondence, inputs: &PageInputs) -> String {
    let mut h = String::with_capacity(16_000);

    h.push_str(&format!(
        "<h1>Edit report — {}</h1>\n",
        esc(inputs.short_id)
    ));
    h.push_str(
        "<p class=\"lede\">What follows describes how a supplied file stands against one \
         Forsheur recording. It reaches no verdict about the file, and gives it no score. \
         Every moment named below is a control: click it to put both players on it.</p>\n",
    );

    // ── The chain, first and separately ──────────────────────────────────
    h.push_str("<section><h2>1 · This recording</h2>\n");
    if inputs.chain_checked {
        h.push_str(&format!(
            "<p class=\"{}\">{}</p>\n",
            if inputs.chain_passed { "ok" } else { "bad" },
            esc(inputs.chain_verdict)
        ));
    } else {
        h.push_str(
            "<p><strong>The cryptographic chain was not checked here.</strong> This page \
             compares pictures and nothing else. What establishes where a recording came \
             from and when — the device signature, the notary chain, the timestamp anchors \
             — is checked by the verifier shipped inside the bundle, which this project \
             deliberately never re-implements: one implementation of the thing that must be \
             right, not two.</p>\n\
             <p>Unpack the bundle and run it, then read its verdict beside this report:</p>\n\
             <pre><code>python3 verify_bundle.py .</code></pre>\n",
        );
    }
    if inputs.chain_checked && !inputs.chain_passed {
        h.push_str(
            "<p>The comparison below is not shown: there is no established original to \
             compare against.</p>\n</section>\n",
        );
        return h;
    }
    h.push_str(&signature_block(r));
    h.push_str("</section>\n");

    // ── The players ──────────────────────────────────────────────────────
    //
    // The row is its own element. The link bar used to be a third flex item
    // beside the two figures, so the row re-distributed every time the
    // "held still" notice appeared — which is exactly at a cut, the moment a
    // reader is looking hardest. The picture jumped from 242 to 306 px.
    h.push_str("<section class=\"players\">\n<div class=\"playerrow\">\n");
    h.push_str(&format!(
        "<figure><figcaption>Original — {}</figcaption>\
         <video id=\"vo\" controls preload=\"metadata\" src=\"{}\"></video>\
         <p class=\"fcount\"><span id=\"fo\">—</span> \
         <label class=\"repick\">Load a file<input type=\"file\" accept=\"video/*\" data-for=\"vo\"></label></p>\
         </figure>\n",
        esc(inputs.original_label),
        esc(inputs.original_src)
    ));
    h.push_str(&format!(
        "<figure><figcaption>Supplied file — {}</figcaption>\
         <video id=\"vc\" controls preload=\"metadata\" src=\"{}\"></video>\
         <p class=\"fcount\"><span id=\"fc\">—</span> \
         <label class=\"repick\">Load a file<input type=\"file\" accept=\"video/*\" data-for=\"vc\"></label></p>\
         </figure>\n",
        esc(inputs.copy_label),
        esc(inputs.copy_src)
    ));
    h.push_str(
        "</div>\n<p class=\"linkbar\"><label><input type=\"checkbox\" id=\"link\" checked> \
         Keep the two players together</label> \
         <span class=\"step\"><button type=\"button\" id=\"prevf\" title=\"previous frame\">\
         ◀ frame</button><button type=\"button\" id=\"nextf\" title=\"next frame\">frame ▶\
         </button></span> \
         <span id=\"linkstate\" class=\"note\"></span></p>\n",
    );
    h.push_str("</section>\n");

    // ── Shots ────────────────────────────────────────────────────────────
    h.push_str("<section><h2>2 · Shots that correspond to the original</h2>\n");
    if r.segments.is_empty() {
        h.push_str(
            "<p>No correspondence was established anywhere in this file. That is an absence \
             of a reading, not a finding about the file: a recompression heavy enough to \
             destroy the burned-in strip, a crop that removed it, or a different recording \
             altogether all read the same way here.</p>\n",
        );
    } else {
        h.push_str(
            "<table><thead><tr><th>Shot</th><th>In the supplied file</th>\
             <th>In the original</th><th>Frames</th><th>Conforming</th>\
             <th>Differ</th><th>Inconclusive</th><th>Worst confirmed</th></tr></thead><tbody>\n",
        );
        for (i, s) in r.segments.iter().enumerate() {
            h.push_str(&shot_row(i + 1, s, r.confirm_distance));
        }
        h.push_str("</tbody></table>\n");
        h.push_str(
            "<p class=\"note\">“Worst confirmed” is the largest fingerprint distance among \
             the frames that did correspond, out of 63 bits. It is shown instead of an \
             average because an average of five hundred frames hides the one that matters.</p>\n",
        );
    }
    h.push_str("</section>\n");

    // ── Cuts ─────────────────────────────────────────────────────────────
    h.push_str("<section><h2>3 · Where the supplied file leaves the original's order</h2>\n");
    if r.cuts.is_empty() {
        h.push_str(if r.segments.len() == 1 {
            "<p>Nowhere. One shot, running from end to end in the original's own order.</p>\n"
        } else {
            "<p>No junction was established.</p>\n"
        });
    } else {
        h.push_str("<ul class=\"cuts\">\n");
        for c in &r.cuts {
            let skipped = c.counters_skipped();
            let body = match c.kind() {
                JoinKind::Backwards => format!(
                    "the file goes back: frame {} is followed by frame {}, \
                     {} frames earlier in the original",
                    c.counter_before,
                    c.counter_after,
                    skipped.unsigned_abs()
                ),
                JoinKind::Removal => format!(
                    "frame {} is followed by frame {} — {} frames of the original are \
                     absent, {} of it",
                    c.counter_before,
                    c.counter_after,
                    skipped.max(0),
                    duration(-c.inserted_us())
                ),
                JoinKind::Insertion => format!(
                    "frame {} is followed by frame {}, and {} of the file sits between \
                     them that the original does not account for",
                    c.counter_before,
                    c.counter_after,
                    duration(c.inserted_us())
                ),
            };
            h.push_str(&format!(
                "<li>{} — {}<br><span class=\"sub\">the original runs on from {} \
                 and resumes at {}</span></li>\n",
                at(
                    c.copy_at_us,
                    Some(c.original_left_us),
                    &format!("at {}", clock(c.copy_at_us))
                ),
                esc(&body),
                at(
                    c.copy_at_us,
                    Some(c.original_left_us),
                    &clock(c.original_left_us)
                ),
                at(
                    c.copy_at_us,
                    Some(c.original_resumed_us),
                    &clock(c.original_resumed_us)
                ),
            ));
        }
        h.push_str("</ul>\n");
        h.push_str(
            "<p class=\"note\">A junction is established by the burned-in frame numbers \
             alone, without comparing a single picture. Frames lost to a re-encode do not \
             appear here: they leave the numbers drifting by one at a time while the elapsed \
             time keeps matching the original's own, and that is continuity, not a cut.</p>\n",
        );
    }
    h.push_str("</section>\n");

    // ── Stretches that correspond to nothing ─────────────────────────────
    //
    // The most important thing on the page when it is not empty, and it used
    // to be missing: inserted material sits BETWEEN two shots, so a report
    // that only walked the inside of each shot showed a second of foreign
    // footage as nothing at all.
    h.push_str("<section><h2>4 · Stretches that correspond to nothing in the original</h2>\n");
    if r.unconfirmed.is_empty() {
        h.push_str("<p>None. Every part of the file was placed in the original.</p>\n");
    } else {
        h.push_str(
            "<p>Nothing in the original was found for these. That has innocent readings — a \
             title card, a logo, a passage recompressed past the point where the strip \
             survives — and it is also what inserted material looks like. The two players \
             are the way to tell.</p>\n<ul class=\"cuts\">\n",
        );
        for u in &r.unconfirmed {
            h.push_str(&format!(
                "<li>{} → {} ({}, {} frames) — {}</li>\n",
                at(u.copy_start_us, None, &clock(u.copy_start_us)),
                at(u.copy_end_us, None, &clock(u.copy_end_us)),
                esc(&duration(u.copy_end_us - u.copy_start_us)),
                u.frames_examined,
                esc(&match (u.frames_declaring, u.frames_contradicted) {
                    (0, _) => "no frame here carries a readable strip, so no question was \
                               put to the original at all. Whether this is inserted \
                               material or simply unreadable is not settled here — \
                               section 3 says whether the join around it accounts for the \
                               time"
                        .to_string(),
                    (d, 0) =>
                        format!("{d} frame(s) carry a frame number the original does not have"),
                    (d, c) => format!(
                        "{d} frame(s) carry a frame number; {c} of them name a moment of \
                         the original and do not look like it"
                    ),
                }),
            ));
        }
        h.push_str("</ul>\n");
    }
    h.push_str("</section>\n");

    // ── Frames that differ ───────────────────────────────────────────────
    h.push_str("<section><h2>5 · Frames whose picture differs from the original's</h2>\n");
    let total_differing: usize = r.segments.iter().map(|s| s.differing.len()).sum();
    if total_differing == 0 {
        h.push_str(
            "<p>None, among the frames that could be judged. See section 8 for the frames \
             that could not.</p>\n",
        );
    } else {
        h.push_str(
            "<p>Each of these declares a frame of the original and does not look like it. \
             A single one is worth looking at: click it.</p>\n<ul class=\"frames\">\n",
        );
        for (i, s) in r.segments.iter().enumerate() {
            for n in &s.differing {
                h.push_str(&frame_line(i + 1, n));
            }
        }
        h.push_str("</ul>\n");
    }
    h.push_str("</section>\n");

    // ── Where inside a frame the picture differs ─────────────────────────
    h.push_str(
        "<section><h2>6 · Where the picture differs inside frames that otherwise \
         correspond</h2>\n",
    );
    h.push_str(&located_section(inputs));
    h.push_str("</section>\n");

    // ── Frames that do not sit with their neighbours ─────────────────────
    h.push_str("<section><h2>7 · Frames that do not sit with their neighbours</h2>\n");
    h.push_str(&out_of_place_section(inputs));
    h.push_str("</section>\n");

    // ── Inconclusive ─────────────────────────────────────────────────────
    let total_inconclusive: usize = r.segments.iter().map(|s| s.inconclusive.len()).sum();
    h.push_str("<section><h2>8 · Frames nothing could be established about</h2>\n");
    h.push_str(&format!(
        "<p>{} frame(s). These are not evidence of anything, in either direction. \
         A frame is here because its strip could not be read — too compressed, too \
         blurred, cropped away, or overwritten — so no question was put to the original \
         at all.</p>\n",
        total_inconclusive
    ));
    if total_inconclusive > 0 && total_inconclusive <= 400 {
        h.push_str("<ul class=\"frames dim\">\n");
        for (i, s) in r.segments.iter().enumerate() {
            for n in &s.inconclusive {
                h.push_str(&frame_line(i + 1, n));
            }
        }
        h.push_str("</ul>\n");
    } else if total_inconclusive > 400 {
        h.push_str(
            "<p class=\"note\">Too many to list one by one; the per-shot counts in \
             section 2 carry them.</p>\n",
        );
    }
    h.push_str("</section>\n");

    // ── What this cannot say ─────────────────────────────────────────────
    h.push_str(&limits(r, inputs));

    h.push_str(&shot_map(r));
    h
}

/// The shots, as the correspondence the linked players follow.
///
/// The offset between the two files is NOT constant — that is the whole point
/// of a cut. On the removal case it is 0 before 0:07.97 and +6 s after, so a
/// pair of players held at a fixed delta would be showing two different
/// moments for most of the file. Each shot carries its own mapping and the
/// page interpolates inside it.
fn shot_map(r: &DeclaredCorrespondence) -> String {
    let mut out = String::from("<script id=\"shots\" type=\"application/json\">[");
    for (i, s) in r.segments.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // The counters at each end come too: they are what lets the page put
        // a frame NUMBER under each player. A reader who cannot tell frame
        // 301 from 302 on screen cannot confirm they are looking at the frame
        // the report named, and "close but not on it" is then an impression
        // nobody can settle.
        out.push_str(&format!(
            "{{\"c0\":{},\"c1\":{},\"o0\":{},\"o1\":{},\"k0\":{},\"k1\":{}}}",
            secs(s.copy_start_us),
            secs(s.copy_end_us),
            secs(s.original_start_us),
            secs(s.original_end_us),
            s.counter_start,
            s.counter_end
        ));
    }
    out.push_str("]</script>\n");
    out
}

/// Section 6: the three states of the picture comparison, and where.
///
/// Counted, then listed. The counts matter as much as the list: a run where
/// almost everything came back "consistent with recompression" and two frames
/// came back located reads very differently from one where half the frames
/// were inconclusive, and a reader who only sees the located ones cannot tell
/// those apart.
fn located_section(inputs: &PageInputs) -> String {
    if !inputs.located_ran {
        return "<p>Not performed in this run.</p>\n".to_string();
    }
    if inputs.located.is_empty() {
        return "<p>No frame was eligible: this comparison runs only over frames whose \
                picture already corresponds, and there were none.</p>\n"
            .to_string();
    }

    let mut consistent = 0usize;
    let mut inconclusive = 0usize;
    let mut located: Vec<&Located> = Vec::new();
    for l in inputs.located {
        match l.difference.state {
            DifferenceState::ConsistentWithRecompression => consistent += 1,
            DifferenceState::Inconclusive => inconclusive += 1,
            DifferenceState::LocalizedDifference => located.push(l),
        }
    }

    let mut out = format!(
        "<p>{} frame(s) compared region by region, on a {}×{} grid at sensitivity {:.1}. \
         <strong>{}</strong> differ evenly across the frame, the way a recompression does. \
         <strong>{}</strong> could not be judged. <strong>{}</strong> carry a difference \
         confined to one part of the picture.</p>\n",
        inputs.located.len(),
        inputs.diff.cols,
        inputs.diff.rows,
        inputs.diff.sensitivity,
        consistent,
        inconclusive,
        located.len(),
    );

    if located.is_empty() {
        out.push_str(
            "<p>Nothing is located, and that is a statement about the measurement, not a \
             clearance: a change smaller than a tile, or one that moves the whole frame \
             evenly, would not appear here.</p>\n",
        );
    } else {
        // Grouped over time, because a per-frame count cannot tell a patch
        // from encoder blocking and the two look identical in one frame.
        let all = imagediff::tracks(inputs.located, 1);
        let (held, blips): (Vec<_>, Vec<_>) = all.iter().partition(|t| t.frames > 1);

        if !held.is_empty() {
            out.push_str(
                "<p><strong>Regions that stay in one place.</strong> A retouch sits still \
                 for as long as it is there; a codec's blocking moves every frame. \
                 Coordinates are fractions of the frame, so they hold whatever size the \
                 file was rescaled to. Click and look.</p>\n<ul class=\"frames\">\n",
            );
            for t in held.iter().take(100) {
                out.push_str(&format!(
                    "<li>{} → {} · frames {}–{} · {} frame(s), {} · \
                     {:.0}% × {:.0}% of the picture at ({:.0}%, {:.0}%) · peak {:.1} \
                     deviations</li>\n",
                    at(t.first_copy_t_us, None, &clock(t.first_copy_t_us)),
                    at(t.last_copy_t_us, None, &clock(t.last_copy_t_us)),
                    t.first_counter,
                    t.last_counter,
                    t.frames,
                    esc(&duration(t.duration_us())),
                    t.region.width * 100.0,
                    t.region.height * 100.0,
                    t.region.x * 100.0,
                    t.region.y * 100.0,
                    t.region.deviations,
                ));
            }
            out.push_str("</ul>\n");
        }

        if !blips.is_empty() {
            out.push_str(&format!(
                "<p><strong>{} region(s) seen in a single frame and nowhere else.</strong> \
                 Listed because nothing here is hidden, and separated because this is what \
                 a starved encoder produces: at a low bit rate some blocks are given far \
                 fewer bits than their neighbours, and they stand out against the frame's \
                 own noise exactly the way an edit does. A single one of these is not \
                 evidence of anything on its own.</p>\n<ul class=\"frames dim\">\n",
                blips.len()
            ));
            for t in blips.iter().take(60) {
                out.push_str(&format!(
                    "<li>{} · frame {} · {:.0}% × {:.0}% at ({:.0}%, {:.0}%) · {:.1} \
                     deviations</li>\n",
                    at(t.first_copy_t_us, None, &clock(t.first_copy_t_us)),
                    t.first_counter,
                    t.region.width * 100.0,
                    t.region.height * 100.0,
                    t.region.x * 100.0,
                    t.region.y * 100.0,
                    t.region.deviations,
                ));
            }
            out.push_str("</ul>\n");
            if blips.len() > 60 {
                out.push_str("<p class=\"note\">Only the first 60 are listed.</p>\n");
            }
        }
    }

    out.push_str(&format!(
        "<p class=\"note\">Nothing here is compared against a fixed number of grey levels. \
         Each frame is judged against its own spread, so a heavily recompressed frame raises \
         its own bar and there is no published constant for anyone to tune against. The top \
         {:.0}% of the frame is left out: that is the burn-in band, whose content is checked \
         elsewhere and by checksum, and whose hard edges are the noisiest thing in the picture \
         under recompression.</p>\n",
        inputs.diff.skip_top_fraction * 100.0
    ));
    out
}

/// The recording signature, said out loud in all three cases.
///
/// This is the cheapest decisive check in the tool: a frame carrying somebody
/// else's two bytes settles a swapped recording without comparing a single
/// picture. It used to be reported only by implication — a match and a
/// no-match read almost the same, and a reader could not see that a check had
/// happened at all. A check nobody notices succeeding is a check nobody will
/// think about when it fails.
/// Section 7: a frame that departs from the original far more than the frames
/// around it do.
///
/// Everything else here compares a frame with the original's frame of the same
/// number and asks whether the difference is spread out or concentrated. A
/// frame that was blurred, graded, re-rendered or swapped in from another
/// source differs EVENLY, so it reads as recompression and vanishes into the
/// conforming count. This asks the question none of the others do: is that
/// much difference normal for this film?
fn out_of_place_section(inputs: &PageInputs) -> String {
    if !inputs.located_ran {
        return "<p>Not performed in this run.</p>\n".to_string();
    }
    let runs = imagediff::out_of_place_runs(inputs.out_of_place);
    let mut out = String::new();

    if runs.is_empty() {
        out.push_str(
            "<p>None. Every frame departs from the original by about as much as the frames \
             around it do.</p>\n",
        );
    } else {
        out.push_str(
            "<p>These depart from the original far more than their neighbours in the same \
             shot. That is a measurement, not a conclusion — go and look at them, the \
             players are already lined up. <strong>The shape matters as much as the \
             count</strong>: an encoder cannot degrade one frame and spare its neighbours, \
             its rate control settles over a run and usually at the start of a file, while \
             a frame that was blurred, graded or swapped in is a single-frame event.</p>\n\
             <ul class=\"frames\">\n",
        );
        for t in runs.iter().take(60) {
            let what = if t.is_single_frame() {
                format!(
                    "<strong>one frame, alone</strong> · frame {}",
                    t.first_counter
                )
            } else {
                format!(
                    "<strong>{} frames over {}</strong> · frames {}–{}{}",
                    t.frames,
                    esc(&duration(t.duration_us())),
                    t.first_counter,
                    t.last_counter,
                    if t.at_file_start {
                        ", in the opening second of the file"
                    } else {
                        ""
                    }
                )
            };
            out.push_str(&format!(
                "<li>{} · {} · differs by {:.0} at its peak where the shot differs by {:.0} \
                 · {:.0}× the shot's own spread</li>\n",
                at(
                    t.first_copy_t_us,
                    Some(t.original_t_us),
                    &clock(t.first_copy_t_us)
                ),
                what,
                t.peak_baseline,
                t.shot_baseline,
                t.peak_deviations
            ));
        }
        out.push_str("</ul>\n");
        if runs.len() > 60 {
            out.push_str("<p class=\"note\">Only the first 60 are listed.</p>\n");
        }
    }

    out.push_str(&format!(
        "<p class=\"note\">Judged against each shot's own spread at sensitivity {:.0}, so a \
         heavily recompressed film raises its own bar and there is no published constant to \
         tune against. Nothing is dropped by the grouping: a blurred passage is a run too, \
         and a real finding. Two things this cannot see. A change applied to the WHOLE film \
         — if every frame is blurred, every frame's neighbours are blurred too and nothing \
         stands out. And a shot filmed with too little texture to measure: the frames of a \
         wall, a sky or a table are set aside before this runs, and a change inside them is \
         set aside with them.</p>\n",
        inputs.diff.frame_sensitivity
    ));
    out
}

fn signature_block(r: &DeclaredCorrespondence) -> String {
    let total: usize = r.tags_seen.iter().map(|(_, n)| *n).sum();
    match (r.tag_expected, r.tags_seen.as_slice()) {
        // Nothing to compare against: no id was given.
        (None, []) => {
            "<p><strong>No recording signature was read, and none was expected.</strong> \
             No recording id was given, so nothing was compared. The strip is destroyed by a \
             heavy recompression and removed by a crop, so its absence on its own says \
             nothing either way.</p>\n"
                .to_string()
        }
        (None, seen) => {
            let list: Vec<String> = seen
                .iter()
                .map(|(t, n)| format!("0x{t:04X} on {n} frame(s)"))
                .collect();
            format!(
                "<p class=\"warn\"><strong>These signatures were read, and compared to \
                 nothing:</strong> {}. No recording id was given, so this report cannot say \
                 whether they are the right ones. Supply the id — the tail of the QR URL — \
                 and this becomes a one-frame check.</p>\n",
                esc(&list.join(", "))
            )
        }
        // An id was given and no strip could be read anywhere.
        (Some(w), []) => format!(
            "<p class=\"warn\"><strong>Expected signature 0x{w:04X}, and no signature could \
             be read at all.</strong> Nothing follows from that on its own: the strip does \
             not survive a heavy recompression and a crop removes it. It does mean this \
             check established nothing, and the correspondence below rests entirely on the \
             pictures.</p>\n"
        ),
        (Some(w), seen) => {
            let mine = seen.iter().find(|(t, _)| *t == w).map_or(0, |(_, n)| *n);
            let others: Vec<String> = seen
                .iter()
                .filter(|(t, _)| *t != w)
                .map(|(t, n)| format!("0x{t:04X} on {n} frame(s)"))
                .collect();
            if others.is_empty() {
                format!(
                    "<p class=\"ok\"><strong>Signature 0x{w:04X}: carried by all {mine} frame(s) \
                     that could be read, and no other signature appears.</strong> Every frame \
                     whose strip survived says it belongs to this recording. It is two bytes and \
                     anyone can draw them, so this does not prove the frames are genuine — but \
                     a frame carrying somebody else's would have settled the opposite here, in \
                     one frame.</p>\n"
                )
            } else if mine == 0 {
                // Not a mixture — a different recording altogether. Saying
                // "frames from more than one recording" here would be wrong,
                // and this is the swapped-recording case, the one the check
                // exists for.
                format!(
                    "<p class=\"bad\"><strong>This file is not the recording this bundle is \
                     for.</strong> The bundle's signature is 0x{w:04X} and not one of the \
                     {total} frame(s) that could be read carries it. They carry {} instead. \
                     Everything below is measured against an original this file does not \
                     claim to come from.</p>\n",
                    esc(&others.join(", "))
                )
            } else {
                format!(
                    "<p class=\"bad\"><strong>This file mixes more than one recording.</strong> \
                     The bundle's signature is 0x{w:04X}, carried by {mine} of the {total} \
                     frame(s) that could be read. The rest carry {}, which is another \
                     recording's.</p>\n",
                    esc(&others.join(", "))
                )
            }
        }
    }
}

fn shot_row(n: usize, s: &DeclaredSegment, confirm: u32) -> String {
    format!(
        "<tr class=\"{}\"><td>{}</td><td>{} → {}</td><td>{} → {}</td>\
         <td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/63</td></tr>\n",
        if s.is_clean() { "clean" } else { "" },
        n,
        at(
            s.copy_start_us,
            Some(s.original_start_us),
            &clock(s.copy_start_us)
        ),
        at(
            s.copy_end_us,
            Some(s.original_end_us),
            &clock(s.copy_end_us)
        ),
        at(
            s.copy_start_us,
            Some(s.original_start_us),
            &clock(s.original_start_us)
        ),
        at(
            s.copy_end_us,
            Some(s.original_end_us),
            &clock(s.original_end_us)
        ),
        s.frames_examined,
        s.frames_confirmed,
        s.differing.len(),
        s.inconclusive.len(),
        s.worst_confirmed_distance
            .min(confirm.max(s.worst_confirmed_distance)),
    )
}

fn frame_line(shot: usize, n: &FrameNote) -> String {
    let what = match n.reason {
        NoteReason::PictureDiffers => match n.distance {
            Some(d) => format!("differs by {d} of 63 bits"),
            None => "differs".to_string(),
        },
        NoteReason::NotInOriginal => {
            "declares a frame number the original does not have".to_string()
        }
        NoteReason::NoDeclaration => "no readable strip".to_string(),
        NoteReason::TooFarToJudge => match n.distance {
            Some(d) => format!(
                "{d} of 63 bits apart — too far to confirm, not far enough to \
                                call it a different picture"
            ),
            None => "the fingerprint did not settle it".to_string(),
        },
    };
    let which = match n.counter {
        Some(c) => format!("frame {c}"),
        None => "no frame number".to_string(),
    };
    format!(
        "<li>shot {} · {} · {} · {}</li>\n",
        shot,
        at(n.copy_t_us, n.original_t_us, &clock(n.copy_t_us)),
        esc(&which),
        esc(&what)
    )
}

fn limits(r: &DeclaredCorrespondence, inputs: &PageInputs) -> String {
    let rate = if inputs.seconds_examined > 0.0 {
        inputs.frames_read as f64 / inputs.seconds_examined
    } else {
        0.0
    };
    format!(
        "<section><h2>9 · What this report cannot say</h2>\n<ul class=\"limits\">\n\
         <li>{} frames were read, {:.1} per second of the supplied file. Nothing is claimed \
         about a moment that was not read.</li>\n\
         <li>The picture comparison uses a 63-bit fingerprint of each frame's coarse \
         light-and-dark structure. It moves when a shot is replaced, a face substituted or \
         an object added. It will not move for a retouch of a few dozen pixels.</li>\n\
         <li>A burned-in frame number is a claim drawn into the picture, not a signature. \
         It is checked by fetching that frame of the original and comparing. A forged number \
         can make this report establish nothing; it cannot make it establish something \
         false.</li>\n\
         <li>Nothing here is a verdict on the supplied file, and nothing here is a score. \
         Sections 4 and 5 are separate on purpose: an unreadable frame is not a suspicious \
         one.</li>\n\
         <li>{} frame(s) carried no readable strip at all.</li>\n\
         </ul></section>\n",
        inputs.frames_read, rate, r.frames_without_band
    )
}

/// The report's stylesheet, `<style>` tags included.
pub const STYLE: &str = r#"<style>
:root { color-scheme: light dark; --line:#d8d8d8; --dim:#666; --bad:#8a1c1c; --ok:#14532d; --warn:#7a4a00; }
@media (prefers-color-scheme: dark) { :root { --line:#333; --dim:#9a9a9a; --bad:#ff9a9a; --ok:#9ae6b4; --warn:#f0c674; } }
body { margin:0 auto; padding:24px 16px 64px; max-width:1100px;
       font:15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif; }
h1 { font-size:22px; margin:0 0 4px; }
h2 { font-size:16px; margin:32px 0 8px; padding-bottom:4px; border-bottom:1px solid var(--line); }
.lede { color:var(--dim); max-width:70ch; }
.ok { color:var(--ok); } .bad { color:var(--bad); font-weight:600; }
/* Amber, not red: "nothing was compared" is not "something is wrong", and
   colouring the two the same would teach a reader to ignore both. */
.warn { color:var(--warn); }
pre { background:rgba(127,127,127,.12); padding:8px 10px; border-radius:4px;
      overflow-x:auto; font-size:13px; }
.note, .sub { color:var(--dim); font-size:13px; }
.players { position:sticky; top:0; background:Canvas; padding:8px 0; z-index:5;
           border-bottom:1px solid var(--line); }
.playerrow { display:flex; gap:12px; flex-wrap:wrap; }
.players figure { flex:1 1 300px; margin:0; min-width:0; }
.players figcaption { font-size:12px; color:var(--dim); margin-bottom:4px; }
.players video { width:100%; max-height:42vh; background:#000; }
/* Two lines' worth reserved whether the notice is showing or not: it appears
   exactly at a cut, and a bar that grows there would shove the picture down
   at the moment the reader is looking hardest. */
.linkbar { margin:6px 0 0; font-size:13px; display:flex; gap:10px;
           align-items:baseline; flex-wrap:wrap; min-height:2.6em; }
.linkbar label { white-space:nowrap; }
.players figure.adrift video { outline:2px solid var(--bad); outline-offset:-2px; }
.fcount { margin:4px 0 0; font-size:12px; color:var(--dim); display:flex; gap:12px;
          align-items:baseline; font-variant-numeric:tabular-nums; }
.fcount span { font-weight:600; color:inherit; }
.step button { font:inherit; font-size:12px; padding:2px 8px; cursor:pointer; }
.step button:first-child { border-radius:4px 0 0 4px; }
.step button:last-child { border-radius:0 4px 4px 0; margin-left:-1px; }
.repick { display:inline-block; font-size:12px; color:var(--dim); cursor:pointer; }
.repick input { display:none; }
table { border-collapse:collapse; width:100%; font-size:14px; }
th, td { text-align:left; padding:5px 8px; border-bottom:1px solid var(--line); white-space:nowrap; }
th { font-weight:600; font-size:12px; color:var(--dim); }
tr.clean td { opacity:.85; }
ul.frames, ul.cuts, ul.limits { padding-left:18px; }
ul.frames li, ul.cuts li { margin:3px 0; }
ul.dim li { color:var(--dim); }
ul.limits li { margin:6px 0; max-width:75ch; }
button.at { font:inherit; font-variant-numeric:tabular-nums; color:inherit; background:none;
            border:none; border-bottom:1px dotted currentColor; padding:0; cursor:pointer; }
button.at:hover { background:rgba(127,127,127,.18); }
code { font-family:ui-monospace,SFMono-Regular,Menlo,monospace; }
</style>
"#;

/// The report's behaviour, WITHOUT the `<script>` tags so a host page can
/// inject it as a real script element — markup handed to `innerHTML` never
/// executes the scripts inside it.
pub const SCRIPT: &str = r#"
// Callable again after the browser build injects a fresh report, hence a named
// function rather than a bare IIFE. Wiring the same pair twice would double
// every listener, so the elements are marked once.
window.editReportLink = function () {
  var vo = document.getElementById('vo');
  var vc = document.getElementById('vc');
  if (!vo || !vc || vc.dataset.wired === '1') return;
  vc.dataset.wired = '1';
  var link = document.getElementById('link');
  var state = document.getElementById('linkstate');
  var shots = [];
  try { shots = JSON.parse(document.getElementById('shots').textContent) || []; } catch (e) {}

  // Map a moment from one file to the other, THROUGH THE FRAME NUMBER.
  //
  // There used to be a second route: interpolate the time directly between a
  // shot's endpoints. It looked equivalent and was not. The frame readout and
  // the step buttons went through the counter, the follow went through time,
  // and nothing made the two agree — drag the original's thumb to frame 1 and
  // the copy landed on frame 3.
  //
  // Going through the counter makes agreement structural rather than likely:
  // the other player is asked for the frame the driver is on, by number, and
  // `timeOfCounter` aims at the middle of that frame's interval so no
  // rounding can put it on a neighbour. Outside every shot there is no
  // counter, hence no answer — which is correct, and is how inserted material
  // and removed material hold the other player still.

  // How long one frame lasts on a side. Derived from the shots rather than
  // assumed: a shot's span divided by the counters it covers IS the frame
  // period, and it stays right when the two files run at different rates.
  function periodOf(side) {
    var best = null;
    for (var i = 0; i < shots.length; i++) {
      var s = shots[i], n = s.k1 - s.k0;
      if (n > 0) {
        var p = (s[side + '1'] - s[side + '0']) / n;
        if (p > 0.0005 && (best === null || n > best.n)) best = { p: p, n: n };
      }
    }
    return best ? best.p : 1 / 30;
  }

  // The frame number at a moment. Floor, not round, and that is not a detail:
  // `timeOfCounter` aims at the middle of a frame, so rounding here would send
  // the middle of frame k back as k+1 and the two conversions would disagree
  // by half a frame. They did — stepping went 180, 181, 183, 184, 186.
  function counterAt(t, side) {
    var slack = periodOf(side);
    for (var i = 0; i < shots.length; i++) {
      var s = shots[i];
      var a0 = s[side + '0'], a1 = s[side + '1'];
      if (t >= a0 - slack && t <= a1 + slack) {
        var span = a1 - a0;
        if (span <= 0.001) return s.k0;
        var k = Math.floor(s.k0 + (s.k1 - s.k0) * ((t - a0) / span) + 1e-6);
        return Math.min(s.k1, Math.max(s.k0, k));
      }
    }
    return null;
  }

  // Where a frame number sits on a side, aimed at the middle of its interval.
  function timeOfCounter(k, side) {
    for (var i = 0; i < shots.length; i++) {
      var s = shots[i];
      if (k >= s.k0 && k <= s.k1) {
        var n = s.k1 - s.k0;
        var a0 = s[side + '0'], a1 = s[side + '1'];
        var t = n > 0 ? a0 + (a1 - a0) * ((k - s.k0) / n) : a0;
        return Math.max(0, t + periodOf(side) / 2);
      }
    }
    return null;
  }

  // The one mapping. Everything that moves a player goes through it.
  function otherTime(t, side) {
    var k = counterAt(t, side);
    return k === null ? null : timeOfCounter(k, side === 'c' ? 'o' : 'c');
  }

  // Both directions. Neither player is "the" driver: on screen they are two
  // identical players, and nothing tells a reader which one commands.
  var HELD = 'no counterpart here — the other player is held still';
  function linked() { return link && link.checked; }

  // ── Echo suppression ────────────────────────────────────────────────────
  //
  // Moving one player moves the other, which makes the other fire the very
  // events we listen for. Telling our own echo from the reader's hand cannot
  // be done with timing: the first version raised a flag and lowered it on the
  // next macrotask, but a `seeked` arrives long after that, so every
  // programmatic seek came back looking like a fresh one and the two players
  // volleyed. Four scrubs produced 1998 events.
  //
  // So the echo is identified by its VALUE, not by when it arrives: we record
  // what we asked for, and the matching event is consumed once.
  // A LIST of outstanding values, not one.
  //
  // A single slot was enough while every move came one at a time. Dragging a
  // thumb does not: the browser fires `timeupdate` throughout the drag and
  // `seeked` at the end, so two driven seeks can be in flight at once. The
  // second overwrote the first, the first echo consumed the slot, and the
  // second echo then arrived unclaimed — read as the reader moving that
  // player. The link reversed direction and dragged the other one to match a
  // position it had itself produced, landing a few frames out. Intermittently,
  // because it depends on which event wins the race.
  var echo = { o: { seek: [], play: [] }, c: { seek: [], play: [] } };
  function slot(v) { return v === vc ? echo.c : echo.o; }

  function driveTime(target, t) {
    // Arm nothing when the player is already there: no event will be fired,
    // and an armed value nobody claims swallows the reader's next move.
    if (Math.abs(target.currentTime - t) < 0.001) return;
    slot(target).seek.push(t);
    try { target.currentTime = t; } catch (e) {}
  }
  function drivePlay(target, playing) {
    if (target.paused === !playing) return;   // nothing will fire
    slot(target).play.push(playing);
    if (playing) { target.play().catch(function () {}); } else { target.pause(); }
  }
  function isEcho(v, kind, value) {
    var s = slot(v);
    if (kind === 'seek') {
      // Any outstanding value may be the one that just landed, and only that
      // one is consumed. Values older than it are dropped with it: the player
      // has moved past them, so nothing will ever claim them.
      for (var i = 0; i < s.seek.length; i++) {
        if (Math.abs(v.currentTime - s.seek[i]) < 0.08) {
          s.seek.splice(0, i + 1);
          return true;
        }
      }
      return false;
    }
    for (var j = 0; j < s.play.length; j++) {
      if (s.play[j] === value) {
        s.play.splice(0, j + 1);
        return true;
      }
    }
    return false;
  }

  // Which player the reader last touched. Only that one corrects the other
  // during playback: both fire `timeupdate` several times a second, and if
  // each corrected the other they would argue over every tenth of a second.
  var driver = null;

  function setAdrift(el, yes) {
    var fig = el && el.closest('figure');
    if (fig) fig.classList.toggle('adrift', !!yes);
    if (state) {
      var any = document.querySelector('.players figure.adrift');
      state.textContent = any ? HELD : '';
    }
  }

  function follow(v, force) {
    if (!linked()) return;
    var other = v === vc ? vo : vc;
    var t = otherTime(v.currentTime, v === vc ? 'c' : 'o');
    if (t === null) {
      if (!other.paused) drivePlay(other, false);
      setAdrift(other, true);
      return;
    }
    setAdrift(other, false);
    if (force || Math.abs(other.currentTime - t) > 0.15) {
      driveTime(other, t);
    }
  }

  function wire(v) {
    var other = v === vc ? vo : vc;

    v.addEventListener('seeked', function () {
      if (isEcho(v, 'seek')) return;
      driver = v;
      atCounter = null;   // the reader moved it themselves
      follow(v, true);
    });

    // Drift correction while both roll. Deliberately loose: seeking a playing
    // video to shave off a tenth of a second makes it stutter, and nobody
    // compares two frames mid-playback.
    v.addEventListener('timeupdate', function () {
      if (v !== driver) return;
      follow(v, false);
    });

    v.addEventListener('play', function () {
      if (isEcho(v, 'play', true)) return;
      driver = v;
      atCounter = null;
      if (!linked()) return;
      // Only if the other side has somewhere to be. Starting a player with no
      // counterpart would walk it away from the moment being compared.
      if (otherTime(v.currentTime, v === vc ? 'c' : 'o') !== null) {
        drivePlay(other, true);
      }
    });

    v.addEventListener('pause', function () {
      if (isEcho(v, 'play', false)) return;
      driver = v;
      if (!linked()) return;
      drivePlay(other, false);
      // Land exactly on pause. The moment you stop is the moment it has to be
      // exact, and this is the only place `force` is worth its cost.
      follow(v, true);
    });

    v.addEventListener('ratechange', function () {
      if (linked()) other.playbackRate = v.playbackRate;
    });
  }
  wire(vc);
  wire(vo);

  if (link) {
    link.addEventListener('change', function () {
      if (link.checked) {
        driver = vc;
        follow(vc, true);
      } else {
        setAdrift(vo, false);
        setAdrift(vc, false);
      }
    });
  }

  // ── Which frame am I looking at ─────────────────────────────────────────
  //
  // Two frames a thirtieth of a second apart look the same, so a reader who
  // clicks a report line has no way to confirm they landed on the frame it
  // named, and no way to check the neighbour. The readout answers the first,
  // the step buttons the second — both through `counterAt` above, the same
  // conversion the players themselves use, so what is displayed cannot
  // disagree with what is aligned.

  function showFrame(v, el) {
    if (!el) return;
    var side = v === vc ? 'c' : 'o';
    var k = counterAt(v.currentTime, side);
    el.textContent = k === null
      ? 'f=? · ' + v.currentTime.toFixed(2) + 's'
      : 'f=' + k + ' · ' + v.currentTime.toFixed(2) + 's';
  }
  function watchFrames(v, el) {
    if (v.requestVideoFrameCallback) {
      var tick = function () {
        showFrame(v, el);
        v.requestVideoFrameCallback(tick);
      };
      v.requestVideoFrameCallback(tick);
    }
    // `seeked` and `timeupdate` cover the browsers without rVFC, and the
    // paused case where no frame is painted.
    v.addEventListener('seeked', function () { showFrame(v, el); });
    v.addEventListener('timeupdate', function () { showFrame(v, el); });
    showFrame(v, el);
  }
  watchFrames(vo, document.getElementById('fo'));
  watchFrames(vc, document.getElementById('fc'));

  // One frame at a time, both together. Native controls have no such thing,
  // and stepping is what turns "I think it is that one" into "it is that one,
  // and here is the one before".
  //
  // Stepped by frame NUMBER, not by time. Adding a frame's duration to each
  // player's clock and re-projecting one onto the other looked equivalent and
  // was not: both the addition and the projection round, the errors add up,
  // and after a few steps the pair sat one or two frames apart. An integer
  // cannot drift — so the counter moves by one and each side is told where
  // that counter lives.
  // The frame the last step aimed at. Kept because re-deriving it from the
  // clock at every click compounds each conversion's rounding; an integer
  // carried forward cannot.
  var atCounter = null;

  function step(dir) {
    // Before the reader has touched either, step from the supplied file —
    // the one the report is about.
    var v = driver === vo ? vo : vc;
    var side = v === vc ? 'c' : 'o';
    drivePlay(vc, false);
    drivePlay(vo, false);

    var k = atCounter !== null ? atCounter : counterAt(v.currentTime, side);
    var tc = k === null ? null : timeOfCounter(k + dir, 'c');
    var to = k === null ? null : timeOfCounter(k + dir, 'o');
    if (tc === null || to === null) {
      // Outside any shot there is no counter to step: move the player the
      // reader is driving and let the other hold, as everywhere else.
      atCounter = null;
      driveTime(v, Math.max(0, v.currentTime + dir * periodOf(side)));
      setTimeout(function () { follow(v, true); }, 0);
      return;
    }
    atCounter = k + dir;
    driveTime(vc, Math.max(0, tc));
    driveTime(vo, Math.max(0, to));
    setAdrift(vo, false);
    setAdrift(vc, false);
  }
  var prevf = document.getElementById('prevf');
  var nextf = document.getElementById('nextf');
  if (prevf) prevf.addEventListener('click', function () { step(-1); });
  if (nextf) nextf.addEventListener('click', function () { step(1); });

  // Seek, then pause: the point is to compare two still frames, and a player
  // that keeps rolling has moved off the frame by the time you look at it.
  function seek(v, t) {
    if (!v || t === undefined || t === null || t === '') return;
    drivePlay(v, false);
    driveTime(v, parseFloat(t));
  }
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('button.at');
    if (!b) return;
    // Both are driven from here, so both echoes are registered — otherwise
    // each lands as a reader's seek and the pair starts volleying.
    seek(vc, b.dataset.copy);
    if (b.dataset.orig !== undefined) {
      seek(vo, b.dataset.orig);
      setAdrift(vo, false);
      setAdrift(vc, false);
    } else {
      drivePlay(vo, false);
      setAdrift(vo, true);
    }
    driver = vc;
    atCounter = null;
    (vo || vc).scrollIntoView({ block: 'start', behavior: 'smooth' });
  });

  // The report may travel without the videos beside it; let the reader point
  // each player at a local file rather than leaving a dead page.
  document.querySelectorAll('.repick input').forEach(function (i) {
    i.addEventListener('change', function () {
      var v = document.getElementById(i.dataset.for);
      if (v && i.files && i.files[0]) v.src = URL.createObjectURL(i.files[0]);
    });
  });

  // A file served without HTTP range support reports seekable = [0,0] and
  // every seek here is silently ignored. Say so once rather than let the
  // reader conclude the report is broken.
  vc.addEventListener('loadeddata', function () {
    if (vc.seekable.length && vc.seekable.end(0) === 0 && state) {
      state.textContent = 'this server does not answer range requests, so the '
        + 'players cannot seek — open the report over a server that does';
    }
  });
};
if (document.getElementById('vc')) { window.editReportLink(); }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declared::{DeclaredCut, FrameNote, NoteReason};

    fn seg() -> DeclaredSegment {
        DeclaredSegment {
            copy_start_us: 0,
            copy_end_us: 7_730_000,
            counter_start: 1,
            counter_end: 233,
            original_start_us: 0,
            original_end_us: 7_730_000,
            frames_examined: 233,
            frames_confirmed: 232,
            differing: vec![FrameNote {
                copy_t_us: 11_400_000,
                copy_index: 342,
                counter: Some(531),
                original_t_us: Some(17_700_000),
                distance: Some(34),
                reason: NoteReason::PictureDiffers,
            }],
            inconclusive: vec![],
            worst_confirmed_distance: 4,
        }
    }

    fn base() -> DeclaredCorrespondence {
        DeclaredCorrespondence {
            segments: vec![seg()],
            cuts: vec![],
            unconfirmed: vec![],
            frames_examined: 233,
            frames_declaring: 233,
            frames_confirmed: 232,
            frames_contradicted: 1,
            counters_repeated: 0,
            isolated_contradictions: 0,
            counters_set_aside: 0,
            confirm_distance: 16,
            tag_expected: Some(0xD53D),
            tags_seen: vec![(0xD53D, 233)],
            frames_without_band: 0,
        }
    }

    fn inputs<'a>() -> PageInputs<'a> {
        PageInputs {
            short_id: "lzvYrVDnmEMQ",
            chain_verdict: "PASS — 5 chunk(s) fully verified",
            chain_passed: true,
            chain_checked: true,
            original_src: "original.mp4",
            copy_src: "copy.mp4",
            original_label: "from the bundle",
            copy_label: "supplied",
            frames_read: 233,
            seconds_examined: 7.73,
            located: &[],
            diff: DiffSettings::default(),
            located_ran: false,
            out_of_place: &[],
        }
    }

    #[test]
    fn every_named_moment_is_a_control_that_drives_both_players() {
        let h = render(&base(), &inputs());
        // The differing frame is the one a reader must be able to reach.
        assert!(
            h.contains(r#"data-copy="11.400" data-orig="17.700""#),
            "{h}"
        );
        assert!(h.contains("id=\"vo\"") && h.contains("id=\"vc\""));
        assert!(h.contains("seek(vc, b.dataset.copy)"));
    }

    #[test]
    fn a_frame_that_declares_nothing_moves_only_the_supplied_player() {
        let mut r = base();
        r.segments[0].inconclusive = vec![FrameNote {
            copy_t_us: 3_000_000,
            copy_index: 90,
            counter: None,
            original_t_us: None,
            distance: None,
            reason: NoteReason::NoDeclaration,
        }];
        let h = render(&r, &inputs());
        assert!(
            h.contains(r#"<button class="at" data-copy="3.000">"#),
            "{h}"
        );
    }

    #[test]
    fn the_page_never_averages_a_shot() {
        let h = render(&base(), &inputs());
        assert!(!h.to_lowercase().contains("average of the shot"));
        assert!(h.contains("Worst confirmed"));
        assert!(h.contains("hides the one that matters"));
    }

    #[test]
    fn a_matching_signature_is_stated_as_a_result_not_implied() {
        // It used to read almost the same whether it matched or not, so a
        // reader could not see that the cheapest decisive check had run.
        let h = render(&base(), &inputs());
        assert!(h.contains("carried by all 233 frame(s)"), "{h}");
        assert!(h.contains("no other signature appears"));
        assert!(h.contains("class=\"ok\""));
    }

    #[test]
    fn no_recording_id_says_that_nothing_was_compared() {
        let mut r = base();
        r.tag_expected = None;
        let h = render(&r, &inputs());
        assert!(h.contains("compared to nothing"), "{h}");
        assert!(
            h.contains("class=\"warn\""),
            "amber, not red: nothing is wrong"
        );
    }

    #[test]
    fn an_unchecked_chain_says_what_to_run() {
        let mut i = inputs();
        i.chain_checked = false;
        let h = render(&base(), &i);
        assert!(h.contains("was not checked here"), "{h}");
        assert!(h.contains("verify_bundle.py"));
        // And it must not stop the report: nothing was established about the
        // chain, which is different from the chain having failed.
        assert!(h.contains("Shots that correspond"));
    }

    #[test]
    fn both_players_drive_each_other() {
        let h = render(&base(), &inputs());
        assert!(h.contains("wire(vc);"), "{h}");
        assert!(h.contains("wire(vo);"), "the original must drive too");
        // One conversion, through the frame number. A second route that
        // interpolated time directly used to exist beside it, and nothing
        // made the two agree: dragging the original to frame 1 put the copy
        // on frame 3.
        assert!(h.contains("function otherTime(t, side)"), "{h}");
        assert!(
            !h.contains("function map(t, from, to)"),
            "the time-interpolating route is back, and it cannot agree"
        );
    }

    #[test]
    fn a_failed_chain_stops_before_any_comparison() {
        let mut i = inputs();
        i.chain_passed = false;
        i.chain_verdict = "FAIL — chunk 3 does not verify";
        let h = render(&base(), &i);
        assert!(!h.contains("Shots that correspond"));
        assert!(h.contains("no established original"));
    }

    #[test]
    fn a_wholly_different_recording_is_not_called_a_mixture() {
        // The swapped-recording case: every frame is consistent, just not
        // this bundle's. Calling it "more than one recording" would describe
        // a different thing entirely.
        let mut r = base();
        r.tags_seen = vec![(0x68E8, 413)];
        let h = render(&r, &inputs());
        assert!(h.contains("is not the recording this bundle is for"), "{h}");
        assert!(!h.contains("mixes more than one"));
    }

    #[test]
    fn a_foreign_signature_is_named_before_any_picture_is_compared() {
        let mut r = base();
        r.tags_seen = vec![(0x68E8, 40), (0xD53D, 193)];
        assert!(r.carries_a_foreign_signature());
        let h = render(&r, &inputs());
        let sig = h.find("0x68E8").expect("signature named");
        let shots = h.find("Shots that correspond").expect("shots section");
        assert!(sig < shots, "the signature must come first");
    }

    #[test]
    fn nothing_established_is_phrased_as_an_absence() {
        let mut r = base();
        r.segments.clear();
        let h = render(&r, &inputs());
        assert!(h.contains("No correspondence was established"));
        let low = h.to_lowercase();
        for word in ["falsif", "fake", "tamper", "forged file", "detector"] {
            assert!(!low.contains(word), "accusatory word {word} in {h}");
        }
    }

    #[test]
    fn a_single_shot_with_no_cut_says_so_plainly() {
        let h = render(&base(), &inputs());
        assert!(h.contains("One shot, running from end to end"));
    }

    #[test]
    fn a_backwards_join_is_described_as_going_back() {
        let mut r = base();
        r.cuts = vec![DeclaredCut {
            copy_at_us: 9_730_000,
            counter_before: 589,
            counter_after: 4,
            original_left_us: 19_470_000,
            original_resumed_us: 100_000,
            copy_left_us: 9_730_000,
            copy_resumed_us: 9_770_000,
        }];
        let h = render(&r, &inputs());
        assert!(h.contains("the file goes back"), "{h}");
        assert!(h.contains("585 frames earlier"));
    }

    #[test]
    fn inserted_material_is_a_section_of_its_own() {
        // It sits BETWEEN two shots, so walking only the inside of each shot
        // showed a second of foreign footage as nothing at all.
        let mut r = base();
        r.unconfirmed = vec![crate::declared::UnconfirmedStretch {
            copy_start_us: 8_970_000,
            copy_end_us: 10_000_000,
            frames_examined: 31,
            frames_declaring: 3,
            frames_contradicted: 2,
        }];
        let h = render(&r, &inputs());
        assert!(h.contains("correspond to nothing in the original"));
        assert!(h.contains("1.03s, 31 frames"), "{h}");
        assert!(h.contains("do not look like it"), "{h}");
    }

    #[test]
    fn an_insertion_is_not_worded_as_a_removal() {
        let mut r = base();
        r.cuts = vec![DeclaredCut {
            copy_at_us: 9_480_000,
            counter_before: 270,
            counter_after: 271,
            original_left_us: 8_970_000,
            original_resumed_us: 9_000_000,
            copy_left_us: 8_970_000,
            copy_resumed_us: 10_000_000,
        }];
        assert_eq!(r.cuts[0].kind(), crate::declared::JoinKind::Insertion);
        let h = render(&r, &inputs());
        assert!(h.contains("the original does not account for"), "{h}");
        assert!(!h.contains("frames of the original are absent"));
    }

    #[test]
    fn the_shot_map_is_emitted_for_the_linked_players() {
        // A fixed delta would be wrong the moment there is a cut, so each shot
        // carries its own mapping.
        let mut r = base();
        r.segments.push(DeclaredSegment {
            copy_start_us: 8_000_000,
            copy_end_us: 13_730_000,
            counter_start: 421,
            counter_end: 593,
            original_start_us: 14_000_000,
            original_end_us: 19_730_000,
            frames_examined: 173,
            frames_confirmed: 173,
            differing: vec![],
            inconclusive: vec![],
            worst_confirmed_distance: 2,
        });
        let h = render(&r, &inputs());
        assert!(
            h.contains(r#"{"c0":0.000,"c1":7.730,"o0":0.000,"o1":7.730,"k0":1,"k1":233}"#),
            "{h}"
        );
        assert!(h.contains(r#"{"c0":8.000,"c1":13.730,"o0":14.000,"o1":19.730,"k0":421,"k1":593}"#));
        assert!(h.contains("id=\"link\""));
    }

    #[test]
    fn a_moment_outside_every_shot_holds_the_original_still() {
        let h = render(&base(), &inputs());
        assert!(h.contains("return null;"), "toOriginal must refuse");
        assert!(h.contains("held still"));
    }

    #[test]
    fn a_located_region_is_a_control_that_drives_both_players() {
        use crate::imagediff::{FrameDifference, Region};
        let located = vec![Located {
            copy_index: 342,
            copy_t_us: 11_400_000,
            original_t_us: 17_700_000,
            counter: 531,
            difference: FrameDifference {
                state: DifferenceState::LocalizedDifference,
                regions: vec![Region {
                    x: 0.25,
                    y: 0.5,
                    width: 0.25,
                    height: 0.25,
                    tiles: 16,
                    deviations: 9.4,
                }],
                baseline: 12.0,
                spread: 3.0,
                inconclusive_because: None,
            },
        }];
        let mut i = inputs();
        i.located = &located;
        i.located_ran = true;
        let h = render(&base(), &i);
        // One frame, so it is reported as a single-frame region and said to be
        // one — not merged into the persistent list where it would read as a
        // finding it is not.
        assert!(h.contains("25% × 25% at (25%, 50%)"), "{h}");
        assert!(h.contains(r#"data-copy="11.400""#));
        assert!(h.contains("9.4 deviations"));
        assert!(h.contains("seen in a single frame and nowhere else"));
    }

    #[test]
    fn nothing_located_is_not_a_clearance() {
        use crate::imagediff::FrameDifference;
        let located = vec![Located {
            copy_index: 1,
            copy_t_us: 0,
            original_t_us: 0,
            counter: 1,
            difference: FrameDifference {
                state: DifferenceState::ConsistentWithRecompression,
                regions: vec![],
                baseline: 8.0,
                spread: 2.0,
                inconclusive_because: None,
            },
        }];
        let mut i = inputs();
        i.located = &located;
        i.located_ran = true;
        let h = render(&base(), &i);
        assert!(h.contains("not a clearance"), "{h}");
        assert!(h.contains("smaller than a tile"));
    }

    #[test]
    fn a_run_without_the_localized_pass_says_so_rather_than_implying_nothing() {
        let h = render(&base(), &inputs());
        assert!(h.contains("Not performed in this run"));
    }

    #[test]
    fn the_players_cannot_volley() {
        // Moving one player moves the other, which makes the other fire the
        // events we listen for. The first version told its own echo apart by
        // timing — a flag lowered on the next macrotask — and a `seeked`
        // arrives long after that, so four scrubs produced 1998 events in a
        // copy/original volley. The echo is identified by its value now.
        let h = render(&base(), &inputs());
        assert!(h.contains("function isEcho(v, kind, value)"), "{h}");
        assert!(
            h.contains("slot(target).seek.push(t);"),
            "a driven seek must be recorded"
        );
        assert!(
            h.contains("slot(target).play.push(playing);"),
            "a driven play must be recorded"
        );
        // A LIST, not one value: dragging a thumb puts two driven seeks in
        // flight at once, the second overwrote the first, and the unclaimed
        // echo was read as the reader moving that player — which reversed the
        // link and left the pair a few frames out.
        assert!(h.contains("echo = { o: { seek: [], play: [] }"), "{h}");
        assert!(
            h.contains("if (Math.abs(target.currentTime - t) < 0.001) return;"),
            "arming a move that will not happen swallows the reader's next one"
        );
        assert!(
            !h.contains("applying"),
            "the timing-based guard is back, and it cannot work"
        );
        // And only the player the reader touched corrects the other while
        // both roll, or they argue over every tenth of a second.
        assert!(h.contains("if (v !== driver) return;"));
    }

    #[test]
    fn the_link_bar_is_not_a_third_player() {
        // It was a flex item beside the two figures, so the row redistributed
        // whenever the "held still" notice appeared — at a cut, which is the
        // moment a reader is looking hardest. The picture jumped 242 → 306 px.
        let h = render(&base(), &inputs());
        let row = h
            .find("<div class=\"playerrow\">")
            .expect("the row must exist");
        let bar = h.find("class=\"linkbar\"").expect("the bar must exist");
        let close = h
            .find("</div>\n<p class=\"linkbar\"")
            .expect("the bar sits after the row");
        assert!(row < close && close < bar + 40);
        assert!(
            h.contains("min-height:2.6em"),
            "the bar must reserve its space"
        );
    }

    #[test]
    fn the_reader_can_name_and_step_the_frame_they_are_on() {
        // Two frames a thirtieth of a second apart look identical, so without
        // a number a reader cannot confirm the click landed where the report
        // said — and cannot move one frame to check the neighbour.
        let h = render(&base(), &inputs());
        assert!(h.contains("id=\"fo\"") && h.contains("id=\"fc\""), "{h}");
        assert!(h.contains("function counterAt(t, side)"));
        assert!(h.contains("id=\"prevf\"") && h.contains("id=\"nextf\""));
        // Stepped by counter, never by adding a duration to each clock: both
        // the addition and the projection round, and the pair drifted one to
        // two frames apart after a few steps.
        assert!(h.contains("function timeOfCounter(k, side)"));
        assert!(h.contains("counterAt(v.currentTime, side)"));
        // The readout and the alignment must share one conversion, or what a
        // reader is shown can differ from what the players were told.
        assert!(h.contains("var t = otherTime(v.currentTime,"));
        // The period is derived from the shots, not assumed, so it is right
        // even when the two files run at different rates.
        assert!(h.contains("function periodOf(side)"));
        // No clamping at the file's edges. A clamp was added once, to cover
        // the copy starting two frames late — a symptom of the demuxer not
        // applying the edit list, fixed in mp4.js where it belonged. Clamping
        // here would let inserted material at the very start of a file read
        // as corresponding, in the players, to frames it does not match.
        assert!(!h.contains("return first.k0;"), "{h}");
    }

    #[test]
    fn the_limits_section_is_always_present() {
        let h = render(&base(), &inputs());
        assert!(h.contains("What this report cannot say"));
        assert!(h.contains("nothing here is a score"));
    }
}
