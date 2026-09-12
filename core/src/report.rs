//! The report: one model, two renderings (JSON here, HTML in `html.rs`).
//!
//! ## The shape is fixed now, before it is filled
//!
//! Sections 3 to 5 — correspondence, cuts, image differences — are present in
//! this schema from the first release and carry an explicit `not_performed`
//! state until the milestone that fills them lands. Omitting them until then
//! would mean the schema changes shape twice more, and a consumer that pinned
//! version 1 would break on a report that is still version 1.
//!
//! It also keeps the reader honest: a section that says "no comparison was
//! performed" cannot be mistaken for a comparison that found nothing.
//!
//! ## Two claims, never one
//!
//! `original.crypto` answers "is the original established?". `copy` answers
//! "how does this other video relate to it?". They are separate objects with
//! separate wording, and no field anywhere multiplies them into a single
//! judgement. There is no score in this schema, and adding one later would be
//! a breaking change to more than the version number.

use crate::align::{Correspondence, Cut};
use crate::bundle::Transmission;
use crate::normalize::Bars;
use crate::qr::QrFindings;
use crate::verifier::CryptoState;
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "forsheur-edit-report/1";

/// Which parts of the pipeline this build actually runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Milestone {
    /// Original ingested, chain state established, burn-in read. No comparison.
    OriginalOnly,
    /// …plus normalisation, temporal alignment, correspondence and cuts.
    Correspondence,
    /// …plus classified image differences.
    ImageDifference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub version: String,
    /// How the frames reaching the core were decoded. Named because a reader
    /// comparing two runs of this tool needs to know whether the pixels came
    /// from the same place.
    pub decoder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    BundleZip,
    BundleDirectory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaProfile {
    pub width: u32,
    pub height: u32,
    pub frame_count: u64,
    pub duration_us: i64,
    pub nominal_fps: Option<f64>,
    pub has_audio: bool,
    /// Which camera stream this is, for a dual-camera recording.
    pub stream_id: Option<String>,
    /// Bars found and removed before fingerprinting, when this is a side that
    /// was normalised.
    #[serde(default)]
    pub bars_removed: Option<Bars>,
}

/// The recording the bundle holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Original {
    pub source_kind: SourceKind,
    /// Short id, or the session UUID when there is none.
    pub label: String,
    pub session_id: String,
    pub short_id: Option<String>,
    /// The scheme exactly as the manifest spells it. Kept verbatim next to the
    /// interpretation below, so a reader can check the reading against the
    /// value it came from.
    pub encryption: String,
    /// What that scheme means for this archive: clear, transport-sealed, or
    /// end-to-end. See [`crate::bundle::Transmission`].
    pub transmission: Transmission,
    pub chunk_count: usize,
    /// Capture window declared by the phone and covered by its signature.
    pub capture_start_us: Option<i64>,
    pub capture_end_us: Option<i64>,
    /// Section 1 of the report.
    pub crypto: CryptoState,
    #[serde(default)]
    pub media: Vec<MediaProfile>,
}

/// The video being compared against the original. Absent at `OriginalOnly`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Copy {
    pub file_name: Option<String>,
    pub media: Option<MediaProfile>,
    /// What normalisation was applied before comparing. Reported for both
    /// sides because a reader cannot judge a correspondence without knowing
    /// what was done to the pictures first.
    pub bars_removed: Option<Bars>,
    pub bars_description: Option<String>,
}

/// A section that a later milestone fills.
///
/// `NotPerformed` carries the reason so nobody has to guess whether a section
/// is empty because the build does not do it yet, because the chain failed, or
/// because there was nothing to compare.
/// Adjacently tagged rather than internally tagged: an internal tag can only
/// be applied to a payload that is itself a map, and two of these carry a list.
/// The shape a reader sees is `{"state": "...", "value": ...}`, uniform across
/// every section whatever it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum Section<T> {
    NotPerformed(String),
    Performed(T),
}

