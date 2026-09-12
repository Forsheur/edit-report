// A minimal MP4 demuxer, because WebCodecs does not demux.
//
// `VideoDecoder` takes encoded chunks and gives back frames; getting the
// chunks out of a container is the caller's problem. The alternative — playing
// the file in a <video> element and grabbing frames as they are presented —
// runs in real time and drops frames whenever the tab is busy, and this tool
// exists to read EVERY frame. So the container is parsed here.
//
// Two shapes are handled, and between them they cover what this tool is
// pointed at:
//
//   * FRAGMENTED (moof/trun) — what a Forsheur phone writes, and what the
//     bundle's verifier hands back;
//   * PROGRESSIVE (moov/stbl) — what a platform re-encode produces.
//
// Only the video track is read. Audio is not this tool's business here: the
// core works without it by design.

const MICROS = 1_000_000;

function u32(v, p) { return v.getUint32(p); }
function u64(v, p) { return v.getUint32(p) * 4294967296 + v.getUint32(p + 4); }
function fourcc(v, p) {
  return String.fromCharCode(v.getUint8(p), v.getUint8(p + 1), v.getUint8(p + 2), v.getUint8(p + 3));
}

/// Walk the boxes at [start, end), calling `fn(type, payloadStart, payloadEnd)`.
function walk(view, start, end, fn) {
  let p = start;
  while (p + 8 <= end) {
    let size = u32(view, p);
    const type = fourcc(view, p + 4);
    let head = 8;
    if (size === 1) { size = u64(view, p + 8); head = 16; }
    else if (size === 0) { size = end - p; }
    if (size < head || p + size > end) break;
    fn(type, p + head, p + size);
    p += size;
  }
}

/// Find the first box of `type` among the children of [start, end).
function child(view, start, end, type) {
  let found = null;
  walk(view, start, end, (t, s, e) => { if (t === type && !found) found = [s, e]; });
  return found;
}

/// Follow a path of box types down from [start, end).
function descend(view, start, end, path) {
  let cur = [start, end];
  for (const t of path) {
    cur = child(view, cur[0], cur[1], t);
    if (!cur) return null;
  }
  return cur;
}

/// The codec description `VideoDecoder` needs (`avcC` / `hvcC` payload), plus
/// the codec string and the coded size.
function videoSampleEntry(view, stsd) {
  let out = null;
  // stsd: 4 bytes version/flags, 4 bytes entry count, then the entries.
  walk(view, stsd[0] + 8, stsd[1], (type, s, e) => {
    if (out) return;
    if (!['avc1', 'avc3', 'hvc1', 'hev1'].includes(type)) return;
    const width = view.getUint16(s + 24);
    const height = view.getUint16(s + 26);
    // The sample entry is 78 bytes, then child boxes.
    let desc = null, codec = null;
    walk(view, s + 78, e, (t2, s2, e2) => {
      if (t2 === 'avcC' && !desc) {
        desc = new Uint8Array(view.buffer, view.byteOffset + s2, e2 - s2);
        const p = view.getUint8(s2 + 1), c = view.getUint8(s2 + 2), l = view.getUint8(s2 + 3);
        codec = `avc1.${[p, c, l].map(b => b.toString(16).padStart(2, '0')).join('')}`;
      } else if (t2 === 'hvcC' && !desc) {
        desc = new Uint8Array(view.buffer, view.byteOffset + s2, e2 - s2);
        codec = 'hvc1.1.6.L93.B0';
      }
    });
    if (desc) out = { codec, description: desc.slice(), width, height };
  });
  return out;
}

