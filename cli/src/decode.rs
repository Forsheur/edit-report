//! The decoder adapter: containers and codecs live here, never in the core.
//!
//! This build shells out to ffmpeg. That is a deliberate choice for the native
//! binary and it is documented in the README rather than hidden:
//!
//!   * the ORIGINAL is always H.264 in fragmented MP4 — that is what a Forsheur
//!     phone writes — so a single-codec decoder would do;
//!   * the COPY is whatever a platform re-encoded it into: H.264, HEVC, VP9,
//!     AV1, in any container. A tool that could not open the copy would be
//!     useless for the job it exists to do.
//!
//! The browser build makes the opposite choice for the same reason: WebCodecs
//! is already in the browser, already handles every codec the platforms use,
//! and costs nothing to ship. Neither decoder is in the core, which is why
//! both can exist.

use edit_report_core::frame::LumaFrame;
use edit_report_core::report::MediaProfile;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum DecodeError {
    ToolMissing(String),
    Failed(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::ToolMissing(t) => write!(
                f,
                "{t} was not found. This build decodes video with ffmpeg; install it, or use \
                 the browser version which decodes with the browser's own WebCodecs"
            ),
            DecodeError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn tool(name: &str) -> Result<Command, DecodeError> {
    let probe = Command::new(name).arg("-version").output();
    match probe {
        Ok(o) if o.status.success() => Ok(Command::new(name)),
        _ => Err(DecodeError::ToolMissing(name.to_string())),
    }
}

/// Which ffmpeg produced the frames, for the report's `tool.decoder` field.
pub fn decoder_name() -> String {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
        .unwrap_or_else(|| "ffmpeg (version unknown)".to_string())
}

/// Read a video's shape without decoding it.
pub fn probe(path: &Path, stream_id: Option<String>) -> Result<MediaProfile, DecodeError> {
    let out = tool("ffprobe")?
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,nb_read_frames,avg_frame_rate",
            "-show_entries",
            "format=duration",
            "-count_frames",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| DecodeError::Failed(format!("ffprobe could not run: {e}")))?;

    if !out.status.success() {
        return Err(DecodeError::Failed(format!(
            "ffprobe refused {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| DecodeError::Failed(format!("ffprobe output unreadable: {e}")))?;
    let s = v["streams"].get(0).ok_or_else(|| {
        DecodeError::Failed(format!("{} carries no video stream", path.display()))
    })?;

    let width = s["width"].as_u64().unwrap_or(0) as u32;
    let height = s["height"].as_u64().unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err(DecodeError::Failed(format!(
            "{} has no usable frame size",
            path.display()
        )));
    }
    let frame_count = s["nb_read_frames"]
        .as_str()
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    let duration_us = v["format"]["duration"]
        .as_str()
        .and_then(|x| x.parse::<f64>().ok())
        .map(|d| (d * 1e6) as i64)
        .unwrap_or(0);
    // `avg_frame_rate`, not `r_frame_rate`: the phone writes variable-duration
    // samples, so the nominal rate is a rounding of reality and is labelled as
    // nominal wherever it is shown.
    let nominal_fps = s["avg_frame_rate"].as_str().and_then(parse_ratio);

    let has_audio = has_audio_stream(path).unwrap_or(false);

    Ok(MediaProfile {
        width,
        height,
        frame_count,
        duration_us,
        nominal_fps,
        has_audio,
        stream_id,
        bars_removed: None,
    })
}

fn parse_ratio(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    if d == 0.0 {
        return None;
    }
    Some(n / d)
}

fn has_audio_stream(path: &Path) -> Result<bool, DecodeError> {
    let out = tool("ffprobe")?
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|e| DecodeError::Failed(format!("ffprobe could not run: {e}")))?;
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// The most positions `decode_positions` will name individually.
///
/// Each becomes an `eq(n,X)` term in one `select` expression, and ffmpeg's
/// expression parser falls over well before a few hundred of them — measured:
/// 24 terms is fine, 240 fails with "Cannot allocate memory" and no frames come
/// back. Anything larger goes through [`decode_stride`], which says the same
/// thing in one term.
const MAX_NAMED_POSITIONS: usize = 48;

/// Decode exactly the frames at `positions` (0-based, ascending) as 8-bit luma.
///
/// One ffmpeg pass with a `select` expression rather than one seek per frame:
/// seeking into fragmented MP4 lands on key frames, and a QR sweep that
/// silently examined the nearest key frame instead of the frame it asked for
/// would report positions it never looked at.
pub fn decode_positions(
    path: &Path,
    profile: &MediaProfile,
    positions: &[u64],
) -> Result<Vec<LumaFrame>, DecodeError> {
    if positions.is_empty() {
        return Ok(Vec::new());
    }
    if positions.len() > MAX_NAMED_POSITIONS {
        return Err(DecodeError::Failed(format!(
            "{} positions named individually; use decode_stride past {MAX_NAMED_POSITIONS}",
            positions.len()
        )));
    }
    let select = positions
        .iter()
        .map(|n| format!("eq(n\\,{n})"))
        .collect::<Vec<_>>()
        .join("+");

    let mut child = tool("ffmpeg")?
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select='{select}'"),
            "-vsync",
            "0",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DecodeError::Failed(format!("ffmpeg could not run: {e}")))?;

    let frame_bytes = (profile.width as usize) * (profile.height as usize);
    let mut buf = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout was piped")
        .read_to_end(&mut buf)
        .map_err(|e| DecodeError::Failed(format!("reading decoded frames failed: {e}")))?;

    let status = child
        .wait()
        .map_err(|e| DecodeError::Failed(e.to_string()))?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut err);
        }
        return Err(DecodeError::Failed(format!(
            "ffmpeg failed on {}: {}",
            path.display(),
            err.trim()
        )));
    }

    // Pair each decoded frame with the position that asked for it. If ffmpeg
    // returned fewer than we asked for — a truncated file, a position past the
    // end — the surplus positions are simply not reported, and the count that
    // reaches the report is the count actually examined.
    let got = buf.len() / frame_bytes.max(1);
    let mut frames = Vec::with_capacity(got);
    for (i, pos) in positions.iter().take(got).enumerate() {
        let start = i * frame_bytes;
        let data = buf[start..start + frame_bytes].to_vec();
        let pts_us = pts_of(profile, *pos);
        frames.push(
            LumaFrame::new(profile.width, profile.height, data, *pos, pts_us)
                .map_err(|e| DecodeError::Failed(e.to_string()))?,
        );
    }
    Ok(frames)
}

