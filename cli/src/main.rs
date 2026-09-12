//! `edit-report` — compare a video against a Forsheur evidence bundle and
//! describe what differs.
//!
//! This is an edit report, not a detector. It produces a description: which
//! parts of one video correspond to which parts of another, where the cuts
//! are, and where the picture differs. It produces no score, and it concludes
//! nothing about whether what the picture shows is true.
//!
//! Milestone 1 — this build — stops before the comparison. It ingests the
//! bundle, has the bundle's own verifier establish the original's
//! cryptographic state, reads whatever codes are burned into the picture, and
//! writes the report.
//!
//! It makes no network request of any kind. There is no HTTP client in this
//! binary; the property is structural, not a promise.

use edit_report::{bundle_dir, decode, editpass, pyverify};

use edit_report_core::align::{self, Correspondence, Cut, Sample};
use edit_report_core::bundle::Manifest;
use edit_report_core::fingerprint::fingerprint;
use edit_report_core::edit_page;
use edit_report_core::html;
use edit_report_core::normalize::Plan;
use edit_report_core::qr::{sample_positions, QrFindings, ScanConfig, Survey};
use edit_report_core::report::{
    milestone_limits, universal_limits, Milestone, Original, QrSection, Report, Section,
    SourceKind, Tool, SCHEMA,
};
use edit_report_core::verifier::{CryptoState, NotEstablished};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
edit-report — describe how a video relates to a Forsheur evidence bundle

USAGE:
    edit-report --bundle <PATH> [OPTIONS]

    <PATH> is an evidence bundle .zip, or a directory it was unpacked into.

OPTIONS:
    --bundle <PATH>        The evidence bundle holding the original. Required.
    --video <PATH>         The video to compare against it. Without this, the
                           report establishes the original and stops there.
    --html <PATH>          Write the edit report here: the shots, the joins
                           between them, and the two players side by side.
                           Needs --video; without one there is nothing to
                           report on and --chain-html is written instead.
    --chain-html <PATH>    Write the older blind-comparison report here — what
                           the bundle establishes about the original, plus the
                           correspondence found without reading the burn-in.
    --json <PATH>          Write the machine-readable report here.
    --qr-frames <N>        Frames to sweep for burned-in codes (default 24,
                           spread across the whole recording).
    --samples-per-second <N>
                           Frames sampled per second of content, on BOTH sides,
                           for alignment (default 4). Both sides must be sampled
                           at the same rate in time; finer costs proportionally
                           more and buys accuracy on fast pans.
    --sensitivity <N>      How far above a frame's OWN noise a region must sit
                           before the report locates a difference in it, in
                           median-absolute-deviations (default 6). Lower finds
                           more and cries wolf more. There is no threshold in
                           grey levels here and there never will be: each frame
                           is judged against its own distribution, so a
                           recompressed one raises its own bar.
    --diff-grid <N>        Tiles across the frame for that comparison
                           (default 16, so 16×16).
    --python <PATH>        Interpreter used to run the bundle's verify_bundle.py.
    --skip-crypto          Do not run the bundle's verifier. The report then
                           states that the chain was not established here.
    -h, --help             This text.

NOTES:
    This tool never re-implements the cryptographic checks. It runs
    verify_bundle.py from inside the bundle and quotes it. With neither --html
    nor --json, a summary is printed and no file is written.

    An encrypted bundle carries ciphertext, so there is no picture to read.
    Decrypt it first with the bundle's own verifier, then point this tool at
    the resulting directory:

        python3 verify_bundle.py . --extract --id-priv
        edit-report --bundle .
";

struct Args {
    bundle: PathBuf,
    video: Option<PathBuf>,
    html: Option<PathBuf>,
    chain_html: Option<PathBuf>,
    json: Option<PathBuf>,
    qr_frames: u32,
    samples_per_second: f64,
    python: Option<String>,
    skip_crypto: bool,
    diff: edit_report_core::imagediff::DiffSettings,
}

