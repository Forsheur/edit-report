# Burn-in bit row — format v1

A machine-readable strip burned into every frame alongside the human-readable
overlay. It carries the frame counter so a tool can read it reliably; the text
band above it stays, unchanged, for people.

**It is not a security feature and must never be described as one.** These are
unsigned pixels, exactly like the text. Anyone can draw them. What the strip
buys is *legibility*: the text reader manages 50–75 % of frames and cannot tell
a misread from a good one, while this strip is read at essentially every frame
and a corrupted read fails its checksum instead of returning a wrong number.

See `SECURITY.md` for why no cryptographic construction can fix the
forgeability, and why it does not need fixing: a forged counter points at a
frame of the original, that frame is fetched, and the pictures decide.

---

## Geometry

All values are in the pixels the phone composes at, and scale with the frame
the way the rest of the overlay does.

| | |
|---|---|
| Strip rows | **0 … 11** inclusive (12 rows) — the top of the frame |
| Strip left edge | **12** |
| Strip right edge | frame width − 12 |
| QR | 87 px at x = 4, **y = 16** — below the strip |
| Text baselines | **36, 64, 92** — 28 apart, as before |
| Band height | **98** |

The strip comes first and everything else follows it. That is not only tidier:
nothing shares the strip's rows, so it runs the **full width** rather than
starting after the QR, and every cell is wider for it — at 720 px that is 9
pixels a cell instead of 8, and at 1280 it is 16 instead of 15.

An earlier version placed the strip below the three text lines, at rows 84…95
with a left edge of 95. It read at 100 % on four real recordings, so the move
is about placement and margin, not about a defect.

The strip is **opaque**: a cell is filled solid, it does not blend with the
picture underneath. Solid blocks are what a video codec preserves best, and the
alternative — a translucent strip — is what makes the QR hard to read.

### Cells

```
cell_width  = (right_edge − left_edge) / 76      integer division
strip spans   left_edge … left_edge + 76 × cell_width
```

Cell *k* fills columns `left_edge + k × cell_width … left_edge + (k+1) × cell_width − 1`,
all 12 rows.

* bit **1** → white, RGB (255, 255, 255)
* bit **0** → black, RGB (0, 0, 0)

Any remainder to the right of the last cell is left as picture.

---

## The 76 cells

| Cells | Field | |
|---|---|---|
| 0 – 9 | start | `1 0 1 0 1 0 1 0 1 1` |
| 10 – 25 | session tag | 16 bits, MSB first |
| 26 – 49 | frame counter | 24 bits, MSB first |
| 50 – 65 | CRC | 16 bits, MSB first |
| 66 – 75 | stop | `0 1 0 1 0 1 0 1 0 0` |

**Start and stop alternate** so a reader recovers the cell pitch from them
without being told it — a transition every cell is a clock.

The stop is the start's **complement**, not its mirror. The first draft used
`1101010101`, whose reverse is exactly the start pattern: a strip read
right-to-left would have framed perfectly and handed back a reversed payload.
Check this property in any implementation — reversing either pattern must not
produce the other.

**Session tag** — the first two bytes of `SHA-256(short_id)`, where `short_id`
is the 12-character base62 id in ASCII, big-endian. `0x0000` when the recording
has no short id.

It exists so material spliced in from *another Forsheur recording* is
recognisable as such from the strip alone, rather than only from the pictures
disagreeing. Two bytes is not an identifier and is not meant to be: it
distinguishes recordings, it does not name them.

**Frame counter** — the same value the text band shows as `f=`, 24 bits,
wrapping at 16 777 216 (155 hours at 30 fps).

**CRC** — CRC-16/CCITT-FALSE over the five payload bytes in order:
`tag_hi, tag_lo, counter[23:16], counter[15:8], counter[7:0]`.

```
polynomial 0x1021, initial value 0xFFFF, no reflection, no final xor
```

A reader that finds the framing but fails the CRC reports **no counter**, never
a guessed one.

---

## Test vectors

Produced by `edit-report-core`, which both phone implementations must match.
`cells` is the 76 cells as `0`/`1`, left to right.

|---|---|---|---|---|
| `(none)` | 0 | `0x0000` | `0x110C` | `1010101011000000000000000000000000000000000000000000010001000011000101010100` |
| `(none)` | 1 | `0x0000` | `0x012D` | `1010101011000000000000000000000000000000000000000100000001001011010101010100` |
| `NElG7hqbvvV8` | 1 | `0xC81A` | `0x43B4` | `1010101011110010000001101000000000000000000000000101000011101101000101010100` |
| `NElG7hqbvvV8` | 301 | `0xC81A` | `0x956B` | `1010101011110010000001101000000000000000010010110110010101011010110101010100` |
| `IF9kZhHt0TJA` | 16777215 | `0xDFCB` | `0x3CC0` | `1010101011110111111100101111111111111111111111111100111100110000000101010100` |
| `3gvqtL7Y7HZL` | 123456 | `0xD992` | `0xAC41` | `1010101011110110011001001000000001111000100100000010101100010000010101010100` |

---

## What a reader must do

1. Take rows 0…11 of the frame, scaled by the frame's own scale factor.
2. Threshold at the midpoint between the row's darkest and brightest columns.
3. **Recover the clock from the strip, never from assumed geometry.** Find the
   run of eight equal alternating runs that opens the start pattern, followed
   by a wider lit run — that wider run is cells 8 and 9, which merge because
   both are lit, and it grows further when the session tag's top bits are lit
   too, so it has a lower bound and no upper one. Cell 0 begins eight cells
   before it.
4. Read 76 cells, sampling the middle half of each.
5. Check the CRC. On failure, report no counter — never a guess.

### Getting the pitch right

This is where a reader will get it wrong, so it is worth being explicit.

Run lengths are whole pixels. When the pitch is a whole number — an unscaled
frame — their median is exact. When it is not, it is half a pixel out, and half
a pixel per cell is **eight cells of drift** across the strip. A copy scaled to
half width carries a pitch of 4.5, and that is the ordinary fate of a copy.

Three estimates are worth having, and none of them wins everywhere:

* the **median** run length — exact at whole-pixel pitches;
* the **span** the alternations cover, divided by how many there are — averages
  the rounding away, but measure it over the INTERIOR runs: the first cell's
  left edge bleeds into the picture beside it;
* the distance to the **stop pattern**, sixty-six cells away — the longest
  baseline available, and the most precise when it locks onto the right run.

Do not choose between them. Try each, plus a small spread of near misses, and
let the **CRC** say which was right. That is what the checksum is for, and it
costs a few hundred comparisons per frame.

Measured this way on a real recording, at 1500 kbit/s and at 150 kbit/s and at
two-thirds width: every frame. At half width: nine frames in ten, and the
tenth refused rather than wrong.
