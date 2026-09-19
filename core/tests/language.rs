//! §9 of the brief, given the separate test it asks for.
//!
//! Every check below builds a report for a situation that is entirely
//! legitimate — a heavily re-encoded copy, a crop that removed the burn-in, a
//! bundle nobody could verify because Python was missing — and asserts that
//! the words that come out do not accuse anyone of anything.
//!
//! This is not decoration. The failure mode it guards against is real and it
//! is gradual: someone adds a helpful phrase to one branch, a journalist reads
//! "no match found" as "this video is fake", and a tool built to prevent that
//! reading becomes the thing that produced it. A rule enforced by a test does
//! not erode; a rule written in a style guide does.

use edit_report_core::align::{Correspondence, Cut, Segment, SelfSimilarity, Unmatched};
use edit_report_core::bundle::Transmission;
use edit_report_core::frame::Rect;
use edit_report_core::html;
use edit_report_core::qr::{CodeSighting, ForsheurRef, QrFindings, Recipe, Threshold};
use edit_report_core::report::{
    milestone_limits, universal_limits, Copy as CopyMedia, MediaProfile, Milestone, Original,
    QrSection, Report, Section, SourceKind, Tool, SCHEMA,
};
use edit_report_core::verifier::{ChainState, CryptoState, NotEstablished, VerifierIdentity};

/// Vocabulary that must never appear in a report about a legitimate case.
///
/// Two families, and both matter. The first accuses: it names a crime. The
/// second concludes: it announces a finding the tool is not entitled to reach
/// from any measurement it makes.
const ACCUSATORY: &[&str] = &[
    "tamper",
    "tampered",
    "tampering",
    "forged",
    "forgery",
    "fake",
    "faked",
    "falsif", // falsified / falsification
    "manipulat",
    "doctored",
    "fraud",
    "deepfake",
    "suspicious",
    "not authentic",
    "inauthentic",
    "proves the video",
    "evidence of editing",
    "detected a", // "detected a modification" and friends
];

/// A tool that is not a detector must not describe itself as one.
const DETECTOR_WORDS: &[&str] = &["detector", "detection tool", "fake detector"];

/// §1.1 — no single number that concludes.
const SCORING_WORDS: &[&str] = &[
    "% authentic",
    "authenticity score",
    "confidence score",
    "out of 100",
];

fn qr_none(frames: u32) -> QrFindings {
    QrFindings {
        frames_examined: frames,
        frames_with_code: 0,
        codes: Vec::new(),
    }
}

fn qr_hit(short_id: &str, host: &str, frames: u32) -> QrFindings {
    let payload = format!("{host}/v/{short_id}");
    QrFindings {
        frames_examined: frames,
        frames_with_code: frames,
        codes: vec![CodeSighting {
            reference: ForsheurRef::parse(&payload),
            payload,
            frames,
            first_frame: 0,
            first_pts_us: 0,
            last_frame: frames as u64 * 30,
            last_pts_us: 0,
            recipe: Recipe {
                region: Rect::new(0, 0, 95, 95),
                upscale: 4,
                threshold: Threshold::Local { block: 48, bias: 6 },
                quiet_zone_px: 16,
            },
        }],
    }
}

fn build(
    milestone: Milestone,
    crypto: CryptoState,
    qr: QrSection,
    copy: Option<CopyMedia>,
) -> Report {
    let mut limits = universal_limits();
    limits.extend(milestone_limits(milestone));
    let reason = "this build stops before comparison";
    Report {
        schema: SCHEMA.to_string(),
        generated_at: "2026-09-10T12:00:00Z".to_string(),
        tool: Tool {
            name: "edit-report".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            decoder: "ffmpeg 8.1".into(),
        },
        milestone,
        original: Original {
            source_kind: SourceKind::BundleZip,
            label: "3gvqtL7Y7HZL".into(),
            session_id: "d3153f66-fb43-4663-b3dc-0a867b66312e".into(),
            short_id: Some("3gvqtL7Y7HZL".into()),
            encryption: "none".into(),
            transmission: Transmission::Clear,
            chunk_count: 19,
            capture_start_us: Some(1_788_432_242_208_262),
            capture_end_us: Some(1_788_432_327_208_262),
            crypto,
            media: vec![MediaProfile {
                width: 720,
                height: 1280,
                frame_count: 2576,
                duration_us: 85_868_333,
                nominal_fps: Some(29.998),
                has_audio: true,
                stream_id: Some("video.back.h264_fmp4.v1".into()),
                bars_removed: None,
            }],
        },
        copy,
        qr,
        correspondence: Section::not_performed(reason),
        cuts: Section::not_performed(reason),
        image_differences: Section::not_performed(reason),
        limits,
    }
}