fn parse_args() -> Result<Args, String> {
    // Hand-rolled, and staying that way: an argument parser is a dependency
    // whose whole job is a match statement, and every dependency in a tool
    // meant to be audited is something a reader has to go and read.
    let mut bundle = None;
    let mut video = None;
    let mut html = None;
    let mut chain_html = None;
    let mut json = None;
    let mut qr_frames = 24u32;
    let mut samples_per_second = decode::SAMPLES_PER_SECOND;
    let mut python = None;
    let mut skip_crypto = false;
    let mut diff = edit_report_core::imagediff::DiffSettings::default();

    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        let a = a.to_string_lossy().into_owned();
        // Paths come in as OsString and stay that way: a Windows path with an
        // accent in it is not always valid UTF-8, and lossy-converting one
        // would open a different file than the user typed.
        let mut next_path = || -> Result<PathBuf, String> {
            it.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{a} needs a value"))
        };
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--bundle" => bundle = Some(next_path()?),
            "--video" => video = Some(next_path()?),
            "--html" => html = Some(next_path()?),
            "--chain-html" => chain_html = Some(next_path()?),
            "--json" => json = Some(next_path()?),
            "--python" => {
                python = it.next().map(|v| v.to_string_lossy().into_owned());
                if python.is_none() {
                    return Err("--python needs a value".into());
                }
            }
            "--qr-frames" => {
                let v = it.next().ok_or("--qr-frames needs a value")?;
                qr_frames = v.to_string_lossy().parse().map_err(|_| {
                    format!("--qr-frames wants a number, got {:?}", v.to_string_lossy())
                })?;
            }
            "--samples-per-second" => {
                let v = it.next().ok_or("--samples-per-second needs a value")?;
                samples_per_second = v.to_string_lossy().parse().map_err(|_| {
                    format!(
                        "--samples-per-second wants a number, got {:?}",
                        v.to_string_lossy()
                    )
                })?;
                if !(samples_per_second.is_finite() && samples_per_second > 0.0) {
                    return Err("--samples-per-second must be a positive number".into());
                }
            }
            "--sensitivity" => {
                let v = it.next().ok_or("--sensitivity needs a value")?;
                diff.sensitivity = v.to_string_lossy().parse().map_err(|_| {
                    format!("--sensitivity wants a number, got {:?}", v.to_string_lossy())
                })?;
                if !(diff.sensitivity.is_finite() && diff.sensitivity > 0.0) {
                    return Err("--sensitivity must be a positive number".into());
                }
            }
            "--diff-grid" => {
                let v = it.next().ok_or("--diff-grid needs a value")?;
                let n: u32 = v.to_string_lossy().parse().map_err(|_| {
                    format!("--diff-grid wants a number, got {:?}", v.to_string_lossy())
                })?;
                if !(2..=64).contains(&n) {
                    return Err("--diff-grid must be between 2 and 64".into());
                }
                diff.cols = n;
                diff.rows = n;
            }
            "--skip-crypto" => skip_crypto = true,
            other => return Err(format!("unknown option {other:?}")),
        }
    }

    Ok(Args {
        bundle: bundle.ok_or("--bundle is required")?,
        video,
        html,
        chain_html,
        json,
        qr_frames,
        samples_per_second,
        python,
        skip_crypto,
        diff,
    })
}

