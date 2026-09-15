//! Cards as pasteable text, so the journey needs no file at all.
//!
//! A file was the only way to hand a card over, and macOS has never heard of
//! `.meshrequest`: it arrives in a note or a message as an unopenable
//! attachment, and the receiver then has to find it on disk and drive a file
//! picker. Text survives every chat app, and pasting is one step.
//!
//! The encoding is deliberately boring. `MESH1` marks the start, the payload is
//! unpadded URL-safe base64 -- no `+`, `/` or `=` for a chat client to wrap,
//! quote or turn into a smart character -- and the decoder ignores anything
//! before the marker, anything that is not a payload character inside it, and
//! everything from
//! the closing `.` onwards. That is what makes "select roughly the right part
//! of the note and copy" work, and why a signature line or "Sent from my
//! phone" after the card is harmless -- without the terminator, trailing prose
//! would decode as more payload, because letters are payload characters.
const MARKER: &str = "MESH1";
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// Wrapped short enough to survive quoting and indentation in mail replies.
const LINE: usize = 60;

fn value(byte: u8) -> Option<u32> {
    ALPHABET.iter().position(|c| *c == byte).map(|i| i as u32)
}

/// A card as text to paste. Wrapped, and safe to copy with surrounding prose.
pub fn encode(bytes: &[u8]) -> String {
    let mut payload = String::new();
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
        // 3 bytes -> 4 characters; a short final chunk emits only the
        // characters its bits reach, which is what "unpadded" means.
        for index in 0..chunk.len() + 1 {
            let shift = 18 - index * 6;
            payload.push(ALPHABET[((bits >> shift) & 0x3f) as usize] as char);
        }
    }
    let mut card = String::from(MARKER);
    for (index, character) in payload.chars().enumerate() {
        if index % LINE == 0 {
            card.push('\n');
        }
        card.push(character);
    }
    card.push_str("\n.\n");
    card
}

/// Recover a card from pasted text, or explain that there is no card in it.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    let start = text
        .find(MARKER)
        .ok_or("That does not look like a Mesh card. Copy the whole thing they sent, including the MESH1 line.")?
        + MARKER.len();
    let mut values = Vec::new();
    for byte in text[start..].bytes() {
        // The closing `.` is the only thing that ends a card. Everything else
        // that is not a payload character -- newlines, the indentation mail
        // adds, the `>` a quoted reply puts at the start of every line -- is
        // skipped, because those are exactly what a card picks up in transit.
        // A character mangled rather than added would corrupt the payload
        // silently, so the card's signature is the real check.
        if byte == b'.' {
            break;
        }
        if let Some(value) = value(byte) {
            values.push(value);
        }
    }
    if values.len() < 2 {
        return Err("That Mesh card is empty or cut short. Copy all of it and try again.".into());
    }
    let mut bytes = Vec::with_capacity(values.len() * 3 / 4);
    for chunk in values.chunks(4) {
        if chunk.len() == 1 {
            return Err("That Mesh card is cut short. Copy all of it and try again.".into());
        }
        let mut bits = 0u32;
        for (index, value) in chunk.iter().enumerate() {
            bits |= value << (18 - index * 6);
        }
        for index in 0..chunk.len() - 1 {
            bytes.push(((bits >> (16 - index * 8)) & 0xff) as u8);
        }
    }
    if bytes.len() > crate::exchange::MAX_FILE_BYTES {
        return Err("That Mesh card is too large.".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_round_trips_including_the_short_final_chunk() {
        for length in 0..200usize {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 7 % 256) as u8).collect();
            let card = encode(&bytes);
            if length == 0 {
                assert!(decode(&card).is_err(), "an empty card is not a card");
                continue;
            }
            assert_eq!(decode(&card).unwrap(), bytes, "length {length}");
        }
    }

    #[test]
    fn a_card_pasted_inside_a_chat_message_still_decodes() {
        let bytes = b"{\"mesh_pool_file\":\"invitation\"}".to_vec();
        let card = encode(&bytes);
        for text in [
            format!("hey, here you go:\n\n{card}\n\nlet me know when you're on"),
            format!("Mic wrote:\n{card}"),
            format!("{card}-- \nSent from my phone"),
            // Indented, quoted, and re-wrapped by the chat client.
            card.replace('\n', "\n   "),
            card.replace('\n', "\n> "),
            card.replace('\n', " "),
        ] {
            assert_eq!(decode(&text).unwrap(), bytes, "failed for {text:?}");
        }
    }

    #[test]
    fn text_with_no_card_in_it_is_reported_not_guessed() {
        for text in ["", "hello", "MESH", "MESH1", "MESH1 A"] {
            assert!(decode(text).is_err(), "{text:?} is not a card");
        }
    }

    #[test]
    fn the_payload_avoids_characters_chat_clients_mangle() {
        let card = encode(&(0u8..=255).collect::<Vec<u8>>());
        assert!(!card.contains('+') && !card.contains('/') && !card.contains('='));
        assert!(card.lines().all(|line| line.len() <= LINE));
    }
}