fn verified() -> CryptoState {
    CryptoState {
        chain: ChainState::Verified {
            chunks_verified: 19,
            notes: 1,
        },
        transcript: Some("VERDICT: PASS — 19 chunk(s) fully verified, 1 note(s)".into()),
        verifier: Some(VerifierIdentity {
            path: None,
            sha256: Some("2bad1c66".into()),
            version: Some("1.0.0".into()),
        }),
        checks: Vec::new(),
    }
}

/// A recording that does not look like itself elsewhere — the ordinary case.
const SIM: SelfSimilarity = SelfSimilarity {
    pairs_examined: 800,
    pairs_within_threshold: 12,
    pairs_within_segment_gate: 2,
};

/// A near-static shot, where the offsets are weakly determined.
const SIM_STATIC: SelfSimilarity = SelfSimilarity {
    pairs_examined: 812,
    pairs_within_threshold: 292,
    pairs_within_segment_gate: 144,
};

fn seg(cs: i64, ce: i64, os: i64, frames: usize, dist: f32) -> Segment {
    Segment {
        copy_start_us: cs,
        copy_end_us: ce,
        original_start_us: os,
        original_end_us: os + (ce - cs),
        offset_us: os - cs,
        frames_agreeing: frames,
        mean_distance: dist,
    }
}

fn with_correspondence(mut r: Report, corr: Correspondence) -> Report {
    r.milestone = Milestone::Correspondence;
    r.limits = {
        let mut l = universal_limits();
        l.extend(milestone_limits(Milestone::Correspondence));
        l
    };
    r.cuts = Section::Performed(corr.cuts.clone());
    r.correspondence = Section::Performed(corr);
    r
}