fn main() -> ExitCode {
    // Windows consoles default to a legacy code page; the transcript we quote
    // is UTF-8 and contains check marks and box drawing.
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn SetConsoleOutputCP(cp: u32) -> i32;
        }
        SetConsoleOutputCP(65001);
    }

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("edit-report: {e}\n");
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("edit-report: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &Args) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let bundle = bundle_dir::open(&args.bundle)?;
    let manifest = Manifest::parse(&std::fs::read(bundle.path("manifest.json"))?)?;

    eprintln!(
        "Bundle: {} ({} sealed segment(s), {})",
        manifest.label(),
        manifest.chunks.len(),
        html::transmission_short(manifest.transmission()),
    );

    // ── 1. The chain, established by the bundle's own verifier ─────────────
    //
    // Extraction is asked for in the same run: `--extract` writes only the
    // payloads that verified, so the media this tool goes on to read is media
    // the reference verifier vouched for. An encrypted bundle is not asked to
    // extract — there is no key here and never will be.
    let transmission = manifest.transmission();
    let want_media = !transmission.payload_is_ciphertext();
    let (crypto, extracted) = if args.skip_crypto {
        (
            CryptoState::not_established(NotEstablished::SkippedByOperator),
            false,
        )
    } else {
        eprintln!("Running the bundle's own verifier (verify_bundle.py)…");
        let run = pyverify::run(&bundle.root, args.python.as_deref(), want_media);
        (run.state, run.extracted)
    };
    eprintln!("  {}", crypto.headline());

    // ── 2. The media, and the codes burned into it ─────────────────────────
    let mut media = Vec::new();
    let mut qr_original = QrFindings {
        frames_examined: 0,
        frames_with_code: 0,
        codes: Vec::new(),
    };
    let mut observations = Vec::new();
    // Path to the stream the comparison reads: the back camera, the one
    // pointed at the scene.
    let mut original_video: Option<PathBuf> = None;
    let mut qr_copy: Option<QrFindings> = None;

    if transmission.payload_is_ciphertext() {
        observations.push(
            "This recording is end-to-end encrypted, so the bundle carries ciphertext and no \
             picture could be read. Decrypt it with the bundle's own verifier \
             (verify_bundle.py --extract --id-priv) and point this tool at the resulting \
             directory."
                .to_string(),
        );
    } else if !extracted {
        observations.push(
            "No verified media was available to read, so no frame was examined. This describes \
             what this run could do, not the recording."
                .to_string(),
        );
    } else {
        let streams = find_streams(&bundle.root);
        if streams.is_empty() {
            observations
                .push("The bundle verified but produced no video stream to read.".to_string());
        }
        // Audio rides as its own stream in the payload, so the video `.bin`
        // never carries it. Asking ffprobe about the video file alone would
        // report "no audio" for a recording that has plenty, and milestone 2
        // decides whether an audio alignment pass is even possible from that
        // flag.
        let has_audio = audio_stream_present(&bundle.root);
        original_video = streams.first().map(|(_, p)| p.clone());
        for (stream_id, path) in streams {
            let mut profile = decode::probe(&path, Some(stream_id.clone()))?;
            profile.has_audio = has_audio;
            eprintln!(
                "  {}: {}×{}, {} frames",
                stream_id, profile.width, profile.height, profile.frame_count
            );

            // Only the back camera is swept. Both cameras carry the same code
            // by construction, so sweeping the second buys a duplicate row and
            // doubles the work.
            if qr_original.frames_examined == 0 {
                let positions = sample_positions(profile.frame_count, args.qr_frames);
                let frames = decode::decode_positions(&path, &profile, &positions)?;
                let mut survey = Survey::new(ScanConfig::default());
                for f in &frames {
                    survey.examine(f);
                }
                qr_original = survey.finish();
                eprintln!(
                    "  burn-in: {} code(s) over {} frame(s) examined",
                    qr_original.codes.len(),
                    qr_original.frames_examined
                );
            }
            media.push(profile);
        }
        observations.extend(cross_check(&manifest, &qr_original));
    }

    // ── 3. The comparison ──────────────────────────────────────────────────
    //
    // Only when there is a second video AND the original was established. §5 of
    // the brief: comparing against an original that was never established means
    // nothing, so the tool declines rather than producing a table nobody can
    // read anything into.
    let mut copy_report: Option<edit_report_core::report::Copy> = None;
    let mut correspondence: Section<Correspondence> =
        Section::not_performed("no video was supplied to compare — pass --video");
    let mut cuts_section: Section<Vec<Cut>> =
        Section::not_performed("no video was supplied to compare — pass --video");
    let mut milestone = Milestone::OriginalOnly;

    if let Some(video_path) = &args.video {
        if !crypto.is_verified() {
            let why = "the original was not cryptographically established, so no comparison was \
                       performed — a correspondence against an unestablished original would mean \
                       nothing";
            correspondence = Section::not_performed(why);
            cuts_section = Section::not_performed(why);
            copy_report = Some(edit_report_core::report::Copy {
                file_name: file_name_of(video_path),
                media: None,
                bars_removed: None,
                bars_description: None,
            });
        } else if let Some(orig_path) = &original_video {
            milestone = Milestone::Correspondence;
            let (copy, corr, found) = compare(orig_path, video_path, args, &mut media)?;
            observations.extend(compare_observations(&corr, &qr_original, &found));
            qr_copy = Some(found);
            cuts_section = Section::Performed(corr.cuts.clone());
            correspondence = Section::Performed(corr);
            copy_report = Some(copy);
        } else {
            let why = "no readable picture was available from the bundle, so there was nothing \
                       to compare against";
            correspondence = Section::not_performed(why);
            cuts_section = Section::not_performed(why);
        }
    }

    // ── 4. The report ──────────────────────────────────────────────────────
    // Taken before `crypto` moves into the report: the edit report quotes the
    // same verdict rather than deriving a second one.
    let chain_headline = crypto.headline().to_string();
    let chain_verified = crypto.is_verified();
    let mut limits = universal_limits();
    limits.extend(milestone_limits(milestone));
    let span = manifest.capture_span_us();

    let report = Report {
        schema: SCHEMA.to_string(),
        generated_at: html::iso_utc(now_us()),
        tool: Tool {
            name: "edit-report".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            decoder: decode::decoder_name(),
        },
        milestone,
        original: Original {
            source_kind: if bundle.was_archive {
                SourceKind::BundleZip
            } else {
                SourceKind::BundleDirectory
            },
            label: manifest.label().to_string(),
            session_id: manifest.session.session_id.clone(),
            short_id: manifest.session.short_id.clone(),
            encryption: manifest.session.encryption.clone(),
            transmission,
            chunk_count: manifest.chunks.len(),
            capture_start_us: span.map(|s| s.0),
            capture_end_us: span.map(|s| s.1),
            crypto,
            media,
        },
        copy: copy_report,
        qr: QrSection {
            original: qr_original,
            copy: qr_copy,
            observations,
        },
        correspondence,
        cuts: cuts_section,
        image_differences: Section::not_performed(
            "this section belongs to the blind path, which maps segments and locates cuts \
             only. The picture comparison runs on the declaration path and is written to \
             --html, where it has its own section",
        ),
        limits,
    };

    if let Some(p) = &args.chain_html {
        std::fs::write(p, html::render(&report))?;
        eprintln!("Wrote {} (chain and blind comparison)", p.display());
    }
    if let Some(p) = &args.json {
        std::fs::write(p, report.to_json())?;
        eprintln!("Wrote {}", p.display());
    }

    // ── 5. The edit report ─────────────────────────────────────────────────
    //
    // A second pass, and deliberately not folded into the one above. That one
    // samples both sides to align a copy whose burn-in may be gone; this one
    // reads EVERY frame of both and asks the original about each counter the
    // copy declares. They answer different questions and neither subsumes the
    // other, so they are run separately and reported separately.
    if let Some(p) = &args.html {
        match (&args.video, &original_video) {
            (Some(copy_path), Some(orig_path)) if chain_verified => {
                let r = edit_pass(
                    orig_path,
                    copy_path,
                    manifest.session.short_id.as_deref().unwrap_or(""),
                    &chain_headline,
                    args.diff,
                    p,
                )?;
                eprintln!(
                    "Wrote {} ({} shot(s), {} join(s))",
                    p.display(),
                    r.0,
                    r.1
                );
            }
            _ => {
                std::fs::write(p, html::render(&report))?;
                eprintln!(
                    "Wrote {} — the chain report, because {}",
                    p.display(),
                    if args.video.is_none() {
                        "no --video was given"
                    } else if !chain_verified {
                        "the original was not established"
                    } else {
                        "the bundle offered no readable picture"
                    }
                );
            }
        }
    }

    if args.html.is_none() && args.chain_html.is_none() && args.json.is_none() {
        print_summary(&report);
    }

    // Exit code says whether the run could do its job, never what it thinks of
    // the recording. A bundle whose chain did not verify is a successful run
    // that reports a failed chain — the report is the output, not the code.
    Ok(ExitCode::SUCCESS)
}

