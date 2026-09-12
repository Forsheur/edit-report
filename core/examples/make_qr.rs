//! Render a QR code to a PGM file, for building test fixtures.
//!
//! Used by `fixtures/derive.sh` to build the "unrelated video carrying a
//! copied reference" case. It exists because lifting the code out of a real
//! recording does not work: the burn-in is composited at 80 % opacity and then
//! H.264-compressed, so a crop of it carries module errors that no amount of
//! contrast stretching repairs — measured, not assumed.
//!
//! It is also the more faithful fixture. Someone forging a reference would
//! read the URL and render a clean code, not screen-grab a washed-out one.
//!
//! Usage: cargo run -p edit-report-core --example make_qr -- <text> <px> <out.pgm>

use rxing::qrcode::QRCodeWriter;
use rxing::{BarcodeFormat, EncodeHints, Writer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: make_qr <text> <size_px> <out.pgm>");
        std::process::exit(2);
    }
    let (text, size, out) = (&args[1], args[2].parse::<u32>()?, &args[3]);

    // Error correction M and a zero quiet zone, matching what the phone burns
    // in (`CameraService.makeQrCgImage` on iOS, `generateQrBitmap` on Android).
    // A fixture that used the defaults would be an easier code than the real
    // one, and would prove less than it appears to.
    let hints = EncodeHints::default()
        .with(rxing::EncodeHintValue::ErrorCorrection("M".to_owned()))
        .with(rxing::EncodeHintValue::Margin("0".to_owned()));
    let matrix = QRCodeWriter {}.encode_with_hints(
        text,
        &BarcodeFormat::QR_CODE,
        size as i32,
        size as i32,
        &hints,
    )?;

    let (w, h) = (matrix.getWidth(), matrix.getHeight());
    let mut pgm = format!("P5\n{w} {h}\n255\n").into_bytes();
    for y in 0..h {
        for x in 0..w {
            pgm.push(if matrix.get(x, y) { 0 } else { 255 });
        }
    }
    std::fs::write(out, pgm)?;
    eprintln!("wrote {out} ({w}×{h}) for {text:?}");
    Ok(())
}
