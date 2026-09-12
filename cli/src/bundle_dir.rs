//! Getting an evidence bundle onto disk as a directory.
//!
//! Both inputs are first-class. A `.zip` is what the server hands out; an
//! already-extracted directory is what a user has after running the bundle's
//! own verifier with a decryption key, which is the supported route for an
//! encrypted session (this tool holds no crypto of its own — see
//! `edit_report_core::verifier`).

use std::io;
use std::path::{Path, PathBuf};

/// A bundle laid out as a directory, plus the temporary directory holding it
/// when we extracted one ourselves.
pub struct BundleDir {
    pub root: PathBuf,
    /// Kept alive so the extraction survives as long as the bundle is in use;
    /// dropping it deletes the temporary tree.
    _temp: Option<tempfile::TempDir>,
    pub was_archive: bool,
}

impl BundleDir {
    pub fn path(&self, rel: &str) -> PathBuf {
        // Joined component by component rather than by pushing a string with
        // separators in it: on Windows the caller's `/` would otherwise end up
        // inside a file name.
        rel.split('/')
            .fold(self.root.clone(), |p, part| p.join(part))
    }
}

/// Open `input` as a bundle directory, extracting it first if it is an archive.
pub fn open(input: &Path) -> io::Result<BundleDir> {
    let meta = std::fs::metadata(input)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot read {}: {e}", input.display())))?;

    if meta.is_dir() {
        let root = absolute(locate_manifest(input.to_path_buf())?);
        return Ok(BundleDir {
            root,
            _temp: None,
            was_archive: false,
        });
    }

    let temp = tempfile::Builder::new()
        .prefix("edit-report-bundle-")
        .tempdir()?;
    extract_zip(input, temp.path())?;
    let root = absolute(locate_manifest(temp.path().to_path_buf())?);
    Ok(BundleDir {
        root,
        _temp: Some(temp),
        was_archive: true,
    })
}

/// Resolve the bundle root to an absolute path.
///
/// Not cosmetic, and the bug that produced it is worth naming. The bundle's
/// own verifier is run with its working directory set to this root, and it is
/// told where to write its machine-readable summary. Given a relative root,
/// that path resolved against the CHILD's directory rather than ours: the file
/// landed somewhere neither process looked, and a bundle that verifies
/// perfectly was reported as a chain this tool could not establish.
///
/// So a path separator produced the exact failure this design exists to
/// prevent — a fact about the run, presented where a reader looks for a fact
/// about the recording. Absolute from here on.
///
/// On Windows this also yields a `\\?\` prefixed path, which lifts the
/// 260-character limit as a side effect.
fn absolute(p: PathBuf) -> PathBuf {
    p.canonicalize().unwrap_or(p)
}

/// A bundle's contents sit under one directory named after the recording. Look
/// for `manifest.json` at the given path first, then exactly one level down —
/// so both `unzip`-into-a-folder layouts work without the user having to know
/// which one they have.
fn locate_manifest(start: PathBuf) -> io::Result<PathBuf> {
    if start.join("manifest.json").is_file() {
        return Ok(start);
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&start)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("manifest.json").is_file())
        .collect();
    candidates.sort();
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "no manifest.json in {} or one level below it — is this an evidence bundle?",
                start.display()
            ),
        )),
        n => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{n} bundles found under {}; point at one of them",
                start.display()
            ),
        )),
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> io::Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a readable zip: {e}"),
        )
    })?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("zip entry {i}: {e}"))
        })?;

        // `enclosed_name` rejects absolute paths and `..` traversal. An
        // evidence bundle is a file a journalist received from a source, which
        // is to say from someone who may not be a friend; an archive that
        // writes outside its own directory is the oldest trick there is.
        let Some(rel) = entry.enclosed_name() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("zip entry {i} has an unsafe path and was refused"),
            ));
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = std::io::BufWriter::new(std::fs::File::create(&out)?);
        std::io::copy(&mut entry, &mut w)?;
    }
    Ok(())
}