/// Every legitimate situation this build can render.
fn legitimate_cases() -> Vec<(&'static str, Report)> {
    let copy = Some(CopyMedia {
        file_name: Some("reposted_clip.mp4".into()),
        media: Some(MediaProfile {
            width: 480,
            height: 480,
            frame_count: 300,
            duration_us: 12_000_000,
            nominal_fps: Some(25.0),
            has_audio: false,
            stream_id: None,
            bars_removed: None,
        }),
        bars_removed: None,
        bars_description: None,
    });
    vec![
        (
            "verified original, burn-in read in both",
            build(
                Milestone::OriginalOnly,
                verified(),
                QrSection {
                    original: qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 24),
                    copy: Some(qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 24)),
                    observations: vec![
                        "Both videos carry the same reference. That is what a copy of this \
                         recording looks like; it is not by itself evidence that they share \
                         content, which only comparing the pictures can establish."
                            .into(),
                    ],
                },
                copy.clone(),
            ),
        ),
        (
            "copy cropped — no code readable in it at all",
            build(
                Milestone::OriginalOnly,
                verified(),
                QrSection {
                    original: qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 24),
                    copy: Some(qr_none(24)),
                    observations: vec![
                        "No code was read in the video under comparison. The burn-in sits at the \
                         top of the frame, where a crop or a subtitle bar removes it, so this \
                         says nothing about where the video came from."
                            .into(),
                    ],
                },
                copy.clone(),
            ),
        ),
        (
            "copy carries a reference to a different recording",
            build(
                Milestone::OriginalOnly,
                verified(),
                QrSection {
                    original: qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 24),
                    copy: Some(qr_hit("olLp8YUjH1kH", "preprod.forsheur.com", 24)),
                    observations: vec![
                        "The video under comparison carries a reference to a different recording \
                         than the bundle supplied. Whether the two share any picture is a \
                         separate question, answered by comparing them and not by the code."
                            .into(),
                    ],
                },
                copy.clone(),
            ),
        ),
        (
            "two different references in one video",
            build(
                Milestone::OriginalOnly,
                verified(),
                QrSection {
                    original: qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 24),
                    copy: Some(QrFindings {
                        frames_examined: 24,
                        frames_with_code: 24,
                        codes: {
                            let mut v = qr_hit("3gvqtL7Y7HZL", "preprod.forsheur.com", 14).codes;
                            v.extend(qr_hit("olLp8YUjH1kH", "preprod.forsheur.com", 10).codes);
                            v
                        },
                    }),
                    observations: vec![
                        "Two different references appear in the same video, in different frames. \
                         A recording assembled from more than one source looks like this, and so \
                         does a screen showing one recording while another is filmed."
                            .into(),
                    ],
                },
                copy.clone(),
            ),
        ),
        (
            "chain could not be established — no Python",
            build(
                Milestone::OriginalOnly,
                CryptoState::not_established(NotEstablished::NoPythonInterpreter {
                    detail: "python3 not on PATH".into(),
                }),
                QrSection {
                    original: qr_none(24),
                    copy: None,
                    observations: Vec::new(),
                },
                None,
            ),
        ),
        (
            "chain could not be established — browser build",
            build(
                Milestone::OriginalOnly,
                CryptoState::not_established(NotEstablished::NoSubprocessInThisBuild),
                QrSection {
                    original: qr_hit("3gvqtL7Y7HZL", "forsheur.com", 24),
                    copy: None,
                    observations: Vec::new(),
                },
                None,
            ),
        ),
        ("encrypted bundle, no key supplied", {
            let mut r = build(
                Milestone::OriginalOnly,
                verified(),
                QrSection {
                    original: qr_none(0),
                    copy: None,
                    observations: Vec::new(),
                },
                None,
            );
            r.original.encryption = "aes-gcm-256".into();
            r.original.media.clear();
            r
        }),
    ]
}

/// Milestone-2 situations: a real correspondence, in each of the shapes an
/// ordinary edited video produces.
fn comparison_cases() -> Vec<(&'static str, Report)> {
    let base = legitimate_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("verified"))
        .map(|(_, r)| r)
        .unwrap();

    let clean = Correspondence {
        segments: vec![seg(0, 85_000_000, 0, 240, 2.1)],
        unmatched: Vec::new(),
        cuts: Vec::new(),
        copy_frames_examined: 240,
        copy_frames_matched: 238,
        copy_frames_low_variance: 2,
        threshold_used: 22,
        original_self_similarity: SIM,
    };

    let montage_segments = vec![
        seg(0, 8_000_000, 5_000_000, 40, 3.0),
        seg(8_100_000, 16_000_000, 40_000_000, 40, 3.4),
        seg(16_100_000, 24_000_000, 70_000_000, 40, 2.8),
    ];
    let montage = Correspondence {
        cuts: vec![
            Cut {
                copy_at_us: 8_050_000,
                original_left_us: 13_000_000,
                original_resumed_us: 40_000_000,
            },
            Cut {
                copy_at_us: 16_050_000,
                original_left_us: 48_000_000,
                original_resumed_us: 70_000_000,
            },
        ],
        segments: montage_segments,
        unmatched: Vec::new(),
        copy_frames_examined: 120,
        copy_frames_matched: 120,
        copy_frames_low_variance: 0,
        threshold_used: 22,
        original_self_similarity: SIM,
    };

    let spliced = Correspondence {
        segments: vec![
            seg(0, 6_000_000, 0, 30, 2.5),
            seg(12_000_000, 18_000_000, 6_000_000, 30, 2.6),
        ],
        unmatched: vec![Unmatched {
            copy_start_us: 6_000_000,
            copy_end_us: 12_000_000,
            frames_examined: 30,
        }],
        cuts: vec![Cut {
            copy_at_us: 9_000_000,
            original_left_us: 6_000_000,
            original_resumed_us: 6_000_000,
        }],
        copy_frames_examined: 90,
        copy_frames_matched: 60,
        copy_frames_low_variance: 0,
        threshold_used: 22,
        original_self_similarity: SIM,
    };

    let reordered = Correspondence {
        segments: vec![
            seg(0, 5_000_000, 40_000_000, 25, 2.2),
            seg(5_100_000, 10_000_000, 5_000_000, 25, 2.4),
        ],
        cuts: vec![Cut {
            copy_at_us: 5_050_000,
            original_left_us: 45_000_000,
            original_resumed_us: 5_000_000,
        }],
        unmatched: Vec::new(),
        copy_frames_examined: 50,
        copy_frames_matched: 50,
        copy_frames_low_variance: 0,
        threshold_used: 22,
        original_self_similarity: SIM,
    };

    let nothing = Correspondence {
        segments: Vec::new(),
        unmatched: vec![Unmatched {
            copy_start_us: 0,
            copy_end_us: 30_000_000,
            frames_examined: 200,
        }],
        cuts: Vec::new(),
        copy_frames_examined: 200,
        copy_frames_matched: 0,
        copy_frames_low_variance: 14,
        threshold_used: 22,
        original_self_similarity: SIM,
    };

    vec![
        (
            "identical copy, one continuous segment",
            with_correspondence(base.clone(), clean),
        ),
        (
            "montage of three segments, two cuts",
            with_correspondence(base.clone(), montage),
        ),
        (
            "foreign material spliced in the middle",
            with_correspondence(base.clone(), spliced),
        ),
        (
            "segments re-ordered, a cut going backwards",
            with_correspondence(base.clone(), reordered),
        ),
        (
            "no correspondence established at all",
            with_correspondence(base, nothing),
        ),
    ]
}

