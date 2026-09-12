//! The browser façade: a flat C interface over `edit-report-core`.
//!
//! ## Why no wasm-bindgen
//!
//! This tool is meant to be audited, and generated glue is code a reader has
//! to trust without having written it. The whole surface the browser needs is
//! below — allocate a buffer, push a frame, ask for the report — and the
//! JavaScript side of it is sixty lines in `web/edit-report.js` that anyone
//! can read. It also means the build is `cargo build --target
//! wasm32-unknown-unknown` and nothing else: no toolchain to install, no
//! version of a code generator to pin.
//!
//! ## The boundary this file does NOT cross
//!
//! No decoding happens here, and none happens in the core. The browser decodes
//! with WebCodecs, which already handles every codec a platform re-encodes
//! into, and hands luma planes over. That is the same rule the native binary
//! follows with ffmpeg, and it is why both can exist over one core.
//!
//! ## Lifetime
//!
//! Single-threaded by construction — wasm without threads — so the session is
//! a thread-local and the functions below are ordered: `reset`, then any
//! number of `push`, then `finish`. Calling them out of order gives an empty
//! report rather than nonsense.

use edit_report_core::bitrow;
use edit_report_core::declared::{self, Declared, OriginalFrame, Tuning};
use edit_report_core::edit_page::{self, PageInputs};
use edit_report_core::fingerprint::fingerprint;
use edit_report_core::frame::LumaFrame;
use std::cell::RefCell;

#[derive(Default)]
struct Session {
    width: u32,
    /// Kept for the report's own record of what geometry the original had.
    #[allow(dead_code)]
    height: u32,
    /// Ratio between the supplied file's width and the original's, so the
    /// strip reader knows how the geometry was scaled.
    copy_scale: f32,
    originals: Vec<OriginalFrame>,
    copies: Vec<Declared>,
    copy_frames: usize,
    copy_duration_us: i64,
    short_id: String,
    chain_verdict: String,
    chain_passed: bool,
    original_label: String,
    copy_label: String,
    original_src: String,
    copy_src: String,
    /// The last string handed back, kept alive while JS copies it out.
    out: String,
}

thread_local! {
    static S: RefCell<Session> = RefCell::new(Session::default());
}

// ── Memory ────────────────────────────────────────────────────────────────
// JS writes into these before every call that takes bytes. Freeing is the
// caller's job, and the caller is the JS in web/ that allocated it.

