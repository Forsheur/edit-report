#!/usr/bin/env python3
"""Run SpangleLabs/vidhash over a reference and a list of copies.

vidhash hashes every sampled frame (dHash at a chosen fps) and then answers one
question: check_match() -> True or False. Two states, no third one, and no
position. The count of agreeing frames below is recomputed here from the frame
hashes the library exposes, because the library itself does not report it.

    vidhash_run.py <reference.mp4> <copy.mp4> [copy.mp4 ...]

The API is async, hence asyncio.run.
"""
import asyncio
import os
import sys
import time

import vidhash
from vidhash.func import CheckOptions
from vidhash.match_options import PercentageMatch

FPS = 5
HAMMING = 3          # vidhash's own default
MATCH_PERCENT = 30   # vidhash's own default

OPTS = vidhash.HashOptions(fps=FPS)


async def hash_of(path):
    started = time.time()
    return await vidhash.hash_video(os.path.abspath(path), OPTS), time.time() - started


def best_offset(copy_hashes, ref_hashes):
    """Frames agreeing at the single offset that agrees most.

    This is generous to vidhash: it is not what the library reports, it is the
    most favourable reading of what it computed.
    """
    best = (0, 0)
    for offset in range(-len(copy_hashes) + 1, len(ref_hashes)):
        agreeing = sum(
            1
            for i, frame in enumerate(copy_hashes)
            if 0 <= i + offset < len(ref_hashes)
            and frame.similar_to(ref_hashes[i + offset], HAMMING)
        )
        if agreeing > best[0]:
            best = (agreeing, offset)
    return best


async def main():
    if len(sys.argv) < 3:
        print(__doc__.strip())
        return 2

    reference, copies = sys.argv[1], sys.argv[2:]
    ref, secs = await hash_of(reference)
    print(f"reference\t{len(ref.image_hashes)} frame hashes at {FPS} fps\t-\t{secs:.1f}s")

    for path in copies:
        name = os.path.basename(path)
        try:
            other, secs = await hash_of(path)
            agreeing, offset = best_offset(other.image_hashes, ref.image_hashes)
            matched = await vidhash.check_match(
                os.path.abspath(reference),
                os.path.abspath(path),
                CheckOptions(OPTS, PercentageMatch(HAMMING, MATCH_PERCENT)),
            )
            print(
                f"{name}\tcheck_match={matched}\t"
                f"{agreeing}/{len(other.image_hashes)} frames agree at offset "
                f"{offset / FPS:+.1f}s\t{secs:.1f}s"
            )
        except Exception as exc:  # a tool that fails is a result, not a crash
            print(f"{name}\tFAILED\t-\t{type(exc).__name__}: {exc}")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
