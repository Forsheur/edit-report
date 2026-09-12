#!/usr/bin/env python3
"""Reassemble the video of a Forsheur session from its public chunk payloads.

For test material only, and it exists because of a gap rather than a design:
a MIGRATED archive (`attest_kind = migration_import`) carries no notary proof,
so `evidence.zip` refuses to serve one — deliberately, since a degraded bundle
would muddy what a bundle means. The media is still public, chunk by chunk, and
that is what this reads.

Which means the videos it produces can be used to exercise NORMALISATION,
FINGERPRINTING and ALIGNMENT, and cannot be used to exercise the tool
end-to-end: there is no original to establish, so there is nothing to compare
against in the sense the report means. Do not build a fixture bundle out of
this — a made-up bundle would be exactly the thing the whole project refuses.

    python3 fetch_media.py https://forsheur.com <session_uuid> out.mp4

Standard library only, like the verifier it sits beside.
"""

import json
import subprocess
import sys
import tempfile
import urllib.request

# --------------------------------------------------------------------------
# Just enough canonical CBOR to read a payload: it is one map of one array of
# maps, with byte strings, text strings and unsigned integers. Written out
# rather than pulled in, for the same reason the verifier is stdlib-only.
# --------------------------------------------------------------------------


def _head(b, i):
    ib = b[i]
    major, minor = ib >> 5, ib & 0x1F
    i += 1
    if minor < 24:
        return major, minor, i
    for bits, n in ((24, 1), (25, 2), (26, 4), (27, 8)):
        if minor == bits:
            return major, int.from_bytes(b[i:i + n], "big"), i + n
    raise ValueError(f"unsupported CBOR header {minor} at {i - 1}")


def cbor_decode(b, i=0):
    major, arg, i = _head(b, i)
    if major == 0:
        return arg, i
    if major == 1:
        return -1 - arg, i
    if major == 2:
        return b[i:i + arg], i + arg
    if major == 3:
        return b[i:i + arg].decode("utf-8"), i + arg
    if major == 4:
        out = []
        for _ in range(arg):
            v, i = cbor_decode(b, i)
            out.append(v)
        return out, i
    if major == 5:
        out = {}
        for _ in range(arg):
            k, i = cbor_decode(b, i)
            v, i = cbor_decode(b, i)
            out[k] = v
        return out, i
    if major == 7 and arg == 22:
        return None, i
    raise ValueError(f"unsupported CBOR major type {major}")


def get(url):
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    base, session, out = sys.argv[1].rstrip("/"), sys.argv[2], sys.argv[3]

    listing = json.loads(get(f"{base}/api/v2/sessions/{session}/chunks"))
    chunks = listing if isinstance(listing, list) else listing.get("chunks", [])
    chunks.sort(key=lambda c: c["seq"])
    print(f"{len(chunks)} chunk(s)")

    streams = {}
    for c in chunks:
        payload = get(f"{base}/api/v2/chunks/{c['chunk_id']}/payload")
        decoded, _ = cbor_decode(payload)
        for s in decoded.get("streams", []):
            streams.setdefault(s["id"], []).append(s["bytes"])
        print(f"  seq {c['seq']:>3}: {len(payload):>9} bytes", end="\r")
    print()

    video = sorted(k for k in streams if k.startswith("video."))
    if not video:
        sys.exit("no video stream in this session — is it encrypted?")
    # Back camera first: the one pointed at the scene.
    vid = next((k for k in video if ".back." in k), video[0])
    print(f"stream {vid}: {len(streams[vid])} fragment(s)")

    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as f:
        for blob in streams[vid]:
            f.write(blob)
        raw = f.name

    # Remux, never re-encode: the bytes the device produced stay the bytes.
    r = subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", raw, "-c", "copy", out])
    if r.returncode != 0:
        sys.exit(f"ffmpeg failed; the raw stream is at {raw}")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
