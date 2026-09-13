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
use crate::imagediff::{self, DiffSettings, DifferenceState, Located};

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
    pub chain_verdict: &'a str,
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
    h.push_str(&format!(
        "<p class=\"{}\">{}</p>\n",
        if inputs.chain_passed { "ok" } else { "bad" },
        esc(inputs.chain_verdict)
    ));
    if !inputs.chain_passed {
        h.push_str(
            "<p>The comparison below is not shown: there is no established original to \
             compare against.</p>\n</section>\n",
        );
        return h;
    }
    h.push_str(&signature_block(r));
    h.push_str("</section>\n");

    // ── The players ──────────────────────────────────────────────────────
    h.push_str("<section class=\"players\">\n");
    h.push_str(&format!(
        "<figure><figcaption>Original — {}</figcaption>\
         <video id=\"vo\" controls preload=\"metadata\" src=\"{}\"></video>\
         <label class=\"repick\">Load a file<input type=\"file\" accept=\"video/*\" data-for=\"vo\"></label>\
         </figure>\n",
        esc(inputs.original_label),
        esc(inputs.original_src)
    ));
    h.push_str(&format!(
        "<figure><figcaption>Supplied file — {}</figcaption>\
         <video id=\"vc\" controls preload=\"metadata\" src=\"{}\"></video>\
         <label class=\"repick\">Load a file<input type=\"file\" accept=\"video/*\" data-for=\"vc\"></label>\
         </figure>\n",
        esc(inputs.copy_label),
        esc(inputs.copy_src)
    ));
    h.push_str(
        "<p class=\"linkbar\"><label><input type=\"checkbox\" id=\"link\" checked> \
         Keep the two players together</label> \
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
            "<p>None, among the frames that could be judged. See section 7 for the frames \
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

    // ── Inconclusive ─────────────────────────────────────────────────────
    let total_inconclusive: usize = r.segments.iter().map(|s| s.inconclusive.len()).sum();
    h.push_str("<section><h2>7 · Frames nothing could be established about</h2>\n");
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
        out.push_str(&format!(
            "{{\"c0\":{},\"c1\":{},\"o0\":{},\"o1\":{}}}",
            secs(s.copy_start_us),
            secs(s.copy_end_us),
            secs(s.original_start_us),
            secs(s.original_end_us)
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

fn signature_block(r: &DeclaredCorrespondence) -> String {
    let mut out = String::new();
    out.push_str("<p>");
    match (r.tag_expected, r.tags_seen.as_slice()) {
        (want, []) => {
            if let Some(w) = want {
                // Say which signature was expected even when none was found.
                // A reader has to be able to check the claim, and "none read"
                // without naming the target is half a sentence.
                out.push_str(&format!(
                    "This bundle's recording signature is <code>0x{w:04X}</code>. "
                ));
            }
            out.push_str(
                "No frame of the supplied file carries a readable recording signature. \
                 Nothing follows from that on its own: the strip is destroyed by a heavy \
                 recompression and removed by a crop.",
            );
        }
        (Some(want), seen) => {
            let mine = seen.iter().find(|(t, _)| *t == want).map(|(_, n)| *n);
            let others: Vec<String> = seen
                .iter()
                .filter(|(t, _)| *t != want)
                .map(|(t, n)| format!("0x{t:04X} on {n} frame(s)"))
                .collect();
            out.push_str(&format!(
                "This bundle's recording signature is <code>0x{want:04X}</code>. \
                 {} carry it",
                match mine {
                    Some(n) => format!("{n} frame(s)"),
                    None => "No frames".to_string(),
                }
            ));
            if others.is_empty() {
                out.push('.');
            } else {
                out.push_str(&format!(
                    ". Other signatures are present: {}. Those frames were burned by a \
                     different recording — a signature is two bytes and can be drawn, but \
                     a frame carrying somebody else's is not this recording's.",
                    esc(&others.join(", "))
                ));
            }
        }
        (None, seen) => {
            let list: Vec<String> = seen
                .iter()
                .map(|(t, n)| format!("0x{t:04X} on {n} frame(s)"))
                .collect();
            out.push_str(&format!(
                "Signatures read in the supplied file: {}.",
                esc(&list.join(", "))
            ));
        }
    }
    out.push_str("</p>\n");
    out
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
        "<section><h2>8 · What this report cannot say</h2>\n<ul class=\"limits\">\n\
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
:root { color-scheme: light dark; --line:#d8d8d8; --dim:#666; --bad:#8a1c1c; --ok:#14532d; }
@media (prefers-color-scheme: dark) { :root { --line:#333; --dim:#9a9a9a; --bad:#ff9a9a; --ok:#9ae6b4; } }
body { margin:0 auto; padding:24px 16px 64px; max-width:1100px;
       font:15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif; }
h1 { font-size:22px; margin:0 0 4px; }
h2 { font-size:16px; margin:32px 0 8px; padding-bottom:4px; border-bottom:1px solid var(--line); }
.lede { color:var(--dim); max-width:70ch; }
.ok { color:var(--ok); } .bad { color:var(--bad); font-weight:600; }
.note, .sub { color:var(--dim); font-size:13px; }
.players { display:flex; gap:12px; flex-wrap:wrap; position:sticky; top:0;
           background:Canvas; padding:8px 0; z-index:5; border-bottom:1px solid var(--line); }
.players figure { flex:1 1 300px; margin:0; min-width:0; }
.players figcaption { font-size:12px; color:var(--dim); margin-bottom:4px; }
.players video { width:100%; max-height:42vh; background:#000; }
.linkbar { margin:6px 0 0; font-size:13px; display:flex; gap:10px; align-items:center; }
.players figure.adrift video { outline:2px solid var(--bad); outline-offset:-2px; }
.repick { display:inline-block; font-size:12px; color:var(--dim); margin-top:4px; cursor:pointer; }
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

  // Where a moment of the supplied file sits in the original.
  //
  // Inside a shot, by interpolation: the two run at the same rate there, and a
  // shot of a re-encoded copy is not always exactly as long as the original's,
  // so mapping the ends and interpolating between them beats adding a fixed
  // offset. Outside every shot — inserted material, an unreadable passage —
  // there is NO answer, and the honest thing is to say so rather than park the
  // original at whatever is nearest.
  function toOriginal(t) {
    for (var i = 0; i < shots.length; i++) {
      var s = shots[i];
      if (t >= s.c0 - 0.02 && t <= s.c1 + 0.02) {
        var span = s.c1 - s.c0;
        var f = span > 0.001 ? (t - s.c0) / span : 0;
        return s.o0 + (s.o1 - s.o0) * f;
      }
    }
    return null;
  }

  var applying = false;          // guard against the echo of our own seek
  function linked() { return link && link.checked; }

  function setAdrift(yes, why) {
    var fig = vo.closest('figure');
    if (fig) fig.classList.toggle('adrift', !!yes);
    if (state) state.textContent = yes ? why : '';
  }

  // Put the original where the supplied file currently is.
  function follow(force) {
    if (!linked() || applying) return;
    var t = toOriginal(vc.currentTime);
    if (t === null) {
      // Nothing in the original corresponds to this moment. Freeze rather
      // than drift: a player showing an unrelated frame beside a claim is
      // worse than a player that has stopped.
      if (!vo.paused) vo.pause();
      setAdrift(true, 'the original has no counterpart to this moment — it is held still');
      return;
    }
    setAdrift(false, '');
    if (force || Math.abs(vo.currentTime - t) > 0.15) {
      applying = true;
      try { vo.currentTime = t; } catch (e) {}
      setTimeout(function () { applying = false; }, 0);
    }
  }

  vc.addEventListener('seeked', function () { follow(true); });
  vc.addEventListener('timeupdate', function () { follow(false); });
  vc.addEventListener('play', function () {
    if (linked() && toOriginal(vc.currentTime) !== null) { vo.play().catch(function () {}); }
  });
  vc.addEventListener('pause', function () {
    if (!linked()) return;
    vo.pause();
    // Land exactly on pause. While both are rolling the correction is kept
    // loose on purpose — seeking a playing video to shave off a tenth of a
    // second makes it stutter, and nobody compares two frames mid-playback.
    // The moment you stop is the moment it has to be exact.
    follow(true);
  });
  vc.addEventListener('ratechange', function () { if (linked()) vo.playbackRate = vc.playbackRate; });

  // The original is the reference, so driving it drives nothing back — except
  // pausing, which must stop both or the pair silently drifts apart.
  vo.addEventListener('pause', function () { if (linked() && !vc.paused) vc.pause(); });

  if (link) {
    link.addEventListener('change', function () {
      if (link.checked) { follow(true); } else { setAdrift(false, ''); }
    });
  }

  // Seek, then pause: the point is to compare two still frames, and a player
  // that keeps rolling has moved off the frame by the time you look at it.
  function seek(v, t) {
    if (!v || t === undefined || t === null || t === '') return;
    try { v.pause(); v.currentTime = parseFloat(t); } catch (e) {}
  }
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('button.at');
    if (!b) return;
    applying = true;
    seek(vc, b.dataset.copy);
    if (b.dataset.orig !== undefined) {
      seek(vo, b.dataset.orig);
      setAdrift(false, '');
    } else {
      vo.pause();
      setAdrift(true, 'the original has no counterpart to this moment — it is held still');
    }
    setTimeout(function () { applying = false; }, 0);
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
            original_src: "original.mp4",
            copy_src: "copy.mp4",
            original_label: "from the bundle",
            copy_label: "supplied",
            frames_read: 233,
            seconds_examined: 7.73,
            located: &[],
            diff: DiffSettings::default(),
            located_ran: false,
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
    fn a_failed_chain_stops_before_any_comparison() {
        let mut i = inputs();
        i.chain_passed = false;
        i.chain_verdict = "FAIL — chunk 3 does not verify";
        let h = render(&base(), &i);
        assert!(!h.contains("Shots that correspond"));
        assert!(h.contains("no established original"));
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
            h.contains(r#"{"c0":0.000,"c1":7.730,"o0":0.000,"o1":7.730}"#),
            "{h}"
        );
        assert!(
            h.contains(r#"{"c0":8.000,"c1":13.730,"o0":14.000,"o1":19.730}"#),
            "{h}"
        );
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
    fn the_limits_section_is_always_present() {
        let h = render(&base(), &inputs());
        assert!(h.contains("What this report cannot say"));
        assert!(h.contains("nothing here is a score"));
    }
}