/// Sample tables of a progressive track, flattened into a sample list.
function progressiveSamples(view, stbl, timescale) {
  const g = t => child(view, stbl[0], stbl[1], t);
  const stts = g('stts'), stsz = g('stsz'), stsc = g('stsc');
  const stco = g('stco'), co64 = g('co64'), stss = g('stss'), ctts = g('ctts');
  if (!stts || !stsz || !stsc || (!stco && !co64)) return [];

  // Decode times.
  const times = [];
  {
    const n = u32(view, stts[0] + 4);
    let t = 0;
    for (let i = 0; i < n; i++) {
      const count = u32(view, stts[0] + 8 + i * 8);
      const delta = u32(view, stts[0] + 12 + i * 8);
      for (let k = 0; k < count; k++) { times.push(t); t += delta; }
    }
  }
  // Sizes.
  const sizes = [];
  {
    const uniform = u32(view, stsz[0] + 4);
    const n = u32(view, stsz[0] + 8);
    for (let i = 0; i < n; i++) {
      sizes.push(uniform !== 0 ? uniform : u32(view, stsz[0] + 12 + i * 4));
    }
  }
  // Chunk offsets.
  const offsets = [];
  {
    const box = stco || co64;
    const n = u32(view, box[0] + 4);
    for (let i = 0; i < n; i++) {
      offsets.push(stco ? u32(view, box[0] + 8 + i * 4) : u64(view, box[0] + 8 + i * 8));
    }
  }
  // Samples per chunk.
  const runs = [];
  {
    const n = u32(view, stsc[0] + 4);
    for (let i = 0; i < n; i++) {
      runs.push({
        first: u32(view, stsc[0] + 8 + i * 12),
        perChunk: u32(view, stsc[0] + 12 + i * 12),
      });
    }
  }
  // Sync samples: absent means every sample is a keyframe.
  let sync = null;
  if (stss) {
    sync = new Set();
    const n = u32(view, stss[0] + 4);
    for (let i = 0; i < n; i++) sync.add(u32(view, stss[0] + 8 + i * 4) - 1);
  }
  // Composition offsets.
  const comp = [];
  if (ctts) {
    const n = u32(view, ctts[0] + 4);
    for (let i = 0; i < n; i++) {
      const count = u32(view, ctts[0] + 8 + i * 8);
      const off = view.getInt32(ctts[0] + 12 + i * 8);
      for (let k = 0; k < count; k++) comp.push(off);
    }
  }

  const out = [];
  let sample = 0;
  for (let c = 0; c < offsets.length && sample < sizes.length; c++) {
    let per = runs[0] ? runs[0].perChunk : 1;
    for (let r = runs.length - 1; r >= 0; r--) {
      if (c + 1 >= runs[r].first) { per = runs[r].perChunk; break; }
    }
    let at = offsets[c];
    for (let k = 0; k < per && sample < sizes.length; k++, sample++) {
      const dts = times[sample] || 0;
      const pts = dts + (comp[sample] || 0);
      out.push({
        offset: at,
        size: sizes[sample],
        timestamp: Math.round((pts / timescale) * MICROS),
        type: !sync || sync.has(sample) ? 'key' : 'delta',
      });
      at += sizes[sample];
    }
  }
  // DECODE order, deliberately left as the file has it.
  //
  // Sorting these by timestamp seemed tidier and broke the first re-encoded
  // copy tried: `VideoDecoder` needs chunks in decode order, and a stream with
  // B-frames does not present them in that order. It emits frames in
  // presentation order on its own, which is what the report's timestamps mean.
  // The Forsheur original has no B-frames, so the mistake hid until a copy
  // came through ffmpeg.
  return out;
}

/// Samples of a fragmented track, from every moof in the file.
function fragmentedSamples(view, trackId, timescale, defaults) {
  const out = [];
  walk(view, 0, view.byteLength, (type, s, e) => {
    if (type !== 'moof') return;
    const moofStart = s - 8;
    walk(view, s, e, (t2, s2, e2) => {
      if (t2 !== 'traf') return;
      const tfhd = child(view, s2, e2, 'tfhd');
      if (!tfhd) return;
      const tfFlags = u32(view, tfhd[0]) & 0xffffff;
      const id = u32(view, tfhd[0] + 4);
      if (id !== trackId) return;
      let p = tfhd[0] + 8;
      let baseOffset = moofStart;
      if (tfFlags & 0x01) { baseOffset = u64(view, p); p += 8; }
      if (tfFlags & 0x02) { p += 4; }                       // sample-description-index
      let defDur = defaults.duration, defSize = defaults.size, defFlags = defaults.flags;
      if (tfFlags & 0x08) { defDur = u32(view, p); p += 4; }
      if (tfFlags & 0x10) { defSize = u32(view, p); p += 4; }
      if (tfFlags & 0x20) { defFlags = u32(view, p); p += 4; }

      let baseTime = 0;
      const tfdt = child(view, s2, e2, 'tfdt');
      if (tfdt) {
        const ver = view.getUint8(tfdt[0]);
        baseTime = ver === 1 ? u64(view, tfdt[0] + 4) : u32(view, tfdt[0] + 4);
      }

      walk(view, s2, e2, (t3, s3) => {
        if (t3 !== 'trun') return;
        const flags = u32(view, s3) & 0xffffff;
        const count = u32(view, s3 + 4);
        let q = s3 + 8;
        let dataOffset = baseOffset;
        if (flags & 0x001) { dataOffset = baseOffset + view.getInt32(q); q += 4; }
        let firstFlags = null;
        if (flags & 0x004) { firstFlags = u32(view, q); q += 4; }
        let t = baseTime;
        let at = dataOffset;
        for (let i = 0; i < count; i++) {
          let dur = defDur, size = defSize, sflags = firstFlags !== null && i === 0 ? firstFlags : defFlags, cto = 0;
          if (flags & 0x100) { dur = u32(view, q); q += 4; }
          if (flags & 0x200) { size = u32(view, q); q += 4; }
          if (flags & 0x400) { sflags = u32(view, q); q += 4; }
          if (flags & 0x800) { cto = view.getInt32(q); q += 4; }
          // "sample_is_non_sync_sample" is bit 16 of the sample flags.
          const isSync = !(sflags & 0x00010000);
          out.push({
            offset: at,
            size,
            timestamp: Math.round(((t + cto) / timescale) * MICROS),
            type: isSync ? 'key' : 'delta',
          });
          at += size;
          t += dur;
        }
      });
    });
  });
  return out;
}

