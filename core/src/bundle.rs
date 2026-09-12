//! The parts of a Forsheur evidence bundle this tool reads.
//!
//! Only `manifest.json` is modelled here, and only the fields the report
//! quotes. Everything else in the archive — payloads, envelopes, notary
//! proofs, attestation chains — is the verifier's business, not ours: see
//! `verifier.rs` for why we do not touch it.
//!
//! Parsing only, no I/O. The adapter hands over bytes it read from a `.zip` or
//! from an `extracted/` directory; this module never learns which.

use serde::{Deserialize, Serialize};

/// The bundle format this tool understands. Written by
/// `server/src/evidence_bundle.rs`.
pub const SUPPORTED_FORMAT: &str = "forsheur-evidence-bundle/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub generated_at: Option<String>,
    pub session: Session,
    #[serde(default)]
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub short_id: Option<String>,
    /// The raw scheme: `none`, `box-seal-x25519`, or `aes-gcm-256`.
    ///
    /// Do NOT test this against `"none"` to decide whether the payloads are
    /// readable — read [`Session::transmission`] or [`Manifest::transmission`]
    /// instead. See [`Transmission`] for why that distinction is not academic.
    pub encryption: String,
    /// `clear`, `transport` or `e2e`, named by the server so consumers do not
    /// re-derive it. Absent from bundles generated before 2026-09-10, which is
    /// why [`Manifest::transmission`] falls back to the scheme.
    #[serde(default)]
    pub transmission: Option<String>,
    pub chunk_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub chunk_id: String,
    pub seq: i64,
    /// Capture time declared by the phone and covered by its signature.
    pub phone_time_us: i64,
    pub sealed_block: Option<u64>,
}

/// Why a bundle cannot be used as an original.
/// How a recording's payload travelled from the phone to the server, and
/// therefore whether the bytes in this bundle can be read without a key.
///
/// The three modes and their names come from the server, `v2_manifest.rs`
/// (`let transmission = match encryption.as_str()`). They are mirrored here
/// rather than re-invented, and this comment exists because getting it wrong
/// is not hypothetical: the first version of this file asked only whether the
/// scheme was `none`, which quietly filed every transport-sealed recording
/// under "encrypted, cannot be read". That is a false sentence in a report,
/// and it disabled the whole picture path for every recording made after
/// transport sealing went into service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transmission {
    /// `none` — never sealed. Older recordings, before transport sealing
    /// existed.
    Clear,
    /// `box-seal-x25519` — sealed to the PLATFORM's key on the phone, so the
    /// recording was never at rest in cleartext on the device, and opened by
    /// the server on arrival. The payloads in this bundle are the cleartext.
    /// This is a property of the journey, not a lock on the archive.
    Transport,
    /// `aes-gcm-256` — end to end. The DEK is sealed to USERS and the server
    /// holds no key at all, so the payloads here are ciphertext and stay that
    /// way until someone with a key opens them.
    E2e,
}

impl Transmission {
    pub fn from_encryption(scheme: &str) -> Transmission {
        match scheme {
            "none" => Transmission::Clear,
            "box-seal-x25519" => Transmission::Transport,
            // Anything else, including a scheme invented after this build, is
            // treated as end to end. That is the conservative direction for a
            // value we do not recognise — do not try to decode what we cannot
            // name — and it is what the server does with the same match. It is
            // NOT an excuse to file a known transport scheme here.
            _ => Transmission::E2e,
        }
    }

    /// Whether `payload.bin` holds ciphertext, and therefore whether there is
    /// any picture in this bundle to read.
    pub fn payload_is_ciphertext(self) -> bool {
        matches!(self, Transmission::E2e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    Malformed(String),
    /// A format this build was not written against. Refused rather than
    /// guessed at: a tool that half-reads an unknown format produces a report
    /// whose gaps nobody can see.
    UnsupportedFormat {
        found: String,
        supported: &'static str,
    },
}

impl core::fmt::Display for BundleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BundleError::Malformed(e) => write!(f, "manifest.json could not be read: {e}"),
            BundleError::UnsupportedFormat { found, supported } => write!(
                f,
                "this bundle declares format {found:?}; this build reads {supported:?}"
            ),
        }
    }
}