/// Run the declaration pass and write the edit report. Returns (shots, joins).
fn edit_pass(
    original: &Path,
    copy: &Path,
    short_id: &str,
    chain_headline: &str,
    diff: edit_report_core::imagediff::DiffSettings,
    out: &Path,
) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    eprintln!("Reading every frame of both files…");
    let pass = editpass::run(original, copy, short_id, diff)?;
    eprintln!(
        "  original {} frame(s), copy {} frame(s), {:.1}s",
        pass.original_frames,
        pass.copy_frames,
        pass.elapsed.as_secs_f64()
    );
    {
        use edit_report_core::imagediff::DifferenceState as D;
        let count = |w: D| pass.located.iter().filter(|l| l.difference.state == w).count();
        eprintln!(
            "  picture compared on {} frame(s): {} even, {} located, {} inconclusive",
            pass.located.len(),
            count(D::ConsistentWithRecompression),
            count(D::LocalizedDifference),
            count(D::Inconclusive),
        );
    }
    if !pass.original_is_readable() {
        eprintln!(
            "  no machine-readable strip on any frame of the original — this recording \
             predates it, so the report below establishes nothing about the shots"
        );
    }

    let page = edit_page::render(
        &pass.correspondence,
        &edit_page::PageInputs {
            short_id,
            chain_verdict: chain_headline,
            chain_passed: true,
            original_src: &editpass::relative(out, original),
            copy_src: &editpass::relative(out, copy),
            original_label: &file_name_of(original).unwrap_or_default(),
            copy_label: &file_name_of(copy).unwrap_or_default(),
            frames_read: pass.copy_frames,
            seconds_examined: (pass.copy_duration_us.max(0) as f64) / 1e6,
            located: &pass.located,
            diff: pass.diff_settings,
            located_ran: true,
        },
    );
    std::fs::write(out, page)?;
    Ok((
        pass.correspondence.segments.len(),
        pass.correspondence.cuts.len(),
    ))
}

