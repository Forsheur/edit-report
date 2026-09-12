//! The cryptographic state of the original — quoted, never recomputed.
//!
//! ## Why there is no crypto in this file
//!
//! `server/verifier/verify_bundle.py` ships inside every evidence bundle, runs
//! on the Python standard library alone, and is the reference implementation.
//! This tool does not re-verify anything it does. That is a deliberate refusal,
//! not a shortcut:
//!
//!   * a second implementation of a verification chain is a second thing that
//!     can be wrong, and the day the two disagree, neither is believable —
//!     which destroys exactly the confidence both exist to produce;
//!   * the divergence would not appear at the port. It would appear later, when
//!     the reference gains a chunk format and the copy does not.
//!
//! So the binary runs the bundle's own verifier as a process and quotes it. The
//! browser build cannot run Python at all and therefore reports the chain as
//! **not established here**, which is a third state and not a failure.
//!
//! ## The three states
//!
//! `Verified` / `Failed` / `NotEstablished`. The third is mandatory and will be
//! common: no Python, a browser, an encrypted bundle nobody supplied a key for.
//! It must never be collapsed into `Failed` — "we could not check" and "the
//! check did not pass" are different statements about the world, and only one
//! of them is about the recording.

use serde::{Deserialize, Serialize};

/// The schema `verify_bundle.py --json` writes.
pub const SUPPORTED_SCHEMA: &str = "forsheur-verify-bundle/1";

/// One check the reference verifier ran, in its own words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// `ok`, `fail` or `note`.
    pub status: String,
    pub message: String,
}

/// Which copy of the reference verifier produced this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierIdentity {
    pub path: Option<String>,
    pub sha256: Option<String>,
}

/// `verify_bundle.py --json`, parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierJson {
    pub schema: String,
    pub verdict: String,
    pub failed_checks: u32,
    pub notes: u32,
    pub chunks_verified: u32,
    #[serde(default)]
    pub checks: Vec<Check>,
    pub verifier: Option<VerifierIdentity>,
}

/// Why the chain could not be established.
///
/// Each variant names a circumstance of the *run*, never a property of the
/// recording. Nothing here is evidence about the video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum NotEstablished {
    /// No Python 3 on this machine.
    NoPythonInterpreter { detail: String },
    /// This build cannot run a subprocess at all — the browser.
    NoSubprocessInThisBuild,
    /// The bundle carried no `verify_bundle.py`.
    VerifierMissingFromBundle,
    /// The verifier ran but its output could not be read.
    VerifierOutputUnreadable { detail: String },
    /// The user asked to skip it.
    SkippedByOperator,
    /// The bundle carries a verifier older than the machine-readable summary
    /// this tool reads. Not parsed out of its prose instead: a summary read by
    /// pattern-matching English is a dependency on wording nobody versioned.
    VerifierTooOld,
}

impl NotEstablished {
    /// One sentence for the report. Every one of these describes this run and
    /// stops there — no phrasing may leave room to read a missing interpreter
    /// as a finding about the recording.
    pub fn sentence(&self) -> String {
        match self {
            NotEstablished::NoPythonInterpreter { detail } => format!(
                "The cryptographic chain was not checked here: no Python 3 interpreter was \
                 available to run the bundle's own verifier ({detail}). This says nothing \
                 about the recording."
            ),
            NotEstablished::NoSubprocessInThisBuild => {
                "The cryptographic chain was not checked here: this build runs in a browser \
                 and cannot execute the bundle's Python verifier. Run `python3 \
                 verify_bundle.py .` inside the bundle yourself, or use the native binary. \
                 This says nothing about the recording."
                    .to_string()
            }
            NotEstablished::VerifierMissingFromBundle => {
                "The cryptographic chain was not checked here: this archive contains no \
                 verify_bundle.py to run. This says nothing about the recording."
                    .to_string()
            }
            NotEstablished::VerifierOutputUnreadable { detail } => format!(
                "The cryptographic chain was not checked here: the verifier ran but its \
                 output could not be read ({detail}). This says nothing about the recording."
            ),
            NotEstablished::VerifierTooOld => {
                "The cryptographic chain was not checked here: this bundle carries a verifier \
                 older than the machine-readable summary this tool reads. Run `python3 \
                 verify_bundle.py .` inside the bundle yourself, or fetch the bundle again \
                 from a current server. This says nothing about the recording."
                    .to_string()
            }
            NotEstablished::SkippedByOperator => {
                "The cryptographic chain was not checked here: verification was skipped at \
                 the operator's request. This says nothing about the recording."
                    .to_string()
            }
        }
    }
}

