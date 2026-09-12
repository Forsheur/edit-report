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

use crate::declared::{DeclaredCorrespondence, DeclaredSegment, FrameNote, NoteReason};

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
}

pub fn render(r: &DeclaredCorrespondence, inputs: &PageInputs) -> String {
    let mut h = String::with_capacity(16_000);
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    h.push_str(&format!(
        "<title>Edit report — {}</title>\n",
        esc(inputs.short_id)
    ));
    h.push_str(STYLE);
    h.push_str("</head><body>\n");

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
             compare against.</p>\n</section>\n</body></html>\n",
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
            let body = if c.goes_backwards() {
                format!(
                    "the file goes back: frame {} is followed by frame {}, \
                     {} frames earlier in the original",
                    c.counter_before,
                    c.counter_after,
                    skipped.unsigned_abs()
                )
            } else {
                format!(
                    "frame {} is followed by frame {} — {} frames of the original are absent",
                    c.counter_before,
                    c.counter_after,
                    skipped.max(0)
                )
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

    // ── Frames that differ ───────────────────────────────────────────────
    h.push_str("<section><h2>4 · Frames whose picture differs from the original's</h2>\n");
    let total_differing: usize = r.segments.iter().map(|s| s.differing.len()).sum();
    if total_differing == 0 {
        h.push_str(
            "<p>None, among the frames that could be judged. See section 5 for the frames \
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

    // ── Inconclusive ─────────────────────────────────────────────────────
    let total_inconclusive: usize = r.segments.iter().map(|s| s.inconclusive.len()).sum();
    h.push_str("<section><h2>5 · Frames nothing could be established about</h2>\n");
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

    h.push_str(SCRIPT);
    h.push_str("</body></html>\n");
    h
}

fn signature_block(r: &DeclaredCorrespondence) -> String {
    let mut out = String::new();
    out.push_str("<p>");
    match (r.tag_expected, r.tags_seen.as_slice()) {
        (_, []) => out.push_str(
            "No frame of the supplied file carries a readable recording signature. \
             Nothing follows from that on its own: the strip is destroyed by a heavy \
             recompression and removed by a crop.",
        ),
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
        at(s.copy_start_us, Some(s.original_start_us), &clock(s.copy_start_us)),
        at(s.copy_end_us, Some(s.original_end_us), &clock(s.copy_end_us)),
        at(s.copy_start_us, Some(s.original_start_us), &clock(s.original_start_us)),
        at(s.copy_end_us, Some(s.original_end_us), &clock(s.original_end_us)),
        s.frames_examined,
        s.frames_confirmed,
        s.differing.len(),
        s.inconclusive.len(),
        s.worst_confirmed_distance.min(confirm.max(s.worst_confirmed_distance)),
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
        "<section><h2>6 · What this report cannot say</h2>\n<ul class=\"limits\">\n\
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

const STYLE: &str = r#"<style>
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

const SCRIPT: &str = r#"<script>
(function () {
  var vo = document.getElementById('vo');
  var vc = document.getElementById('vc');
  // Seek, then pause: the point is to compare two still frames, and a player
  // that keeps rolling has moved off the frame by the time you look at it.
  function seek(v, t) {
    if (!v || t === undefined || t === null || t === '') return;
    try { v.pause(); v.currentTime = parseFloat(t); } catch (e) {}
  }
  document.addEventListener('click', function (e) {
    var b = e.target.closest && e.target.closest('button.at');
    if (!b) return;
    seek(vc, b.dataset.copy);
    seek(vo, b.dataset.orig);
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
})();
</script>
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
        }
    }

    #[test]
    fn every_named_moment_is_a_control_that_drives_both_players() {
        let h = render(&base(), &inputs());
        // The differing frame is the one a reader must be able to reach.
        assert!(h.contains(r#"data-copy="11.400" data-orig="17.700""#), "{h}");
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
        assert!(h.contains(r#"<button class="at" data-copy="3.000">"#), "{h}");
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
        }];
        let h = render(&r, &inputs());
        assert!(h.contains("the file goes back"), "{h}");
        assert!(h.contains("585 frames earlier"));
    }

    #[test]
    fn the_limits_section_is_always_present() {
        let h = render(&base(), &inputs());
        assert!(h.contains("What this report cannot say"));
        assert!(h.contains("nothing here is a score"));
    }
}
