#!/usr/bin/env python3
"""Run Meta's vpdq (facebook/ThreatExchange) over a reference and copies.

vpdq is the one competitor whose design targets the same question we do: it
hashes frames with PDQ (256 bits each) and is meant to report WHICH parts of a
video match, not just whether it does. The Python binding published on PyPI
exposes only computeHash(); the matching is in the C++ side. So the matching
below is written here, at vpdq's own documented threshold, which makes this a
generous reading of vpdq rather than a strict one.

    vpdq_run.py <reference.mp4> <copy.mp4> [copy.mp4 ...]

Needs libav* headers at build time:
    PKG_CONFIG_PATH=/opt/homebrew/lib/pkgconfig pip install vpdq
"""
import os
import sys
import time

import vpdq

SECONDS_PER_HASH = 0.2   # 5 hashes per second, as for vidhash
DISTANCE = 31            # vpdq's own documented per-frame threshold, of 256


def hashes_of(path):
    started = time.time()
    features = vpdq.computeHash(os.path.abspath(path),
                                seconds_per_hash=SECONDS_PER_HASH)
    return features, time.time() - started


def compare(ref, copy):
    """Fraction of copy frames with any reference frame within DISTANCE, and
    the single time offset that most of those agree on."""
    matched = 0
    offsets = []
    for c in copy:
        best = None
        for r in ref:
            d = vpdq.hamming_distance(c.hash, r.hash)
            if d <= DISTANCE and (best is None or d < best[0]):
                best = (d, r.timestamp)
        if best is not None:
            matched += 1
            offsets.append(round(best[1] - c.timestamp, 1))

    if not offsets:
        return matched, None, 0
    # The offset the most frames agree on, and how many agree on it.
    counts = {}
    for o in offsets:
        counts[o] = counts.get(o, 0) + 1
    top = max(counts.items(), key=lambda kv: kv[1])
    return matched, top[0], top[1]


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip())
        return 2

    reference, copies = sys.argv[1], sys.argv[2:]
    ref, secs = hashes_of(reference)
    print(f"reference\t{len(ref)} frame hashes\t-\t{secs:.1f}s")

    for path in copies:
        name = os.path.basename(path)
        try:
            copy, secs = hashes_of(path)
            matched, offset, agreeing = compare(ref, copy)
            share = 100.0 * matched / len(copy) if copy else 0.0
            if offset is None:
                detail = "no frame within distance 31 of any reference frame"
            else:
                detail = (f"{matched}/{len(copy)} frames matched ({share:.0f}%), "
                          f"{agreeing} of them at offset {offset:+.1f}s")
            print(f"{name}\t{detail}\t{secs:.1f}s")
        except Exception as exc:  # a tool that fails is a result, not a crash
            print(f"{name}\tFAILED\t{type(exc).__name__}: {exc}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