/// The first of the report's two separate claims: is the ORIGINAL cryptographically
/// established?
///
/// It never mentions the copy, and the copy's section never mentions this one.
/// Fusing them is what produces a sentence like "this video failed verification"
/// out of two facts that mean something else entirely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ChainState {
    Verified { chunks_verified: u32, notes: u32 },
    Failed { failed_checks: u32, notes: u32 },
    NotEstablished(NotEstablished),
}

/// Everything the report says about the original's cryptography.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoState {
    pub chain: ChainState,
    /// The reference verifier's complete output, byte for byte.
    ///
    /// Quoted rather than summarised because §6 of the brief requires it and
    /// because summarising is where a tool starts putting words in another
    /// tool's mouth. Rendered in the HTML report as preformatted text.
    pub transcript: Option<String>,
    pub verifier: Option<VerifierIdentity>,
    /// Every check the verifier reported, for readers who want the list
    /// without reading the transcript.
    #[serde(default)]
    pub checks: Vec<Check>,
}

impl CryptoState {
    pub fn not_established(reason: NotEstablished) -> Self {
        CryptoState {
            chain: ChainState::NotEstablished(reason),
            transcript: None,
            verifier: None,
            checks: Vec::new(),
        }
    }

    /// Build from `verify_bundle.py --json` plus the transcript it printed.
    pub fn from_verifier(
        json: VerifierJson,
        transcript: Option<String>,
    ) -> Result<Self, VerifierError> {
        if json.schema != SUPPORTED_SCHEMA {
            return Err(VerifierError::UnsupportedSchema {
                found: json.schema,
                supported: SUPPORTED_SCHEMA,
            });
        }
        // Trust the counter, not the word: `verdict` is a label the verifier
        // derives from `failed_checks`, and if the two ever disagreed we would
        // rather report the stricter of the pair than pick the friendlier one.
        let chain = if json.verdict == "pass" && json.failed_checks == 0 {
            ChainState::Verified {
                chunks_verified: json.chunks_verified,
                notes: json.notes,
            }
        } else {
            ChainState::Failed {
                failed_checks: json.failed_checks.max(1),
                notes: json.notes,
            }
        };
        Ok(CryptoState {
            chain,
            transcript,
            verifier: json.verifier,
            checks: json.checks,
        })
    }

    /// True only when the chain actually verified. Gates the comparison: §5 of
    /// the brief — comparing against an original that was never established
    /// means nothing, so the tool stops instead.
    pub fn is_verified(&self) -> bool {
        matches!(self.chain, ChainState::Verified { .. })
    }

