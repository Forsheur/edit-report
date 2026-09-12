// The browser build: decode with WebCodecs, measure in wasm, render the same
// report the native binary writes.
//
// The glue is hand-written and short on purpose. `wasm/src/lib.rs` exports a
// flat C interface — allocate, push a frame, ask for the report — so there is
// no generated code between what a reader audits and what runs.

import { demux, decodeEveryFrame } from './mp4.js';

let wasm = null;
let mem = () => new Uint8Array(wasm.memory.buffer);

export async function load(url = './edit_report_wasm.wasm') {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(url), {});
  wasm = instance.exports;
  return wasm;
}

/// Hand a string to wasm. Field ids match `er_set_text` in wasm/src/lib.rs.
function setText(field, s) {
  const bytes = new TextEncoder().encode(s || '');
  const p = wasm.er_alloc(bytes.length || 1);
  mem().set(bytes, p);
  wasm.er_set_text(field, p, bytes.length);
  wasm.er_free(p, bytes.length || 1);
}

/// Read back whatever the last call left in the wasm-side buffer.
function takeOut(len) {
  const p = wasm.er_out_ptr();
  return new TextDecoder().decode(mem().subarray(p, p + len));
}

export function style() { return takeOut(wasm.er_style()); }
export function script() { return takeOut(wasm.er_script()); }

/// Copy a frame's luma plane into wasm memory and push it.
///
/// Three things make this fiddlier than it looks.
///
///  * **No format conversion.** Asking `copyTo` for I420 raised "this pixel
///    format conversion is not supported" on the very first real frame:
///    Chrome converts only between some pairs. So the frame is copied in
///    whatever format it already has, and the luma is taken from that.
///  * **Plane 0 is luma** in every YUV layout WebCodecs produces (I420, I422,
///    I444, NV12 and their alpha variants), so one branch covers all of them.
///    RGBA and BGRA have no luma plane and are converted here instead.
///  * **Stride is not width.** A plane's rows can be padded; read as one block
///    the picture shears, and the burned-in strip is the first thing that
///    would stop reading. Rows are copied one at a time.
async function pushFrame(which, frame, index) {
  const rect = frame.visibleRect
    || { x: 0, y: 0, width: frame.codedWidth, height: frame.codedHeight };
  const w = rect.width, h = rect.height;

  const scratch = new Uint8Array(frame.allocationSize({ rect }));
  const layout = await frame.copyTo(scratch, { rect });

  const p = wasm.er_alloc(w * h);
  const dst = mem();
  const fmt = frame.format || '';
  if (fmt.startsWith('RGBA') || fmt.startsWith('BGRA') || fmt.startsWith('RGBX') || fmt.startsWith('BGRX')) {
    // Rec. 601 luma, the same weighting the native side gets from ffmpeg's
    // gray conversion, so a frame measured in the browser and the same frame
    // measured natively give the same fingerprint.
    const l = layout[0];
    const r0 = fmt.startsWith('B') ? 2 : 0, b0 = fmt.startsWith('B') ? 0 : 2;
    for (let y = 0; y < h; y++) {
      const src = l.offset + y * l.stride;
      for (let x = 0; x < w; x++) {
        const q = src + x * 4;
        dst[p + y * w + x] =
          (scratch[q + r0] * 77 + scratch[q + 1] * 150 + scratch[q + b0] * 29) >> 8;
      }
    }
  } else {
    const y = layout[0];
    for (let row = 0; row < h; row++) {
      const from = y.offset + row * y.stride;
      dst.set(scratch.subarray(from, from + w), p + row * w);
    }
  }

  const push = which === 'original' ? wasm.er_push_original : wasm.er_push_copy;
  const read = push(p, w, h, index, frame.timestamp);
  wasm.er_free(p, w * h);
  return read;
}

/// Read one file whole. `onProgress(done, total)` is called as it goes.
async function scan(which, buffer, track, onProgress) {
  let strips = 0;
  let seen = 0;
  const total = track.samples.length;
  await decodeEveryFrame(buffer, track, async (frame, index) => {
    strips += await pushFrame(which, frame, index);
    if (++seen % 60 === 0 && onProgress) onProgress(seen, total);
  });
  if (onProgress) onProgress(seen, total);
  return { frames: seen, strips };
}

/// Compare two files and return the report's HTML fragment.
///
/// `chainVerdict` is quoted, never derived here: the cryptographic check is
/// the bundle's own `verify_bundle.py`, and there is no Python in a browser.
/// Saying so is the honest thing; re-implementing it in JavaScript would be a
/// second implementation of the one thing that must have exactly one.
export async function compare({ originalFile, copyFile, shortId, chainVerdict, chainPassed,
                                sensitivity, grid, onProgress }) {
  const originalBuffer = await originalFile.arrayBuffer();
  const originalTrack = demux(originalBuffer);

  wasm.er_reset(originalTrack.width, originalTrack.height);
  // Before the first frame: the grids are built as frames arrive, so changing
  // the geometry afterwards would compare two different griddings.
  wasm.er_set_diff(sensitivity || 0, grid || 0);
  setText(0, shortId || '');
  setText(1, chainVerdict || 'not established here — this page does not run the bundle\u2019s verifier');
  setText(2, originalFile.name);
  setText(3, copyFile.name);
  setText(4, URL.createObjectURL(originalFile));
  setText(5, URL.createObjectURL(copyFile));
  wasm.er_set_chain_passed(chainPassed === false ? 0 : 1);

  const o = await scan('original', originalBuffer, originalTrack,
    (d, t) => onProgress && onProgress('original', d, t));

  const copyBuffer = await copyFile.arrayBuffer();
  const copyTrack = demux(copyBuffer);
  const c = await scan('copy', copyBuffer, copyTrack,
    (d, t) => onProgress && onProgress('copy', d, t));

  wasm.er_set_copy_duration_us(copyTrack.durationUs);
  const fps = copyTrack.durationUs > 0 ? (c.frames / (copyTrack.durationUs / 1e6)) : 30;
  const html = takeOut(wasm.er_finish(fps));
  return {
    html,
    originalFrames: o.frames,
    copyFrames: c.frames,
    originalDeclaring: wasm.er_original_declaring(),
    located: wasm.er_located_count(),
  };
}
