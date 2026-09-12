//! A bundle named by a relative path must verify exactly like an absolute one.
//!
//! This is a regression test for a bug that produced the worst output this
//! tool is capable of. The bundle's verifier runs with its working directory
//! set to the bundle root and is told where to write its summary; when the
//! root was relative, that path resolved against the CHILD's directory, the
//! file landed where neither process looked, and a bundle that verifies
//! perfectly was reported as "the cryptographic chain was not checked here".
//!
//! A fact about a path separator, presented in the section where a reader
//! looks for a fact about the recording. Hence a test, not a comment.

use std::fs;
use std::process::Command;

/// A stand-in for `verify_bundle.py` that takes the same arguments and always
/// passes. The real verifier's behaviour is not what is under test here — the
/// plumbing between the two processes is.
const STUB_VERIFIER: &str = r#"
import argparse, json, os
ap = argparse.ArgumentParser()
ap.add_argument("bundle")
ap.add_argument("--extract", action="store_true")
ap.add_argument("--json")
a = ap.parse_args()
print("Session stub")
print("  ✓ sha256(payload) == h_received")
if a.extract:
    os.makedirs("extracted", exist_ok=True)
if a.json:
    with open(a.json, "w", encoding="utf-8") as f:
        json.dump({
            "schema": "forsheur-verify-bundle/1",
            "verdict": "pass",
            "failed_checks": 0,
            "notes": 0,
            "chunks_verified": 2,
            "checks": [{"status": "ok", "message": "sha256(payload) == h_received"}],
            "verifier": {"path": os.path.abspath(__file__), "sha256": None},
        }, f)
print("VERDICT: PASS — 2 chunk(s) fully verified")
"#;

const MANIFEST: &str = r#"{
  "format": "forsheur-evidence-bundle/1",
  "session": {"session_id": "d3153f66-fb43-4663-b3dc-0a867b66312e",
              "short_id": "3gvqtL7Y7HZL", "encryption": "none", "chunk_count": 2},
  "chunks": [
    {"chunk_id": "01a066de-5620-7a19-8407-0a1eb967768c", "seq": 0, "phone_time_us": 1788432242208262},
    {"chunk_id": "01a066de-6a07-7b9b-8680-2fcd977c239a", "seq": 1, "phone_time_us": 1788432247208262}
  ]
}"#;

fn make_bundle() -> tempfile::TempDir {
    let parent = tempfile::tempdir().expect("temp dir");
    let root = parent.path().join("forsheur-evidence-3gvqtL7Y7HZL");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("manifest.json"), MANIFEST).unwrap();
    fs::write(root.join("verify_bundle.py"), STUB_VERIFIER).unwrap();
    parent
}

fn run_with_bundle_arg(cwd: &std::path::Path, bundle_arg: &str) -> (String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_edit-report"))
        .args(["--bundle", bundle_arg])
        .current_dir(cwd)
        .output()
        .expect("binary runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_relative_bundle_path_verifies_like_an_absolute_one() {
    let parent = make_bundle();
    // The parent of the bundle directory, so the tool is handed a name with no
    // separator at all — the shape a user types when they are already there.
    let workdir = parent.path().parent().unwrap().to_path_buf();
    let rel = parent
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let (_, rel_err) = run_with_bundle_arg(&workdir, &rel);
    let (_, abs_err) = run_with_bundle_arg(&workdir, &parent.path().to_string_lossy());

    for (label, err) in [("relative", &rel_err), ("absolute", &abs_err)] {
        assert!(
            err.contains("cryptographically verified"),
            "{label} path did not verify.\n{err}"
        );
        assert!(
            !err.contains("not checked here"),
            "{label} path reported the chain as unchecked.\n{err}"
        );
    }
}

#[test]
fn a_bundle_named_through_a_dot_path_behaves_the_same() {
    let parent = make_bundle();
    let workdir = parent.path().parent().unwrap().to_path_buf();
    let dotted = format!("./{}", parent.path().file_name().unwrap().to_string_lossy());
    let (_, err) = run_with_bundle_arg(&workdir, &dotted);
    assert!(err.contains("cryptographically verified"), "{err}");
}

#[test]
fn a_path_with_a_space_and_an_accent_is_opened_as_typed() {
    // Windows paths routinely carry both, and an argument that went through a
    // lossy UTF-8 conversion would open a different file than the one named.
    let parent = tempfile::tempdir().unwrap();
    let odd = parent.path().join("dossier accentué é 1");
    let root = odd.join("forsheur-evidence-3gvqtL7Y7HZL");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("manifest.json"), MANIFEST).unwrap();
    fs::write(root.join("verify_bundle.py"), STUB_VERIFIER).unwrap();

    let (_, err) = run_with_bundle_arg(parent.path(), "dossier accentué é 1");
    assert!(err.contains("cryptographically verified"), "{err}");
}

#[test]
fn a_directory_that_is_not_a_bundle_says_so_and_does_not_pretend_to_verify() {
    let d = tempfile::tempdir().unwrap();
    let (_, err) = run_with_bundle_arg(d.path(), ".");
    assert!(err.contains("is this an evidence bundle"), "{err}");
    assert!(!err.contains("cryptographically verified"), "{err}");
}
