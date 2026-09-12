//! Running the bundle's own verifier and quoting it.
//!
//! This module contains no cryptography and never will. It finds a Python 3,
//! runs `verify_bundle.py` from inside the bundle being examined, and hands
//! back what it said. Every failure mode here produces `NotEstablished` — a
//! third state — never `Failed`: a missing interpreter is a fact about this
//! computer, and turning it into a verdict about a recording would be the
//! single most damaging bug this tool could ship.

use edit_report_core::verifier::{parse_verifier_json, CryptoState, NotEstablished};
use std::path::Path;
use std::process::Command;

/// What the verifier run produced, beyond the state itself.
pub struct VerifierRun {
    pub state: CryptoState,
    /// `extracted/` was populated, so decodable media is on disk.
    pub extracted: bool,
}

/// Interpreters to try, in order. `python3` first because that is what the
/// bundle's own README tells the user to type.
const INTERPRETERS: &[&str] = &["python3", "python", "py"];

pub fn find_interpreter(explicit: Option<&str>) -> Option<String> {
    let candidates: Vec<&str> = match explicit {
        Some(p) => vec![p],
        None => INTERPRETERS.to_vec(),
    };
    for c in candidates {
        // `--version` rather than `which`: it proves the thing runs, which is
        // the actual question, and it works the same on every platform.
        if let Ok(out) = Command::new(c).arg("--version").output() {
            if out.status.success() {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// Run `verify_bundle.py` inside `bundle_root`.
///
/// `extract` asks it to also write the verified media streams to
/// `extracted/`. We take the media from that run rather than decoding the
/// payloads ourselves, and the reason is not convenience: `--extract` writes
/// only payloads that passed verification, so anything this tool later
/// compares against is bytes the reference verifier vouched for.
pub fn run(bundle_root: &Path, interpreter: Option<&str>, extract: bool) -> VerifierRun {
    let script = bundle_root.join("verify_bundle.py");
    if !script.is_file() {
        return VerifierRun {
            state: CryptoState::not_established(NotEstablished::VerifierMissingFromBundle),
            extracted: false,
        };
    }

    let Some(py) = find_interpreter(interpreter) else {
        let detail = match interpreter {
            Some(p) => format!("{p} did not run"),
            None => format!("tried {}", INTERPRETERS.join(", ")),
        };
        return VerifierRun {
            state: CryptoState::not_established(NotEstablished::NoPythonInterpreter { detail }),
            extracted: false,
        };
    };

    let json_path = bundle_root.join("edit-report-verify.json");

    // A bundle whose embedded verifier predates `--json` is reported as a
    // chain this run could not establish, and stops there. Reading the verdict
    // out of the text instead was considered and dropped: a summary recovered
    // by matching English sentences is a dependency on wording that nobody
    // versions and that the next edit to the verifier would silently break.
    let out = match invoke(&py, &script, bundle_root, extract, Some(&json_path)) {
        Ok(o) if rejected_the_flag(&o) => {
            return VerifierRun {
                state: CryptoState::not_established(NotEstablished::VerifierTooOld),
                extracted: false,
            }
        }
        Ok(o) => o,
        Err(e) => return started_badly(&py, e, false),
    };

    let extracted = extract && bundle_root.join("extracted").is_dir();
    let mut transcript = String::from_utf8_lossy(&out.stdout).into_owned();
    let errs = String::from_utf8_lossy(&out.stderr);
    if !errs.trim().is_empty() {
        transcript.push_str("\n--- stderr ---\n");
        transcript.push_str(&errs);
    }

    let json_bytes = match std::fs::read(&json_path) {
        Ok(b) => b,
        Err(e) => {
            return VerifierRun {
                state: CryptoState::not_established(NotEstablished::VerifierOutputUnreadable {
                    detail: format!("no machine-readable summary was written: {e}"),
                }),
                extracted,
            }
        }
    };
    let _ = std::fs::remove_file(&json_path);

    let state = match parse_verifier_json(&json_bytes)
        .and_then(|j| CryptoState::from_verifier(j, Some(transcript.clone())))
    {
        Ok(s) => s,
        Err(e) => CryptoState::not_established(NotEstablished::VerifierOutputUnreadable {
            detail: e.to_string(),
        }),
    };

    VerifierRun { state, extracted }
}

fn started_badly(py: &str, e: std::io::Error, extracted: bool) -> VerifierRun {
    VerifierRun {
        state: CryptoState::not_established(NotEstablished::VerifierOutputUnreadable {
            detail: format!("{py} could not be started: {e}"),
        }),
        extracted,
    }
}

/// argparse refuses an unknown option by name, on stderr. The message is what
/// we test rather than the exit status, because some launchers swallow the
/// status and a false "it ran fine" here would be read as a verified chain.
fn rejected_the_flag(out: &std::process::Output) -> bool {
    let err = String::from_utf8_lossy(&out.stderr);
    err.contains("unrecognized arguments: --json")
        || err.contains("unrecognized arguments") && err.contains("--json")
}

fn invoke(
    py: &str,
    script: &Path,
    bundle_root: &Path,
    extract: bool,
    json_path: Option<&Path>,
) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new(py);
    cmd.arg(script).arg(".");
    if let Some(j) = json_path {
        cmd.arg("--json").arg(j);
    }
    cmd.current_dir(bundle_root)
        // The verifier prompts for a key on encrypted bundles. We never pass
        // one and never want a prompt: an unattended run must not block on a
        // hidden `getpass` with no terminal to type into.
        .stdin(std::process::Stdio::null())
        // Its output is UTF-8 with box-drawing and check marks. Windows would
        // otherwise encode it in the console code page and mangle the
        // transcript we are about to quote verbatim.
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1");
    if extract {
        cmd.arg("--extract");
    }
    cmd.output()
}