impl std::error::Error for BundleError {}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, BundleError> {
        let m: Manifest =
            serde_json::from_slice(bytes).map_err(|e| BundleError::Malformed(e.to_string()))?;
        if m.format != SUPPORTED_FORMAT {
            return Err(BundleError::UnsupportedFormat {
                found: m.format.clone(),
                supported: SUPPORTED_FORMAT,
            });
        }
        Ok(m)
    }

    /// How this recording travelled. See [`Transmission`].
    ///
    /// Prefers the server's own `transmission` label and falls back to reading
    /// the scheme, so an older bundle keeps working. The two agree by
    /// construction — the server derives its label from the same match — and
    /// where a future bundle uses a word this build does not know, the
    /// fallback answers rather than the unknown word.
    pub fn transmission(&self) -> Transmission {
        match self.session.transmission.as_deref() {
            Some("clear") => Transmission::Clear,
            Some("transport") => Transmission::Transport,
            Some("e2e") => Transmission::E2e,
            _ => Transmission::from_encryption(&self.session.encryption),
        }
    }

    /// How the recording names itself: its short id, or the session UUID when
    /// it never got one.
    pub fn label(&self) -> &str {
        self.session
            .short_id
            .as_deref()
            .unwrap_or(&self.session.session_id)
    }

    /// Capture time of the earliest and latest chunk, in microseconds.
    pub fn capture_span_us(&self) -> Option<(i64, i64)> {
        let min = self.chunks.iter().map(|c| c.phone_time_us).min()?;
        let max = self.chunks.iter().map(|c| c.phone_time_us).max()?;
        Some((min, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r#"{
      "format": "forsheur-evidence-bundle/1",
      "generated_at": "2026-09-10T12:00:00+00:00",
      "session": {
        "session_id": "d3153f66-fb43-4663-b3dc-0a867b66312e",
        "short_id": "3gvqtL7Y7HZL",
        "encryption": "none",
        "chunk_count": 19
      },
      "chunks": [
        {"chunk_id": "01a066de-5620-7a19-8407-0a1eb967768c", "seq": 0, "phone_time_us": 1788432242208262, "sealed_block": 2551, "iv": null},
        {"chunk_id": "01a066de-6a07-7b9b-8680-2fcd977c239a", "seq": 1, "phone_time_us": 1788432247208262, "sealed_block": 2551, "iv": null}
      ]
    }"#;

    #[test]
    fn reads_a_real_manifest() {
        let m = Manifest::parse(REAL.as_bytes()).unwrap();
        assert_eq!(m.label(), "3gvqtL7Y7HZL");
        assert_eq!(m.transmission(), Transmission::Clear);
        assert_eq!(m.chunks.len(), 2);
        assert_eq!(
            m.capture_span_us(),
            Some((1788432242208262, 1788432247208262))
        );
    }

    #[test]
    fn unknown_fields_do_not_break_it() {
        // The server adds keys to the manifest over time. An older build of
        // this tool must keep reading a newer bundle of the same format
        // version, or a journalist's copy stops working the day the server
        // gains a field.
        let s = REAL.replace(r#""format""#, r#""something_new": {"a": 1}, "format""#);
        assert!(Manifest::parse(s.as_bytes()).is_ok());
    }

    #[test]
    fn refuses_a_format_it_was_not_written_against() {
        let s = REAL.replace("forsheur-evidence-bundle/1", "forsheur-evidence-bundle/2");
        assert!(matches!(
            Manifest::parse(s.as_bytes()).unwrap_err(),
            BundleError::UnsupportedFormat { .. }
        ));
    }

    fn with_scheme(scheme: &str) -> Manifest {
        let s = REAL.replace(
            r#""encryption": "none""#,
            &format!(r#""encryption": "{scheme}""#),
        );
        Manifest::parse(s.as_bytes()).unwrap()
    }

    #[test]
    fn the_three_modes_map_the_way_the_server_maps_them() {
        assert_eq!(with_scheme("none").transmission(), Transmission::Clear);
        assert_eq!(
            with_scheme("box-seal-x25519").transmission(),
            Transmission::Transport
        );
        assert_eq!(with_scheme("aes-gcm-256").transmission(), Transmission::E2e);
    }

    #[test]
    fn a_transport_sealed_recording_is_readable() {
        // The regression that produced this test, and it shipped in two places
        // at once. `box-seal-x25519` means the phone sealed the payload to the
        // platform so it was never at rest in cleartext on the device; the
        // server opened it on arrival and the bytes in the bundle ARE the
        // picture. Filing it as encrypted refused to read a recording that
        // reads perfectly, and sent the reader after a key that does not exist
        // for that session.
        assert!(!with_scheme("box-seal-x25519")
            .transmission()
            .payload_is_ciphertext());
    }

    #[test]
    fn only_end_to_end_withholds_the_picture() {
        assert!(!with_scheme("none").transmission().payload_is_ciphertext());
        assert!(!with_scheme("box-seal-x25519")
            .transmission()
            .payload_is_ciphertext());
        assert!(with_scheme("aes-gcm-256")
            .transmission()
            .payload_is_ciphertext());
    }

    #[test]
    fn an_unrecognised_scheme_is_treated_as_end_to_end() {
        // Not a licence to lump known schemes here: this is for a value this
        // build has never heard of, where declining to decode is right.
        let m = with_scheme("something-invented-later");
        assert_eq!(m.transmission(), Transmission::E2e);
        assert!(m.transmission().payload_is_ciphertext());
    }

    #[test]
    fn the_servers_own_label_is_read_when_present() {
        let s = REAL.replace(
            r#""encryption": "none""#,
            r#""encryption": "box-seal-x25519", "transmission": "transport""#,
        );
        assert_eq!(
            Manifest::parse(s.as_bytes()).unwrap().transmission(),
            Transmission::Transport
        );
    }

    #[test]
    fn an_older_bundle_without_the_label_still_reads() {
        // Bundles generated before 2026-09-10 carry no `transmission` field.
        // They must keep working, and keep being classified correctly.
        assert!(!REAL.contains("transmission"));
        assert_eq!(
            with_scheme("box-seal-x25519").transmission(),
            Transmission::Transport
        );
    }

    #[test]
    fn a_label_this_build_does_not_know_falls_back_to_the_scheme() {
        let s = REAL.replace(
            r#""encryption": "none""#,
            r#""encryption": "aes-gcm-256", "transmission": "something-new""#,
        );
        assert_eq!(
            Manifest::parse(s.as_bytes()).unwrap().transmission(),
            Transmission::E2e
        );
    }

    #[test]
    fn falls_back_to_the_uuid_when_there_is_no_short_id() {
        let s = REAL.replace(r#""short_id": "3gvqtL7Y7HZL""#, r#""short_id": null"#);
        let m = Manifest::parse(s.as_bytes()).unwrap();
        assert_eq!(m.label(), "d3153f66-fb43-4663-b3dc-0a867b66312e");
    }
}
