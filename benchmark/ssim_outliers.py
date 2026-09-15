#!/usr/bin/env python3
"""Find frames that stand out in an ffmpeg ssim stats file.

ffmpeg's ssim filter writes one line per frame. The global average it prints at
the end is the number everybody quotes, and it is the number that ranks a
150 kbit/s recompression below a deliberate retouch. This reads the per-frame
log instead and asks the same question the edit-report asks: is this frame's
value unusual *for this file*, measured against the file's own median and
median-absolute-deviation.

No threshold in SSIM units appears here, on purpose: a heavily recompressed
file raises its own bar.

    ssim_outliers.py <stats.log> [deviations]

Prints the frames furthest BELOW the median, in MAD units. It says nothing
about why a frame stands out; an encoder settling at the start of a file
produces the same shape as a substituted frame, and telling those apart is not
something this script can do.
"""
import re
import statistics
import sys


def load(path):
    values = []
    for line in open(path):
        m = re.search(r"All:([0-9.]+)", line)
        if m:
            values.append(float(m.group(1)))
    return values


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip())
        return 2
    path = sys.argv[1]
    bar = float(sys.argv[2]) if len(sys.argv) > 2 else 8.0

    values = load(path)
    if not values:
        print(f"{path}\tno per-frame lines")
        return 1

    median = statistics.median(values)
    mad = statistics.median([abs(v - median) for v in values])
    if mad == 0:
        print(f"{path}\tn={len(values)}\tmedian={median:.4f}\tspread is zero, "
              f"nothing can be judged against it")
        return 0

    out = sorted(
        ((median - v) / mad, i + 1, v) for i, v in enumerate(values)
    )[::-1]

    print(f"n={len(values)}\tmedian={median:.4f}\tmad={mad:.5f}\tbar={bar} deviations")
    hits = [row for row in out if row[0] >= bar]
    if not hits:
        print("  no frame departs from this file's own spread by that much")
        return 0
    for dev, frame, value in hits[:10]:
        print(f"  f={frame:<5d} t={(frame - 1) / 30:6.2f}s  ssim={value:.4f}  "
              f"{dev:7.1f} deviations below the median")
    if len(hits) > 10:
        print(f"  ... and {len(hits) - 10} more")
    return 0


if __name__ == "__main__":
    sys.exit(main())