#[test]
fn no_comparison_result_produces_accusatory_language() {
    // The milestone-2 wording is where the risk concentrates: cuts, holes and
    // "no correspondence" are all easy to phrase as findings against a video,
    // and all three are what an ordinary edit produces.
    for (name, report) in comparison_cases() {
        let html = html::render(&report).to_lowercase();
        let json = report.to_json().to_lowercase();
        for (surface, text) in [("html", &html), ("json", &json)] {
            for word in ACCUSATORY {
                assert!(
                    !text.contains(word),
                    "case {name:?}: the {surface} report says {word:?}"
                );
            }
        }
    }
}

#[test]
fn a_self_similar_original_carries_its_caveat_into_the_report() {
    // The limit that decides what the timestamps are worth. A near-static shot
    // matches itself at other moments, and the report must say so rather than
    // present displaced offsets as measurements.
    let (_, mut report) = comparison_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("montage"))
        .unwrap();
    if let Section::Performed(c) = &mut report.correspondence {
        c.original_self_similarity = SIM_STATIC;
    }
    let html = html::render(&report);
    assert!(
        html.contains("looks like itself at other moments"),
        "caveat missing"
    );
    assert!(html.contains("much more firmly than establishing WHERE"));
    // And it still accuses nobody.
    let lower = html.to_lowercase();
    for word in ACCUSATORY {
        assert!(
            !lower.contains(word),
            "self-similarity caveat says {word:?}"
        );
    }
}

#[test]
fn an_ordinary_original_carries_no_such_caveat() {
    let (_, report) = comparison_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("montage"))
        .unwrap();
    assert!(!html::render(&report).contains("looks like itself"));
}

#[test]
fn cuts_are_never_presented_as_irregular() {
    let (_, report) = comparison_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("montage"))
        .unwrap();
    let html = html::render(&report);
    assert!(
        html.contains("is not itself irregular"),
        "the standing note on cuts is gone"
    );
    // The timestamps must be there in BOTH videos — §6.4.
    assert!(html.contains("Left the original at"));
    assert!(html.contains("Resumed at"));
}

#[test]
fn a_hole_in_the_correspondence_is_phrased_as_absence() {
    let (_, report) = comparison_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("foreign material"))
        .unwrap();
    let html = html::render(&report);
    assert!(html.contains("no correspondence established"));
    assert!(
        html.contains("is not evidence that anything was altered"),
        "the standing note on unmatched stretches is gone"
    );
}

