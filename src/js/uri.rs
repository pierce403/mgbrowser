//! Bounded URI builtins over JavaScript UTF-16 strings, not form-urlencoded data.
//! Semantics follow ECMA-262 5.1 section 15.1.3; no I/O or third-party codec.

pub const MAX_UNITS: usize = 1024 * 1024;
pub const LIMIT_ERROR: &str = "JavaScript URI size limit exhausted";
const MALFORMED: &str = "URIError: malformed URI sequence";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn reserved(unit: u16) -> bool {
    unit < 128 && b";/?:@&=+$,#".contains(&(unit as u8))
}

fn unescaped(character: char, component: bool) -> bool {
    character.is_ascii_alphanumeric()
        || (character.is_ascii() && b"-_.!~*'()".contains(&(character as u8)))
        || (!component && character.is_ascii() && reserved(character as u16))
}

/// Preflight exact UTF-16 output length, so the evaluator can charge its budget
/// before allocating. Encode rejects unpaired input surrogates.
pub fn encoded_len(input: &[u16], component: bool) -> Result<usize, String> {
    if input.len() > MAX_UNITS {
        return Err(LIMIT_ERROR.into());
    }
    let mut length = 0usize;
    for character in char::decode_utf16(input.iter().copied()) {
        let character = character.map_err(|_| MALFORMED.to_string())?;
        length += if unescaped(character, component) {
            1
        } else {
            character.len_utf8() * 3
        };
        if length > MAX_UNITS {
            return Err(LIMIT_ERROR.into());
        }
    }
    Ok(length)
}

pub fn encode(input: &[u16], component: bool) -> Result<Vec<u16>, String> {
    let length = encoded_len(input, component)?;
    let mut output = Vec::with_capacity(length);
    for character in char::decode_utf16(input.iter().copied()) {
        let character = character.map_err(|_| MALFORMED.to_string())?;
        if unescaped(character, component) {
            output.push(character as u16);
            continue;
        }
        let mut bytes = [0; 4];
        for byte in character.encode_utf8(&mut bytes).bytes() {
            output.extend_from_slice(&[
                b'%' as u16,
                HEX[(byte >> 4) as usize] as u16,
                HEX[(byte & 15) as usize] as u16,
            ]);
        }
    }
    debug_assert_eq!(output.len(), length);
    Ok(output)
}

fn hex(unit: u16) -> Option<u8> {
    match unit {
        48..=57 => Some((unit - 48) as u8),
        65..=70 => Some((unit - 65 + 10) as u8),
        97..=102 => Some((unit - 97 + 10) as u8),
        _ => None,
    }
}

fn octet(input: &[u16], position: usize) -> Result<u8, String> {
    let triplet = input.get(position..position + 3).ok_or(MALFORMED)?;
    if triplet[0] != b'%' as u16 {
        return Err(MALFORMED.into());
    }
    let high = hex(triplet[1]).ok_or(MALFORMED)?;
    let low = hex(triplet[2]).ok_or(MALFORMED)?;
    Ok((high << 4) | low)
}