/// Streams `verify_bundle.py --extract` wrote, as (stream id, path).
///
/// Reads the concatenated fMP4 the verifier assembled from verified payloads,
/// not the `.mp4` it may have remuxed with ffmpeg: the `.bin` is the byte
/// sequence the device signed, and one fewer transformation sits between the
/// signature and the pixels we measure.
fn find_streams(root: &Path) -> Vec<(String, PathBuf)> {
    let dir = root.join("extracted");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bin"))
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().into_owned();
            name.starts_with("video.").then_some((name, p))
        })
        .collect();
    // Back camera first: it is the one pointed at the scene.
    out.sort_by_key(|(id, _)| (!id.starts_with("video.back"), id.clone()));
    out
}

fn file_name_of(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Decode both sides, normalise them, fingerprint them, and align.
///
/// Normalisation happens per side and BEFORE any fingerprint is taken. A
/// platform that letterboxed the copy has moved every pixel; comparing without
/// undoing that reports a video modified from end to end, about the most
/// ordinary thing that happens to a video. It is the first bug this code would
/// have, so it is the first thing it does.
fn compare(
    original_path: &Path,
    copy_path: &Path,
    args: &Args,
    original_media: &mut [edit_report_core::report::MediaProfile],
) -> Result<(edit_report_core::report::Copy, Correspondence, QrFindings), Box<dyn std::error::Error>>
{
    let copy_profile = decode::probe(copy_path, None)?;
    eprintln!(
        "Copy: {} — {}×{}, {} frames, {:.1} s",
        copy_path.display(),
        copy_profile.width,
        copy_profile.height,
        copy_profile.frame_count,
        copy_profile.duration_us as f64 / 1e6,
    );

    let orig_profile = decode::probe(original_path, None)?;

    // Bars are decided on a couple of dozen frames spread across each video —
    // enough for a majority to agree, cheap enough to collect.
    let orig_bar_frames = decode::decode_spread(original_path, &orig_profile, 24)?;
    let copy_bar_frames = decode::decode_spread(copy_path, &copy_profile, 24)?;

    // Read the copy's own burn-in while those frames are in hand. Secondary by
    // design: it says which recording the copy CLAIMS to be, and the answer to
    // whether it is comes from the pictures below.
    let mut survey = Survey::new(ScanConfig::default());
    for f in &copy_bar_frames {
        survey.examine(f);
    }
    let qr_copy = survey.finish();
    eprintln!(
        "  copy burn-in: {} code(s) over {} frame(s) examined",
        qr_copy.codes.len(),
        qr_copy.frames_examined
    );

    let orig_plan = Plan::from_frames(&orig_bar_frames);
    let copy_plan = Plan::from_frames(&copy_bar_frames);
    if let Some(p) = &copy_plan {
        eprintln!("  copy normalisation: {}", p.bars.describe());
    }
    if let Some(p) = &orig_plan {
        eprintln!("  original normalisation: {}", p.bars.describe());
        // The original's own profile carries what was done to it, so the two
        // sides of the report can be read against each other.
        if let Some(m) = original_media.first_mut() {
            m.bars_removed = Some(p.bars);
        }
    }

    // Both sides are sampled at the same rate in TIME, and fingerprinted as the
    // frames arrive so no video is ever held in memory.
    let sample_side = |path: &Path,
                       profile: &edit_report_core::report::MediaProfile,
                       plan: &Option<Plan>|
     -> Result<Vec<Sample>, decode::DecodeError> {
        let mut out = Vec::new();
        let stride = decode::stride_for_rate(profile, args.samples_per_second);
        decode::decode_stream(path, profile, stride, |f| {
            let normalised = match plan {
                Some(p) => p.apply(&f),
                None => Some(f.clone()),
            };
            if let Some(n) = normalised {
                out.push(Sample {
                    index: f.index,
                    t_us: f.pts_us,
                    fp: fingerprint(&n),
                });
            }
        })?;
        Ok(out)
    };

    let orig_samples = sample_side(original_path, &orig_profile, &orig_plan)?;
    let copy_samples = sample_side(copy_path, &copy_profile, &copy_plan)?;
    eprintln!(
        "  fingerprinted {} original and {} copy frame(s) at {} per second",
        orig_samples.len(),
        copy_samples.len(),
        args.samples_per_second,
    );

    let corr = align::align(&copy_samples, &orig_samples);
    eprintln!(
        "  correspondence: {} segment(s), {} cut(s), {} unmatched stretch(es); {}/{} copy frames matched",
        corr.segments.len(),
        corr.cuts.len(),
        corr.unmatched.len(),
        corr.copy_frames_matched,
        corr.copy_frames_examined,
    );

    let mut media = copy_profile;
    media.bars_removed = copy_plan.map(|p| p.bars);
    let copy = edit_report_core::report::Copy {
        file_name: file_name_of(copy_path),
        bars_removed: media.bars_removed,
        bars_description: copy_plan.map(|p| p.bars.describe()),
        media: Some(media),
    };
    Ok((copy, corr, qr_copy))
}

/// Whether the verifier wrote an audio stream alongside the video ones.
fn audio_stream_present(root: &Path) -> bool {
    std::fs::read_dir(root.join("extracted"))
        .map(|d| {
            d.filter_map(|e| e.ok()).any(|e| {
                let p = e.path();
                p.extension().is_some_and(|x| x == "bin")
                    && p.file_stem()
                        .is_some_and(|n| n.to_string_lossy().starts_with("audio."))
            })
        })
        .unwrap_or(false)
}

/// Plain-language readings of the comparison.
///
/// Every one of these is a statement about what was measured. None concludes
/// anything about authenticity, and the negative cases are phrased as absence
/// of a reading — never as a finding against the video.
fn compare_observations(
    corr: &Correspondence,
    qr_original: &QrFindings,
    qr_copy: &QrFindings,
) -> Vec<String> {
    let mut out = Vec::new();

    if corr.nothing_established() {
        out.push(format!(
            "No correspondence was established between the two videos, over {} frame(s) examined \
             in the copy. This is what an unrelated recording produces, and equally what a crop, \
             a very heavy re-encode, a mirror or a speed change produce. It is not a finding \
             about the copy.",
            corr.copy_frames_examined
        ));
    } else {
        let secs = corr.matched_duration_us() as f64 / 1e6;
        out.push(format!(
            "{} stretch(es) of the copy correspond to stretches of the original, covering {:.1} s \
             of the copy, on {} of {} frame(s) examined.",
            corr.segments.len(),
            secs,
            corr.copy_frames_matched,
            corr.copy_frames_examined
        ));
    }

    if !corr.cuts.is_empty() {
        let back = corr.cuts.iter().filter(|c| c.goes_backwards()).count();
        out.push(format!(
            "{} discontinuit(ies) in the correspondence{}. A video that was edited has these; \
             their presence is expected and is not itself irregular.",
            corr.cuts.len(),
            if back > 0 {
                format!(", {back} of which re-order the original rather than skip forward")
            } else {
                String::new()
            }
        ));
    }

    if let Some(caveat) = corr.original_self_similarity.caveat() {
        out.push(caveat);
    }

    if corr.copy_frames_low_variance > 0 {
        out.push(format!(
            "{} of the copy's examined frames carry too little structure to anchor a match — a \
             dark passage, a blank wall, a fade. They were left out of the alignment rather than \
             matched to whatever else was blank.",
            corr.copy_frames_low_variance
        ));
    }

    // The QR cross-check, once both sides are known.
    let orig_ids = qr_original.distinct_short_ids();
    let copy_ids = qr_copy.distinct_short_ids();
    if copy_ids.is_empty() {
        out.push(format!(
            "No code was read in the copy, over {} frame(s) examined. The burn-in sits at the top \
             of the frame, where a crop or a caption bar removes it, so this says nothing about \
             where the copy came from.",
            qr_copy.frames_examined
        ));
    } else if copy_ids.iter().any(|id| orig_ids.contains(id)) {
        out.push(
            "The copy carries a code naming the same recording as the bundle. Those pixels are \
             not signed — anyone can render them — so the code says what the copy claims to be, \
             and the correspondence above is what says how much of it is there."
                .to_string(),
        );
    } else {
        out.push(format!(
            "The copy carries {} reference(s) — {} — which do not name the recording in this \
             bundle. A video can show another video, and a burn-in can be copied.",
            copy_ids.len(),
            copy_ids.join(", ")
        ));
    }

    // The combination worth naming out loud, because it is the one a reader is
    // most likely to misread in the dangerous direction.
    if corr.nothing_established()
        && !copy_ids.is_empty()
        && copy_ids.iter().any(|id| orig_ids.contains(id))
    {
        out.push(
            "The copy names this recording and yet no correspondence was established. Both facts \
             stand as measured; neither explains the other. A heavily degraded copy, a crop, and \
             a code rendered into unrelated footage all produce this pair, and this tool cannot \
             tell them apart."
                .to_string(),
        );
    }
    out
}

/// Plain-language readings of the sweep. Statements about codes, never about
/// authenticity, and never phrased so that an absence reads as a finding.
fn cross_check(manifest: &Manifest, qr: &QrFindings) -> Vec<String> {
    let mut out = Vec::new();
    let Some(short_id) = manifest.session.short_id.as_deref() else {
        return out;
    };

    if qr.is_empty() {
        out.push(format!(
            "No code was read in the original, over {} frame(s) examined. The burn-in can be \
             absent from a recording made before it existed, or unreadable at this resolution.",
            qr.frames_examined
        ));
        return out;
    }

    let ids = qr.distinct_short_ids();
    if ids.contains(&short_id) {
        out.push(format!(
            "The picture carries a code naming this same recording ({short_id}), which is what \
             a Forsheur recording burns into its own frames. Those pixels are not signed: they \
             agree with the manifest, they do not prove it."
        ));
    } else if !ids.is_empty() {
        out.push(format!(
            "The picture carries {} reference(s) — {} — while the bundle's manifest names \
             {short_id}. A recording can show another recording, and a burn-in can be copied; \
             what the pictures have in common is a separate question.",
            ids.len(),
            ids.join(", ")
        ));
    }
    if ids.len() > 1 {
        out.push(
            "More than one distinct reference appears across the frames examined. That is \
             reported as observed and is not by itself irregular."
                .to_string(),
        );
    }
    out
}

fn print_summary(r: &Report) {
    println!("\n── Edit report — {} ──\n", r.original.label);
    println!("1. Cryptographic state of the original");
    println!(" {}\n", r.original.crypto.headline());
    println!("2. Codes burned into the picture");
    if r.qr.original.is_empty() {
        println!(
            " none read over {} frame(s) examined",
            r.qr.original.frames_examined
        );
    } else {
        for c in &r.qr.original.codes {
            println!(
                " {}  ({} of {} frames)",
                c.payload, c.frames, r.qr.original.frames_examined
            );
        }
    }
    for o in &r.qr.observations {
        println!(" · {o}");
    }
    println!("\n3-5. Correspondence, cuts, image differences");
    println!(" not performed — this build establishes the original only\n");
    println!("6. Limits");
    for l in &r.limits {
        println!(" · {l}");
    }
    println!("\nPass --html or --json to write the full report.");
}

fn now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}