/// How many frames per second of content each side is sampled at.
///
/// **The two sides must be sampled at the same rate in TIME, not at the same
/// count.** Sampling a fixed number of frames from each gives a long original a
/// coarse step and a short copy a fine one; the copy's instants then fall
/// between the original's, and for a camera that is moving the picture in
/// between is a different picture. Measured: with the original stepped at 0.5 s
/// and the copy at 0.067 s, a twenty-second extract matched 41 frames of 300
/// and put them all on one original frame. At a common rate the same extract is
/// one clean segment.
///
/// Four per second is the compromise. Finer costs time proportionally; coarser
/// starts losing fast pans, where a quarter of a second is already a different
/// view.
pub const SAMPLES_PER_SECOND: f64 = 4.0;

/// Frame stride that yields `rate` samples per second of content for this
/// video. The whole point is that the SAME rate is used on both sides.
pub fn stride_for_rate(profile: &MediaProfile, rate: f64) -> u64 {
    match profile.nominal_fps.filter(|f| *f > 0.0) {
        Some(fps) if rate > 0.0 => ((fps / rate).round() as u64).max(1),
        _ => 1,
    }
}

/// Stream every `stride`-th frame through `f`, one at a time.
///
/// Streaming rather than collecting, because the alternative does not fit: at
/// four samples per second a seven-minute 720p recording is 1 700 frames, and
/// holding them is a gigabyte and a half. The brief requires never loading a
/// whole video into memory and this is where that is honoured — the caller
/// keeps 64 bits per frame, not a megabyte.
pub fn decode_stream<F>(
    path: &Path,
    profile: &MediaProfile,
    stride: u64,
    mut f: F,
) -> Result<usize, DecodeError>
where
    F: FnMut(LumaFrame),
{
    let stride = stride.max(1);
    let mut child = tool("ffmpeg")?
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select='not(mod(n\\,{stride}))'"),
            "-vsync",
            "0",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DecodeError::Failed(format!("ffmpeg could not run: {e}")))?;

    let frame_bytes = (profile.width as usize) * (profile.height as usize);
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut buf = vec![0u8; frame_bytes];
    let mut n = 0usize;
    loop {
        match read_exact_or_eof(&mut stdout, &mut buf)? {
            false => break,
            true => {
                let index = (n as u64) * stride;
                let frame = LumaFrame::new(
                    profile.width,
                    profile.height,
                    buf.clone(),
                    index,
                    pts_of(profile, index),
                )
                .map_err(|e| DecodeError::Failed(e.to_string()))?;
                f(frame);
                n += 1;
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| DecodeError::Failed(e.to_string()))?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut err);
        }
        return Err(DecodeError::Failed(format!(
            "ffmpeg failed on {}: {}",
            path.display(),
            err.trim()
        )));
    }
    Ok(n)
}