#[no_mangle]
pub extern "C" fn er_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` must come from `er_alloc` with the same `len`, and must not be used
/// afterwards.
#[no_mangle]
pub unsafe extern "C" fn er_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(Vec::from_raw_parts(ptr, 0, len));
    }
}

unsafe fn borrow_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        return "";
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
}

// ── Session ───────────────────────────────────────────────────────────────

/// Begin a comparison. `width`/`height` are the ORIGINAL's frame size; the
/// copy's is given per frame, since a rescaled copy is the ordinary case.
#[no_mangle]
pub extern "C" fn er_reset(width: u32, height: u32) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        *s = Session {
            width,
            height,
            copy_scale: 1.0,
            chain_passed: true,
            ..Session::default()
        };
    });
}

/// # Safety
/// `ptr`/`len` must describe valid UTF-8 readable for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn er_set_text(field: u32, ptr: *const u8, len: usize) {
    let v = borrow_str(ptr, len).to_string();
    S.with(|s| {
        let mut s = s.borrow_mut();
        match field {
            0 => s.short_id = v,
            1 => s.chain_verdict = v,
            2 => s.original_label = v,
            3 => s.copy_label = v,
            4 => s.original_src = v,
            5 => s.copy_src = v,
            _ => {}
        }
    });
}

#[no_mangle]
pub extern "C" fn er_set_chain_passed(passed: u32) {
    S.with(|s| s.borrow_mut().chain_passed = passed != 0);
}

/// Total duration of the supplied file in microseconds, for the report's own
/// statement of how much of it was read.
#[no_mangle]
pub extern "C" fn er_set_copy_duration_us(us: f64) {
    S.with(|s| s.borrow_mut().copy_duration_us = us as i64);
}

/// One frame of the ORIGINAL. `ptr` is `width * height` luma bytes.
///
/// # Safety
/// `ptr` must point at `width * height` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn er_push_original(
    ptr: *const u8,
    width: u32,
    height: u32,
    index: f64,
    t_us: f64,
) -> u32 {
    let Some(frame) = frame_from(ptr, width, height, index, t_us) else {
        return 0;
    };
    let reading = bitrow::read(&frame, 1.0).ok();
    let fp = fingerprint(&frame);
    S.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(k) = reading {
            s.originals.push(OriginalFrame {
                counter: k.counter,
                index: index as u64,
                t_us: t_us as i64,
                fp,
            });
        }
    });
    reading.is_some() as u32
}

/// One frame of the SUPPLIED file. Returns 1 if its strip could be read.
///
/// # Safety
/// `ptr` must point at `width * height` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn er_push_copy(
    ptr: *const u8,
    width: u32,
    height: u32,
    index: f64,
    t_us: f64,
) -> u32 {
    let Some(frame) = frame_from(ptr, width, height, index, t_us) else {
        return 0;
    };
    // The strip's geometry is expressed in the original's pixels; a rescaled
    // copy is told apart by the ratio, not guessed from the frame.
    let scale = S.with(|s| {
        let s = s.borrow();
        if s.width > 0 {
            width as f32 / s.width as f32
        } else {
            1.0
        }
    });
    let reading = bitrow::read(&frame, scale).ok();
    let fp = fingerprint(&frame);
    S.with(|s| {
        let mut s = s.borrow_mut();
        s.copy_scale = scale;
        s.copy_frames += 1;
        s.copies.push(Declared {
            copy_index: index as u64,
            copy_t_us: t_us as i64,
            counter: reading.map(|k| k.counter),
            tag: reading.map(|k| k.session_tag),
            fp,
        });
    });
    reading.is_some() as u32
}

unsafe fn frame_from(
    ptr: *const u8,
    width: u32,
    height: u32,
    index: f64,
    t_us: f64,
) -> Option<LumaFrame> {
    let n = (width as usize).checked_mul(height as usize)?;
    if ptr.is_null() || n == 0 {
        return None;
    }
    let data = std::slice::from_raw_parts(ptr, n).to_vec();
    LumaFrame::new(width, height, data, index as u64, t_us as i64).ok()
}

/// Build the report and keep it. Returns its length in bytes; the pointer
/// comes from `er_out_ptr`.
#[no_mangle]
pub extern "C" fn er_finish(fps: f64) -> usize {
    S.with(|s| {
        let mut s = s.borrow_mut();
        let tuning = Tuning {
            expected_tag: (!s.short_id.is_empty())
                .then(|| bitrow::session_tag(Some(&s.short_id))),
            ..Tuning::every_frame(if fps > 0.0 { fps } else { 30.0 })
        };
        let r = declared::build_with(&s.copies, &s.originals, tuning);
        let seconds = (s.copy_duration_us.max(0) as f64) / 1e6;
        let html = edit_page::fragment(
            &r,
            &PageInputs {
                short_id: &s.short_id,
                chain_verdict: &s.chain_verdict,
                chain_passed: s.chain_passed,
                original_src: &s.original_src,
                copy_src: &s.copy_src,
                original_label: &s.original_label,
                copy_label: &s.copy_label,
                frames_read: s.copy_frames,
                seconds_examined: seconds,
            },
        );
        s.out = html;
        s.out.len()
    })
}

#[no_mangle]
pub extern "C" fn er_out_ptr() -> *const u8 {
    S.with(|s| s.borrow().out.as_ptr())
}

/// Frames of the original whose strip could be read. Zero means the original
/// predates the strip and the declaration path has nothing to work with —
/// worth saying out loud rather than showing an empty report.
#[no_mangle]
pub extern "C" fn er_original_declaring() -> usize {
    S.with(|s| s.borrow().originals.len())
}

// ── The page's own style and behaviour ────────────────────────────────────
// Handed over rather than duplicated in the host page, so there is one source
// of truth for both builds.

#[no_mangle]
pub extern "C" fn er_style() -> usize {
    S.with(|s| {
        let mut s = s.borrow_mut();
        s.out = edit_page::STYLE.to_string();
        s.out.len()
    })
}

#[no_mangle]
pub extern "C" fn er_script() -> usize {
    S.with(|s| {
        let mut s = s.borrow_mut();
        s.out = edit_page::SCRIPT.to_string();
        s.out.len()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma(w: u32, h: u32, v: u8) -> Vec<u8> {
        vec![v; (w * h) as usize]
    }

    #[test]
    fn a_session_with_no_frames_still_renders_a_report() {
        er_reset(720, 1280);
        let n = er_finish(30.0);
        assert!(n > 0);
        let s = unsafe { std::slice::from_raw_parts(er_out_ptr(), n) };
        let html = std::str::from_utf8(s).unwrap();
        assert!(html.contains("No correspondence was established"));
        // Never an accusation, even with nothing to go on.
        assert!(!html.to_lowercase().contains("falsif"));
    }

    #[test]
    fn frames_without_a_strip_are_counted_but_never_invented() {
        er_reset(64, 64);
        let f = luma(64, 64, 128);
        let got = unsafe { er_push_copy(f.as_ptr(), 64, 64, 0.0, 0.0) };
        assert_eq!(got, 0, "a flat grey frame carries no strip");
        assert_eq!(er_original_declaring(), 0);
    }

    #[test]
    fn the_style_and_the_behaviour_come_from_the_core() {
        let n = er_style();
        let s = unsafe { std::slice::from_raw_parts(er_out_ptr(), n) };
        assert!(std::str::from_utf8(s).unwrap().contains("<style>"));
        let n = er_script();
        let s = unsafe { std::slice::from_raw_parts(er_out_ptr(), n) };
        let js = std::str::from_utf8(s).unwrap();
        assert!(js.contains("editReportLink"));
        assert!(!js.contains("<script"), "the host page supplies the tags");
    }

    #[test]
    fn a_short_id_becomes_the_signature_the_report_expects() {
        er_reset(720, 1280);
        let id = "lzvYrVDnmEMQ";
        unsafe { er_set_text(0, id.as_ptr(), id.len()) };
        let n = er_finish(30.0);
        let s = unsafe { std::slice::from_raw_parts(er_out_ptr(), n) };
        assert!(std::str::from_utf8(s).unwrap().contains("0xD53D"));
    }
}
