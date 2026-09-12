//! Emit the bit-row test vectors that the phone implementations must match.
//!
//!     cargo run -p edit-report-core --example bitrow_vectors

use edit_report_core::bitrow::{crc16, encode, session_tag, CELLS};

fn main() {
    let cases: [(Option<&str>, u64); 6] = [
        (None, 0),
        (None, 1),
        (Some("NElG7hqbvvV8"), 1),
        (Some("NElG7hqbvvV8"), 301),
        (Some("IF9kZhHt0TJA"), 16_777_215),
        (Some("3gvqtL7Y7HZL"), 123_456),
    ];
    println!("| short_id | counter | tag | crc | cells |");
    println!("|---|---|---|---|---|");
    for (sid, counter) in cases {
        let tag = session_tag(sid);
        let payload = [
            (tag >> 8) as u8,
            tag as u8,
            (counter >> 16) as u8,
            (counter >> 8) as u8,
            counter as u8,
        ];
        let cells = encode(tag, counter);
        let s: String = cells.iter().map(|b| if *b { '1' } else { '0' }).collect();
        assert_eq!(s.len(), CELLS);
        println!(
            "| `{}` | {counter} | `0x{tag:04X}` | `0x{:04X}` | `{s}` |",
            sid.unwrap_or("(none)"),
            crc16(&payload),
        );
    }
}