/// Fill `buf` completely, or report a clean end of stream.
///
/// A partial frame at the end is discarded rather than padded: half a frame
/// fingerprints to something, and that something would be a measurement of
/// nothing.
fn read_exact_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<bool, DecodeError> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => return Ok(false),
            Ok(k) => filled += k,
            Err(e) => return Err(DecodeError::Failed(format!("reading frames failed: {e}"))),
        }
    }
    Ok(true)
}

/// Decode every `stride`-th frame: 0, stride, 2·stride, …
///
/// One `select` term whatever the count, so it scales where a list of named
/// positions does not. Collects, so it is for bounded work — bar detection on a
/// couple of dozen frames — and [`decode_stream`] is for the rest.
pub fn decode_stride(
    path: &Path,
    profile: &MediaProfile,
    stride: u64,
) -> Result<Vec<LumaFrame>, DecodeError> {
    let stride = stride.max(1);
    let mut child = tool("ffmpeg")?
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select='not(mod(n\\,{stride}))'"),
            "-vsync",
            "0",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DecodeError::Failed(format!("ffmpeg could not run: {e}")))?;

    let frame_bytes = (profile.width as usize) * (profile.height as usize);
    let mut buf = Vec::new();
    child
        .stdout
        .take()
        .expect("stdout was piped")
        .read_to_end(&mut buf)
        .map_err(|e| DecodeError::Failed(format!("reading decoded frames failed: {e}")))?;

    let status = child
        .wait()
        .map_err(|e| DecodeError::Failed(e.to_string()))?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = s.read_to_string(&mut err);
        }
        return Err(DecodeError::Failed(format!(
            "ffmpeg failed on {}: {}",
            path.display(),
            err.trim()
        )));
    }

    let got = buf.len() / frame_bytes.max(1);
    let mut frames = Vec::with_capacity(got);
    for i in 0..got {
        let index = (i as u64) * stride;
        let start = i * frame_bytes;
        let data = buf[start..start + frame_bytes].to_vec();
        frames.push(
            LumaFrame::new(
                profile.width,
                profile.height,
                data,
                index,
                pts_of(profile, index),
            )
            .map_err(|e| DecodeError::Failed(e.to_string()))?,
        );
    }
    Ok(frames)
}

/// Presentation time of frame `index`, from the nominal rate.
///
/// The phone writes variable-duration samples, so this is an approximation —
/// and a good enough one: measured against the fixture recording it drifts by a
/// few milliseconds over 85 seconds, while alignment groups offsets in buckets
/// a quarter of a second wide. If a source ever drifts enough to matter, the
/// symptom is a segment splitting in two at a constant offset, which is visible
/// in the report rather than silent.
fn pts_of(profile: &MediaProfile, index: u64) -> i64 {
    match profile.nominal_fps.filter(|f| *f > 0.0) {
        Some(fps) => ((index as f64) / fps * 1e6) as i64,
        None => 0,
    }
}

/// Decode roughly `count` frames spread across the whole video, as 8-bit luma.
///
/// Spread rather than the first N: a copy that was trimmed at the head, or
/// whose opening seconds are a title card, would otherwise be judged on the
/// part of it that says least.
pub fn decode_spread(
    path: &Path,
    profile: &MediaProfile,
    count: u32,
) -> Result<Vec<LumaFrame>, DecodeError> {
    if profile.frame_count == 0 || count == 0 {
        return Ok(Vec::new());
    }
    let stride = (profile.frame_count / count.max(1) as u64).max(1);
    decode_stride(path, profile, stride)
}