impl<T> Section<T> {
    pub fn not_performed(reason: impl Into<String>) -> Self {
        Section::NotPerformed(reason.into())
    }
}

/// Where the QR sweep looked. The burn-in lives in both videos, and a report
/// that did not say which one it read would be unreadable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QrSection {
    /// Findings in the bundle's own media.
    pub original: QrFindings,
    /// Findings in the video under comparison, when there is one.
    pub copy: Option<QrFindings>,
    /// Plain-language readings of what the sweep means. Never a verdict:
    /// each is a statement about codes, not about authenticity.
    pub observations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub generated_at: String,
    pub tool: Tool,
    pub milestone: Milestone,
    /// §6.1
    pub original: Original,
    pub copy: Option<Copy>,
    /// §6.2
    pub qr: QrSection,
    /// §6.3 — intervals of the copy mapped onto intervals of the original.
    pub correspondence: Section<Correspondence>,
    /// §6.4 — discontinuities in that mapping. Derived from the
    /// correspondence, never searched for separately.
    pub cuts: Section<Vec<Cut>>,
    /// §6.5 — classified into exactly three states, never two.
    pub image_differences: Section<serde_json::Value>,
    /// §6.6 — mandatory, never folded away, never empty.
    pub limits: Vec<String>,
}

impl Report {
    pub fn to_json(&self) -> String {
        // Pretty and stable: a report that a journalist may diff against
        // another run should differ only where the finding differs.
        serde_json::to_string_pretty(self).expect("report is always serialisable")
    }
}

/// The limits that apply to every report this tool produces.
///
/// Not assembled per run and not conditional on the outcome: a limits section
/// that shrinks when the news is good teaches a reader to stop reading it.
/// Milestone-specific limits are appended to these, never substituted for them.
pub fn universal_limits() -> Vec<String> {
    vec![
        "This tool reports on provenance and integrity. It establishes nothing about whether \
         what the picture shows is true. A scene can be staged, a witness can lie, a camera can \
         be pointed away from what matters — none of that is visible to any of the checks here."
            .to_string(),
        "A difference between two videos is not an accusation. Re-encoding, cropping, subtitling \
         and format conversion all produce differences, and they are what normally happens to a \
         video that circulates. This report describes differences; it does not penalise them."
            .to_string(),
        "The burned-in overlay — timestamp, frame counter, coordinates, QR code — is pixels, not \
         signatures. Anyone can render the same text into their own footage. It is used here to \
         propose an alignment, and never on its own to conclude that two videos are related."
            .to_string(),
        "The cryptographic chain is checked by the bundle's own verifier, verify_bundle.py, which \
         this tool runs and quotes. This tool contains no independent implementation of it, on \
         purpose: two implementations would eventually disagree, and then neither could be \
         believed."
            .to_string(),
        "Absence of a finding is not a finding. A section reporting that no code was read, or \
         that no correspondence was established, describes what this run could measure — not what \
         the video is."
            .to_string(),
    ]
}