/// Parse `buffer` and return the video track: codec, size, and every sample in
/// presentation order.
export function demux(buffer) {
  const view = new DataView(buffer);
  const moov = child(view, 0, view.byteLength, 'moov');
  if (!moov) throw new Error('no moov box — this does not look like an MP4');

  let track = null;
  walk(view, moov[0], moov[1], (type, s, e) => {
    if (type !== 'trak' || track) return;
    const hdlr = descend(view, s, e, ['mdia', 'hdlr']);
    if (!hdlr || fourcc(view, hdlr[0] + 8) !== 'vide') return;
    const tkhd = child(view, s, e, 'tkhd');
    const mdhd = descend(view, s, e, ['mdia', 'mdhd']);
    const stbl = descend(view, s, e, ['mdia', 'minf', 'stbl']);
    if (!tkhd || !mdhd || !stbl) return;
    const ver = view.getUint8(mdhd[0]);
    const timescale = ver === 1 ? u32(view, mdhd[0] + 20) : u32(view, mdhd[0] + 12);
    const trackId = view.getUint8(tkhd[0]) === 1 ? u32(view, tkhd[0] + 20) : u32(view, tkhd[0] + 12);
    const stsd = child(view, stbl[0], stbl[1], 'stsd');
    const entry = stsd ? videoSampleEntry(view, stsd) : null;
    if (!entry) return;
    track = { trackId, timescale, ...entry, stbl };
  });
  if (!track) throw new Error('no video track with a codec this build understands');

  // Fragment defaults live in mvex/trex, and are what a trun falls back to.
  let defaults = { duration: 0, size: 0, flags: 0 };
  const trex = descend(view, moov[0], moov[1], ['mvex', 'trex']);
  if (trex) {
    defaults = {
      duration: u32(view, trex[0] + 12),
      size: u32(view, trex[0] + 16),
      flags: u32(view, trex[0] + 20),
    };
  }

  let samples = progressiveSamples(view, track.stbl, track.timescale);
  if (samples.length === 0) {
    samples = fragmentedSamples(view, track.trackId, track.timescale, defaults);
  }
  if (samples.length === 0) throw new Error('the video track carries no samples this build could locate');

  return {
    codec: track.codec,
    description: track.description,
    width: track.width,
    height: track.height,
    samples,
    // Latest presentation time, plus one frame. Not the last sample's — in
    // decode order the last sample is not the last one shown.
    durationUs: (() => {
      let last = 0, prev = 0;
      for (const s of samples) {
        if (s.timestamp > last) { prev = last; last = s.timestamp; }
      }
      return samples.length > 1 ? last + (last - prev) : last;
    })(),
  };
}

/// Decode every frame, in order, awaiting `onFrame(VideoFrame, index)`.
///
/// Backpressure on both sides. The decoder queue is capped so a long file
/// never sits decoded in memory, and the number of frames handed out and not
/// yet released is capped too: `onFrame` is asynchronous (copying a plane out
/// of a VideoFrame is), and without a bound the callback's work would pile up
/// behind a decoder that is happy to run ahead. The whole point of this pass
/// is that it never holds a video.
///
/// The frame handed to `onFrame` is closed for you once its promise settles.
export async function decodeEveryFrame(buffer, track, onFrame) {
  const bytes = new Uint8Array(buffer);
  let index = 0;
  let pending = 0;
  let failed = null;
  const tick = () => new Promise(r => setTimeout(r, 0));

  const decoder = new VideoDecoder({
    output: (frame) => {
      const held = frame.clone();
      frame.close();
      pending++;
      Promise.resolve()
        .then(() => onFrame(held, index++))
        .catch(e => { failed = failed || e; })
        .finally(() => { held.close(); pending--; });
    },
    error: (e) => { failed = failed || e; },
  });
  decoder.configure({
    codec: track.codec,
    description: track.description,
    optimizeForLatency: true,
  });

  for (const s of track.samples) {
    if (failed) break;
    decoder.decode(new EncodedVideoChunk({
      type: s.type,
      timestamp: s.timestamp,
      data: bytes.subarray(s.offset, s.offset + s.size),
    }));
    while (!failed && (decoder.decodeQueueSize > 24 || pending > 8)) await tick();
  }
  // A codec that raised an error has already closed itself; flushing it then
  // throws a second, less useful error over the first one.
  try {
    if (!failed) await decoder.flush();
  } catch (e) {
    failed = failed || e;
  }
  while (pending > 0) await tick();
  if (decoder.state !== 'closed') decoder.close();
  if (failed) throw failed;
  return index;
}