    /// The headline sentence for this section.
    pub fn headline(&self) -> String {
        match &self.chain {
            ChainState::Verified {
                chunks_verified,
                notes,
            } => {
                let n = if *notes == 0 {
                    String::new()
                } else if *notes == 1 {
                    ", with 1 note".to_string()
                } else {
                    format!(", with {notes} notes")
                };
                format!(
                    "The original is cryptographically verified: {chunks_verified} sealed \
                     segment(s) checked against the device signature, the notary chain and \
                     its anchor{n}."
                )
            }
            ChainState::Failed { failed_checks, .. } => format!(
                "The original could NOT be cryptographically verified: {failed_checks} \
                 check(s) failed. No comparison was performed — comparing against an \
                 unestablished original would mean nothing."
            ),
            ChainState::NotEstablished(r) => r.sentence(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierError {
    UnsupportedSchema {
        found: String,
        supported: &'static str,
    },
    Malformed(String),
}

impl core::fmt::Display for VerifierError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VerifierError::UnsupportedSchema { found, supported } => write!(
                f,
                "verify_bundle.py reported schema {found:?}; this build reads {supported:?}"
            ),
            VerifierError::Malformed(e) => write!(f, "verifier output could not be parsed: {e}"),
        }
    }
}

impl std::error::Error for VerifierError {}

pub fn parse_verifier_json(bytes: &[u8]) -> Result<VerifierJson, VerifierError> {
    serde_json::from_slice(bytes).map_err(|e| VerifierError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass_json() -> VerifierJson {
        VerifierJson {
            schema: SUPPORTED_SCHEMA.to_string(),
            verdict: "pass".into(),
            failed_checks: 0,
            notes: 1,
            chunks_verified: 19,
            checks: vec![Check {
                status: "ok".into(),
                message: "sha256(payload) == h_received".into(),
            }],
            verifier: Some(VerifierIdentity {
                path: None,
                sha256: Some("2bad1c66".into()),
            }),
        }
    }

    #[test]
    fn a_passing_run_verifies_and_keeps_the_transcript_verbatim() {
        let t = "Session d3153f66…\n  ✓ sha256(payload) == h_received\nVERDICT: PASS — 19 chunk(s)"
            .to_string();
        let s = CryptoState::from_verifier(pass_json(), Some(t.clone())).unwrap();
        assert!(s.is_verified());
        assert_eq!(s.transcript.as_deref(), Some(t.as_str()));
        assert_eq!(s.checks.len(), 1);
    }

    #[test]
    fn a_failing_run_blocks_the_comparison() {
        let j = VerifierJson {
            verdict: "fail".into(),
            failed_checks: 3,
            ..pass_json()
        };
        let s = CryptoState::from_verifier(j, None).unwrap();
        assert!(!s.is_verified());
        assert!(s.headline().contains("No comparison was performed"));
    }

    #[test]
    fn a_pass_label_over_a_nonzero_failure_count_is_read_as_a_failure() {
        // The two disagree only if something is wrong. Take the strict reading.
        let j = VerifierJson {
            verdict: "pass".into(),
            failed_checks: 2,
            ..pass_json()
        };
        let s = CryptoState::from_verifier(j, None).unwrap();
        assert!(!s.is_verified());
    }

    #[test]
    fn an_unknown_schema_is_refused_rather_than_half_read() {
        let j = VerifierJson {
            schema: "forsheur-verify-bundle/9".into(),
            ..pass_json()
        };
        assert!(matches!(
            CryptoState::from_verifier(j, None).unwrap_err(),
            VerifierError::UnsupportedSchema { .. }
        ));
    }

    #[test]
    fn not_established_is_never_a_failure_and_never_accuses_the_recording() {
        for r in [
            NotEstablished::NoPythonInterpreter {
                detail: "python3 not on PATH".into(),
            },
            NotEstablished::NoSubprocessInThisBuild,
            NotEstablished::VerifierMissingFromBundle,
            NotEstablished::VerifierOutputUnreadable {
                detail: "empty".into(),
            },
            NotEstablished::SkippedByOperator,
            NotEstablished::VerifierTooOld,
        ] {
            let s = CryptoState::not_established(r);
            assert!(!s.is_verified());
            assert!(matches!(s.chain, ChainState::NotEstablished(_)));
            let h = s.headline();
            assert!(h.contains("not checked here"), "{h}");
            assert!(h.contains("says nothing about the recording"), "{h}");
            // The word that must never appear about a recording we did not check.
            for banned in ["tampered", "forged", "falsified", "invalid recording"] {
                assert!(!h.to_lowercase().contains(banned), "{h}");
            }
        }
    }
}