/// Limits that are true only while the build stops at `milestone`.
pub fn milestone_limits(milestone: Milestone) -> Vec<String> {
    match milestone {
        Milestone::OriginalOnly => vec![
            "This build performs no comparison at all. It reads the original, establishes its \
             cryptographic state, and reads whatever codes are burned into the picture. Nothing \
             in this report says anything about any other video."
                .to_string(),
        ],
        Milestone::Correspondence => vec![
            "This build maps segments and locates cuts. It does not yet classify differences \
             within the image, so a segment reported as corresponding may still differ inside \
             the frame in ways this run did not examine."
                .to_string(),
        ],
        Milestone::ImageDifference => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::Transmission;
    use crate::qr::QrFindings;
    use crate::verifier::{ChainState, CryptoState, NotEstablished};

    fn empty_qr() -> QrFindings {
        QrFindings {
            frames_examined: 0,
            frames_with_code: 0,
            codes: Vec::new(),
        }
    }

    fn sample(milestone: Milestone) -> Report {
        let mut limits = universal_limits();
        limits.extend(milestone_limits(milestone));
        Report {
            schema: SCHEMA.to_string(),
            generated_at: "2026-09-10T12:00:00Z".to_string(),
            tool: Tool {
                name: "edit-report".into(),
                version: "0.1.0".into(),
                decoder: "ffmpeg".into(),
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
                capture_start_us: Some(1788432242208262),
                capture_end_us: Some(1788432327208262),
                crypto: CryptoState {
                    chain: ChainState::Verified {
                        chunks_verified: 19,
                        notes: 1,
                    },
                    transcript: Some("VERDICT: PASS".into()),
                    verifier: None,
                    checks: Vec::new(),
                },
                media: Vec::new(),
            },
            copy: None,
            qr: QrSection {
                original: empty_qr(),
                copy: None,
                observations: Vec::new(),
            },
            correspondence: Section::not_performed("this build stops before comparison"),
            cuts: Section::not_performed("this build stops before comparison"),
            image_differences: Section::not_performed("this build stops before comparison"),
            limits,
        }
    }

    #[test]
    fn every_report_carries_limits() {
        for m in [
            Milestone::OriginalOnly,
            Milestone::Correspondence,
            Milestone::ImageDifference,
        ] {
            let mut l = universal_limits();
            l.extend(milestone_limits(m));
            assert!(l.len() >= universal_limits().len());
            assert!(!l.is_empty(), "limits must never be empty");
        }
    }

    #[test]
    fn universal_limits_are_the_same_whatever_happened() {
        // The point of the section is that it does not move. If a future edit
        // makes it conditional, this fails.
        assert_eq!(universal_limits(), universal_limits());
        assert_eq!(universal_limits().len(), 5);
    }

    #[test]
    fn unperformed_sections_are_present_and_say_why() {
        let r = sample(Milestone::OriginalOnly);
        let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
        for key in ["correspondence", "cuts", "image_differences"] {
            assert_eq!(v[key]["state"], "not_performed", "{key}");
            assert!(v[key]["value"].as_str().unwrap().len() > 10, "{key}");
        }
    }

    #[test]
    fn the_schema_carries_no_score_no_percentage_no_traffic_light() {
        // §1.1 of the brief, enforced rather than promised. A single number
        // that concludes is a threshold, and a published threshold is a recipe.
        let json = sample(Milestone::ImageDifference).to_json().to_lowercase();
        for banned in [
            "\"score\"",
            "\"confidence\"",
            "\"percent\"",
            "\"authenticity\"",
            "\"rating\"",
            "\"grade\"",
            "\"probability\"",
            "\"verdict\":",
        ] {
            assert!(
                !json.contains(banned),
                "report schema must not contain {banned}"
            );
        }
    }

    #[test]
    fn the_two_claims_stay_in_separate_objects() {
        let v: serde_json::Value =
            serde_json::from_str(&sample(Milestone::OriginalOnly).to_json()).unwrap();
        assert!(v["original"]["crypto"].is_object());
        assert!(v["copy"].is_null());
        // Nothing about the copy may live inside the original's crypto object.
        let crypto = serde_json::to_string(&v["original"]["crypto"])
            .unwrap()
            .to_lowercase();
        assert!(!crypto.contains("copy"));
    }

    #[test]
    fn a_report_round_trips_through_its_own_schema() {
        let r = sample(Milestone::OriginalOnly);
        let back: Report = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn a_chain_that_was_not_established_still_produces_a_whole_report() {
        let mut r = sample(Milestone::OriginalOnly);
        r.original.crypto = CryptoState::not_established(NotEstablished::NoSubprocessInThisBuild);
        let v: serde_json::Value = serde_json::from_str(&r.to_json()).unwrap();
        assert_eq!(v["original"]["crypto"]["chain"]["state"], "not_established");
        assert_eq!(
            v["original"]["crypto"]["chain"]["reason"],
            "no_subprocess_in_this_build"
        );
        assert!(!r.limits.is_empty());
    }
}