#[test]
fn no_correspondence_at_all_lists_the_innocent_explanations_first() {
    // §1.2: it must be structurally impossible to read a low match as proof of
    // falsification. The wording carries the alternatives with it.
    let (_, report) = comparison_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("no correspondence"))
        .unwrap();
    let html = html::render(&report);
    assert!(html.contains("No correspondence was established"));
    assert!(html.contains("crop"));
    assert!(html.contains("re-encode"));
    assert!(html.contains("not a finding about the copy"));
}

#[test]
fn a_comparison_report_still_carries_no_score() {
    for (name, report) in comparison_cases() {
        let json = report.to_json().to_lowercase();
        for banned in [
            "\"score\"",
            "\"confidence\"",
            "\"percent\"",
            "\"authenticity\"",
            "\"rating\"",
        ] {
            assert!(!json.contains(banned), "case {name:?} contains {banned}");
        }
        let html = html::render(&report).to_lowercase();
        for banned in SCORING_WORDS {
            assert!(!html.contains(banned), "case {name:?} contains {banned:?}");
        }
    }
}

#[test]
fn every_comparison_case_renders_a_self_contained_document() {
    for (name, report) in comparison_cases() {
        let html = html::render(&report);
        for forbidden in [
            "<script", "<img", "<link", "<iframe", "@import", "src=", "href=",
        ] {
            assert!(
                !html.contains(forbidden),
                "case {name:?} contains {forbidden:?}"
            );
        }
    }
}

#[test]
fn no_legitimate_case_produces_accusatory_language() {
    for (name, report) in legitimate_cases() {
        let html = html::render(&report).to_lowercase();
        let json = report.to_json().to_lowercase();
        for surface in [("html", &html), ("json", &json)] {
            for word in ACCUSATORY {
                assert!(
                    !surface.1.contains(word),
                    "case {name:?}: the {} report contains the accusatory word {word:?}",
                    surface.0
                );
            }
        }
    }
}

#[test]
fn the_tool_never_calls_itself_a_detector() {
    for (name, report) in legitimate_cases() {
        let html = html::render(&report).to_lowercase();
        for word in DETECTOR_WORDS {
            assert!(
                !html.contains(word),
                "case {name:?}: report calls itself a {word:?}"
            );
        }
    }
}

#[test]
fn no_report_offers_a_single_number_that_concludes() {
    for (name, report) in legitimate_cases() {
        let html = html::render(&report).to_lowercase();
        for word in SCORING_WORDS {
            assert!(
                !html.contains(word),
                "case {name:?}: report contains {word:?}"
            );
        }
    }
}

#[test]
fn a_negative_result_is_phrased_as_absence_of_a_reading() {
    // §1.2: it must be structurally impossible to read a low match as proof of
    // falsification. The wording for "nothing found" is fixed here.
    let (_, report) = legitimate_cases()
        .into_iter()
        .find(|(n, _)| n.starts_with("copy cropped"))
        .unwrap();
    let html = html::render(&report);
    assert!(html.contains("No code was read"));
    assert!(html.contains("says nothing about"));
}

#[test]
fn the_limits_section_is_present_in_every_single_case() {
    for (name, report) in legitimate_cases() {
        assert!(!report.limits.is_empty(), "case {name:?} has no limits");
        let html = html::render(&report);
        assert!(
            html.contains("Limits"),
            "case {name:?}: no limits section rendered"
        );
        assert!(
            html.contains("does not shrink when the news is good"),
            "case {name:?}: the limits section lost its standing note"
        );
    }
}

