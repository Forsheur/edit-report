#!/usr/bin/env python3
"""Run akamhy/videohash over a reference and a list of copies.

videohash reduces a whole video to ONE 64-bit hash: it samples frames across
the recording, tiles them into a single collage image, and perceptually hashes
that collage. There is therefore no such thing as asking it where a cut is —
the question has no place to land in its output.

    videohash_run.py <reference.mp4> <copy.mp4> [copy.mp4 ...]

Prints one tab-separated line per copy: the hash, its Hamming distance from the
reference, and whether videohash's own is_similar() accepts it (default
similar_percentage = 15, i.e. about 9.6 of 64 bits).
"""
import os
import sys
import time

from PIL import Image

# videohash 3.0.1 calls PIL.Image.ANTIALIAS, which Pillow removed in version 10
# (July 2023). Upstream has not shipped a fix. Without this shim the package
# raises AttributeError on the first video and cannot be measured at all; the
# benchmark records that the shim was required.
if not hasattr(Image, "ANTIALIAS"):
    Image.ANTIALIAS = Image.LANCZOS

from videohash import VideoHash  # noqa: E402  (must follow the shim)


def hash_of(path):
    started = time.time()
    return VideoHash(path=os.path.abspath(path)), time.time() - started


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip())
        return 2

    reference, copies = sys.argv[1], sys.argv[2:]
    ref, secs = hash_of(reference)
    print(f"reference\t{ref.hash}\t-\t-\t{secs:.1f}s")

    for path in copies:
        name = os.path.basename(path)
        try:
            other, secs = hash_of(path)
            # .hash carries a "0b" prefix; compare the bits only.
            distance = sum(a != b for a, b in zip(ref.hash[2:], other.hash[2:]))
            similar = ref.is_similar(other)
            print(f"{name}\t{other.hash}\t{distance}\t{similar}\t{secs:.1f}s")
        except Exception as exc:  # a tool that fails is a result, not a crash
            print(f"{name}\tFAILED\t-\t-\t{type(exc).__name__}: {exc}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