/// Decode never grows the input, and preserves ordinary UTF-16 code units,
/// including literal unpaired surrogates. Percent-encoded UTF-8 must be valid.
pub fn decode(input: &[u16], component: bool) -> Result<Vec<u16>, String> {
    if input.len() > MAX_UNITS {
        return Err(LIMIT_ERROR.into());
    }
    let mut output = Vec::with_capacity(input.len());
    let mut position = 0;
    while position < input.len() {
        if input[position] != b'%' as u16 {
            output.push(input[position]);
            position += 1;
            continue;
        }
        let first = octet(input, position)?;
        if first < 128 {
            if !component && reserved(first as u16) {
                output.extend_from_slice(&input[position..position + 3]);
            } else {
                output.push(first as u16);
            }
            position += 3;
            continue;
        }
        let count = match first {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(MALFORMED.into()),
        };
        let mut bytes = [0u8; 4];
        bytes[0] = first;
        for (index, byte) in bytes.iter_mut().enumerate().take(count).skip(1) {
            *byte = octet(input, position + index * 3)?;
        }
        // Rust's strict UTF-8 validation also rejects overlong sequences,
        // encoded surrogates, invalid continuations and values above U+10FFFF.
        let text = std::str::from_utf8(&bytes[..count]).map_err(|_| MALFORMED.to_string())?;
        output.extend(text.encode_utf16());
        position += count * 3;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }
    fn encoded(text: &str, component: bool) -> String {
        String::from_utf16(&encode(&units(text), component).unwrap()).unwrap()
    }
    fn decoded(text: &str, component: bool) -> String {
        String::from_utf16(&decode(&units(text), component).unwrap()).unwrap()
    }

    #[test]
    fn component_and_whole_uri_keep_distinct_reserved_sets() {
        let raw = "https://example.test/a b?q=Rust & café#part";
        assert_eq!(
            encoded(raw, false),
            "https://example.test/a%20b?q=Rust%20&%20caf%C3%A9#part"
        );
        assert_eq!(
            encoded(raw, true),
            "https%3A%2F%2Fexample.test%2Fa%20b%3Fq%3DRust%20%26%20caf%C3%A9%23part"
        );
        assert_eq!(encoded("AZaz09-_.!~*'()", true), "AZaz09-_.!~*'()");
        assert_eq!(encoded("[]%", false), "%5B%5D%25");
        assert_eq!(
            decoded("%2f%3F%23%26%2b%41%20%25", false),
            "%2f%3F%23%26%2bA %"
        );
        assert_eq!(decoded("%2f%3F%23%26%2b%41%20%25", true), "/?#&+A %");
        assert_eq!(decoded("a+b%252f", true), "a+b%2f");
    }

    #[test]
    fn unicode_scalars_round_trip_including_astral_and_nul() {
        assert_eq!(encoded("€😀\0", true), "%E2%82%AC%F0%9F%98%80%00");
        assert_eq!(decoded("%e2%82%ac%f0%9f%98%80%00", true), "€😀\0");
        for value in (0..128).chain([
            0x80, 0x7ff, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff,
        ]) {
            let mut storage = [0; 2];
            let text = char::from_u32(value).unwrap().encode_utf16(&mut storage);
            for component in [true, false] {
                let encoded = encode(text, component).unwrap();
                assert_eq!(encoded.len(), encoded_len(text, component).unwrap());
                assert_eq!(decode(&encoded, component).unwrap(), text);
            }
        }
    }

    #[test]
    fn malformed_percent_utf8_never_decodes_to_replacement_or_prefix() {
        for input in [
            "%",
            "%0",
            "%gg",
            "%é0",
            "%80",
            "%C0%80",
            "%C1%BF",
            "%C2",
            "%C2A0",
            "%C2%20",
            "%E0%80%80",
            "%ED%A0%80",
            "%F0%80%80%80",
            "%F4%90%80%80",
            "%F5%80%80%80",
            "%FE",
            "okay%FF",
        ] {
            for component in [true, false] {
                assert_eq!(
                    decode(&units(input), component).unwrap_err(),
                    MALFORMED,
                    "{input}"
                );
            }
        }
    }

    #[test]
    fn encoding_rejects_unpaired_surrogates_but_decoding_preserves_literals() {
        for input in [
            vec![0xd800],
            vec![0xdc00],
            vec![0xd800, b'a' as u16],
            vec![0xdc00, 0xd800],
        ] {
            for component in [true, false] {
                assert_eq!(encode(&input, component).unwrap_err(), MALFORMED);
                assert_eq!(decode(&input, component).unwrap(), input);
            }
        }
    }

    #[test]
    fn input_and_expansion_are_preflight_bounded() {
        let excessive = vec![b'a' as u16; MAX_UNITS + 1];
        assert_eq!(encode(&excessive, true).unwrap_err(), LIMIT_ERROR);
        assert_eq!(decode(&excessive, true).unwrap_err(), LIMIT_ERROR);
        let expanding = vec![b' ' as u16; MAX_UNITS / 3 + 1];
        assert_eq!(encoded_len(&expanding, true).unwrap_err(), LIMIT_ERROR);
        assert_eq!(encode(&expanding, true).unwrap_err(), LIMIT_ERROR);
        assert!(encode(&[], true).unwrap().is_empty());
        assert!(decode(&[], false).unwrap().is_empty());
    }
}
