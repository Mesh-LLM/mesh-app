//! BOLT11 -> PNG QR code. Pure data transform; no UI and no networking.

/// Uppercase with the `LIGHTNING:` scheme: alphanumeric QR mode is denser,
/// and wallets and exchanges accept it (BOLT11 is case-insensitive).
pub fn lightning_uri(bolt11: &str) -> String {
    format!("LIGHTNING:{}", bolt11.trim().to_ascii_uppercase())
}

/// Black-on-white PNG with a 4-module quiet zone, `scale` pixels per module.
pub fn png(data: &str, scale: u32) -> Result<Vec<u8>, String> {
    let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| e.to_string())?;
    let modules = code.width() as u32;
    let quiet = 4;
    let side = (modules + 2 * quiet) * scale;
    let mut pixels = vec![255u8; (side * side) as usize];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                for dy in 0..scale {
                    let row = ((y + quiet) * scale + dy) * side;
                    let start = (row + (x + quiet) * scale) as usize;
                    pixels[start..start + scale as usize].fill(0);
                }
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, side, side);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&pixels)
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_is_uppercase_lightning_scheme() {
        assert_eq!(lightning_uri(" lnbc10n1abc "), "LIGHTNING:LNBC10N1ABC");
    }

    #[test]
    fn png_round_trips_with_quiet_zone_and_dark_modules() {
        let bytes = png(
            &lightning_uri("lnbc1pvjluezpp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypq"),
            4,
        )
        .unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.width, info.height);
        assert_eq!(buf[0], 255, "quiet zone is white");
        assert!(buf.contains(&0), "has dark modules");
    }
}