#[test]
fn a_failed_chain_stops_the_comparison_and_says_only_that() {
    let mut r = build(
        Milestone::OriginalOnly,
        CryptoState {
            chain: ChainState::Failed {
                failed_checks: 2,
                notes: 0,
            },
            transcript: Some("VERDICT: FAIL — 2 check(s) failed".into()),
            verifier: None,
            checks: Vec::new(),
        },
        QrSection {
            original: qr_none(24),
            copy: None,
            observations: Vec::new(),
        },
        None,
    );
    r.original.label = "broken".into();
    let html = html::render(&r);
    // It reports a failure of the BUNDLE's own checks — the one place the word
    // is earned, because the reference verifier said it.
    assert!(html.contains("could NOT be cryptographically verified"));
    assert!(html.contains("No comparison was performed"));
    // And still says nothing about anybody having done anything.
    let lower = html.to_lowercase();
    for word in ACCUSATORY {
        assert!(
            !lower.contains(word),
            "a failed chain must not accuse: {word:?}"
        );
    }
}

#[test]
fn every_case_renders_a_self_contained_document() {
    // No external resource, in any case, ever: loading nothing is what makes
    // the no-network promise checkable in a browser's network tab, and a
    // report that fetched a font would break it after the fact.
    //
    // What is banned is the machinery that fetches, not the letters h-t-t-p:
    // the verifier's own transcript is quoted verbatim and prints URLs — the
    // Roughtime key list, Google's attestation roots — and stripping those
    // would mean altering the very text §6 requires be reproduced unaltered.
    for (name, report) in legitimate_cases() {
        let html = html::render(&report);
        for forbidden in [
            "<script", "<img", "<link", "<iframe", "<object", "<embed", "@import", "url(", "src=",
            "srcset=", "href=",
        ] {
            assert!(
                !html.contains(forbidden),
                "case {name:?}: rendered report contains {forbidden:?}"
            );
        }
    }
}

#[test]
fn the_verifier_note_tells_the_reader_how_to_check_the_verifier() {
    // The verifier runs from inside the bundle it vouches for. Naming its
    // digest is only useful if the reader is also told what to compare it
    // against, and that the ordinary case -- an older bundle carrying an older
    // verifier -- is not a finding. This sentence is the only place the report
    // says so, so it is pinned here rather than left to drift.
    let (_, report) = legitimate_cases().into_iter().next().unwrap();
    let html = html::render(&report);
    assert!(html.contains("2bad1c66"), "the digest is not named");
    assert!(html.contains("1.0.0"), "the version is not named");
    assert!(
        html.contains("Forsheur/verify-bundle"),
        "the reader is not told where an independent copy lives"
    );
    assert!(
        html.contains("no second implementation"),
        "the report stopped saying it re-implements nothing"
    );
    // And the escape hatch stays: naming a location must not make the document
    // fetch one. Covered generally elsewhere; asserted here because this is
    // the sentence that introduced a URL into the report.
    for forbidden in ["<script", "<img", "<link", "src=", "href="] {
        assert!(
            !html.contains(forbidden),
            "became a live resource: {forbidden:?}"
        );
    }
}

#[test]
fn a_bundle_whose_verifier_predates_versioning_still_renders() {
    // A bundle made before the verifier declared a version is an ordinary
    // bundle, not a suspect one. The note must degrade to naming the digest
    // alone rather than printing an empty version or refusing to render.
    let (_, mut report) = legitimate_cases().into_iter().next().unwrap();
    if let Some(v) = report.original.crypto.verifier.as_mut() {
        v.version = None;
    }
    let html = html::render(&report);
    assert!(html.contains("2bad1c66"), "the digest is not named");
    assert!(
        !html.contains("version <code></code>"),
        "empty version rendered"
    );
}

#[test]
fn a_quoted_transcript_containing_a_url_is_still_self_contained() {
    // Guards the exemption above: a URL printed by the verifier survives into
    // the report as text, and still loads nothing.
    let (_, mut report) = legitimate_cases().into_iter().next().unwrap();
    report.original.crypto.transcript = Some(
        "  ✓ published at https://github.com/cloudflare/roughtime/blob/master/ecosystem.json\n VERDICT: PASS"
            .to_string(),
    );
    let html = html::render(&report);
    assert!(
        html.contains("github.com/cloudflare/roughtime"),
        "transcript was altered"
    );
    for forbidden in ["<script", "<img", "<link", "src=", "href="] {
        assert!(
            !html.contains(forbidden),
            "quoted URL became a live resource: {forbidden:?}"
        );
    }
}
