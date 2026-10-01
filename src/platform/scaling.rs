//! Read-only X11 desktop DPI discovery. No toolkit or helper process is needed.
//!
//! XSETTINGS Xft/DPI has priority over the root Xft.dpi resource. Both describe
//! effective font DPI, so they must not be multiplied by another toolkit scale.
//! Missing, malformed and unsupported values use 100 percent. Discovery is
//! screen-global; it does not claim per-monitor fractional Wayland support.

use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt as _},
};

const MAX_PROPERTY: usize = 64 * 1024;
const MAX_SETTINGS: u32 = 1024;

/// Read one bounded snapshot of the desktop's effective scale (75..=300 percent).
///
/// The normal X11 host may poll this; restricted workers never call it. An
/// XSETTINGS owner disappearing during the read is harmless: try Xresources.
/// Missing or invalid preferences resolve to 100; an invalid screen or failed
/// root-property request returns an error for the host to handle non-fatally.
pub fn read_scale<C: Connection>(conn: &C, screen_num: usize) -> Result<u16, String> {
    let screen = conn
        .setup()
        .roots
        .get(screen_num)
        .ok_or("Invalid X11 screen for display scaling")?;
    let settings = read_xsettings(conn, screen_num).ok().flatten();
    if let Some(scale) = settings {
        return Ok(scale);
    }
    let reply = conn
        .get_property(
            false,
            screen.root,
            AtomEnum::RESOURCE_MANAGER,
            AtomEnum::STRING,
            0,
            (MAX_PROPERTY / 4) as u32,
        )
        .map_err(|error| format!("Cannot read desktop DPI: {error}"))?
        .reply()
        .map_err(|error| format!("Cannot read desktop DPI: {error}"))?;
    let resources = if reply.format == 8 && reply.bytes_after == 0 {
        parse_resources(&reply.value)
    } else {
        None
    };
    Ok(select_scale(settings, resources))
}

fn read_xsettings<C: Connection>(
    conn: &C,
    screen_num: usize,
) -> Result<Option<u16>, Box<dyn std::error::Error>> {
    let selection = conn
        .intern_atom(true, format!("_XSETTINGS_S{screen_num}").as_bytes())?
        .reply()?
        .atom;
    if selection == x11rb::NONE {
        return Ok(None);
    }
    let owner = conn.get_selection_owner(selection)?.reply()?.owner;
    if owner == x11rb::NONE {
        return Ok(None);
    }
    let property = conn
        .intern_atom(true, b"_XSETTINGS_SETTINGS")?
        .reply()?
        .atom;
    if property == x11rb::NONE {
        return Ok(None);
    }
    let reply = conn
        .get_property(
            false,
            owner,
            property,
            property,
            0,
            (MAX_PROPERTY / 4) as u32,
        )?
        .reply()?;
    Ok(if reply.format == 8 && reply.bytes_after == 0 {
        parse_xsettings(&reply.value)
    } else {
        None
    })
}

fn select_scale(settings: Option<u16>, resources: Option<u16>) -> u16 {
    settings.or(resources).unwrap_or(100)
}

fn dpi_percent(dpi: f64) -> Option<u16> {
    let percent = dpi / 96.0 * 100.0;
    if percent.is_finite() && (75.0..=300.0).contains(&percent) {
        Some(percent.round() as u16)
    } else {
        None
    }
}

fn parse_resources(bytes: &[u8]) -> Option<u16> {
    if bytes.len() > MAX_PROPERTY {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut found = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('!') {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        // This is one explicit desktop resource, not an application Xrm merge.
        if name.trim() == "Xft.dpi" {
            if found.is_some() {
                return None;
            }
            found = Some(dpi_percent(value.trim().parse().ok()?)?);
        }
    }
    found
}

struct Cursor<'a> {
    rest: &'a [u8],
    little: bool,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let result = self.rest.get(..count)?;
        self.rest = &self.rest[count..];
        Some(result)
    }

    fn u16(&mut self) -> Option<u16> {
        let bytes = self.take(2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32(&mut self) -> Option<u32> {
        let bytes = self.take(4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    fn padded(&mut self, count: usize) -> Option<&'a [u8]> {
        let bytes = self.take(count)?;
        self.take((4 - count % 4) % 4)?;
        Some(bytes)
    }
}

fn parse_xsettings(bytes: &[u8]) -> Option<u16> {
    if bytes.len() > MAX_PROPERTY {
        return None;
    }
    let little = match bytes.first()? {
        0 => true,
        1 => false,
        _ => return None,
    };
    let mut cursor = Cursor {
        rest: bytes,
        little,
    };
    cursor.take(8)?; // Byte order, padding and serial.
    let count = cursor.u32()?;
    if count > MAX_SETTINGS {
        return None;
    }
    let mut dpi = None;
    for _ in 0..count {
        let kind = cursor.take(2)?[0];
        let length = cursor.u16()? as usize;
        let name = cursor.padded(length)?;
        cursor.take(4)?; // Last-change serial.
        match kind {
            0 => {
                let value = cursor.u32()? as i32;
                if name == b"Xft/DPI" {
                    if dpi.is_some() {
                        return None;
                    }
                    dpi = Some(value);
                }
            }
            1 => {
                let length = cursor.u32()? as usize;
                cursor.padded(length)?;
                if name == b"Xft/DPI" {
                    return None;
                }
            }
            2 => {
                cursor.take(8)?;
                if name == b"Xft/DPI" {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if !cursor.rest.is_empty() {
        return None;
    }
    dpi_percent(f64::from(dpi?) / 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(little: bool, value: i32) -> Vec<u8> {
        let mut data = vec![u8::from(!little), 0, 0, 0];
        let u32bytes = |value: u32| {
            if little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            }
        };
        data.extend(u32bytes(0));
        data.extend(u32bytes(1));
        data.extend([0, 0]);
        data.extend(if little {
            7u16.to_le_bytes()
        } else {
            7u16.to_be_bytes()
        });
        data.extend(b"Xft/DPI\0");
        data.extend(u32bytes(0));
        data.extend(u32bytes(value as u32));
        data
    }

    #[test]
    fn dpi_validation_and_priority_do_not_double_count() {
        for (dpi, expected) in [
            (72.0, 75),
            (96.0, 100),
            (120.0, 125),
            (192.0, 200),
            (288.0, 300),
        ] {
            assert_eq!(dpi_percent(dpi), Some(expected));
        }
        for dpi in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -96.0,
            0.0,
            71.9,
            288.1,
        ] {
            assert_eq!(dpi_percent(dpi), None);
        }
        assert_eq!(select_scale(Some(200), Some(200)), 200);
        assert_eq!(select_scale(Some(150), Some(200)), 150);
        assert_eq!(select_scale(None, Some(200)), 200);
        assert_eq!(select_scale(None, None), 100);
    }

    #[test]
    fn bounded_resources_accept_only_a_valid_explicit_dpi() {
        assert_eq!(
            parse_resources(b"! comment\nXft.dpi:\t192\nXcursor.size:48\n"),
            Some(200)
        );
        assert_eq!(parse_resources(b" Xft.dpi : 120 \r\n"), Some(125));
        for bytes in [
            &b""[..],
            &b"Xft.dpi: NaN"[..],
            &b"Xft.dpi: inf"[..],
            &b"Xft.dpi: 999"[..],
            &b"Xft.dpi: -1"[..],
            &b"Xft.dpi: 0"[..],
            &b"Xft.dpi: 192garbage"[..],
            &b"Xft.dpi: 192\nXft.dpi: 96"[..],
            &b"! Xft.dpi: 192"[..],
            &b"other: 192"[..],
            &b"\xff"[..],
        ] {
            assert_eq!(parse_resources(bytes), None, "{bytes:?}");
        }
        assert_eq!(parse_resources(&vec![b' '; MAX_PROPERTY + 1]), None);
    }

    #[test]
    fn xsettings_accepts_both_byte_orders_and_checks_dpi_range() {
        for little in [false, true] {
            assert_eq!(parse_xsettings(&settings(little, 192 * 1024)), Some(200));
            assert_eq!(parse_xsettings(&settings(little, 108 * 1024)), Some(113));
            for value in [-1, 0, 71 * 1024, 289 * 1024, i32::MAX, i32::MIN] {
                assert_eq!(parse_xsettings(&settings(little, value)), None);
            }
        }
    }

    #[test]
    fn unrelated_string_and_color_records_do_not_hide_valid_dpi() {
        let mut data = settings(true, 192 * 1024);
        data[8..12].copy_from_slice(&3u32.to_le_bytes());
        data.extend([1, 0, 3, 0]);
        data.extend(b"Foo\0");
        data.extend(0u32.to_le_bytes());
        data.extend(3u32.to_le_bytes());
        data.extend(b"bar\0");
        data.extend([2, 0, 5, 0]);
        data.extend(b"Color\0\0\0");
        data.extend(0u32.to_le_bytes());
        data.extend([0; 8]);
        assert_eq!(parse_xsettings(&data), Some(200));
        for end in 32..data.len() {
            assert_eq!(parse_xsettings(&data[..end]), None);
        }
        // An unrelated string claims more than the entire bounded property.
        data[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_xsettings(&data), None);
    }

    #[test]
    fn malformed_xsettings_is_bounded_and_never_uses_partial_data() {
        let good = settings(true, 192 * 1024);
        for end in 0..good.len() {
            assert_eq!(parse_xsettings(&good[..end]), None);
        }
        for (offset, value) in [(0, 2), (12, 9), (14, 255), (15, 255)] {
            let mut bad = good.clone();
            bad[offset] = value;
            assert_eq!(parse_xsettings(&bad), None);
        }
        let mut duplicate = good.clone();
        duplicate[8..12].copy_from_slice(&2u32.to_le_bytes());
        duplicate.extend(&good[12..]);
        assert_eq!(parse_xsettings(&duplicate), None);
        let mut extra = good.clone();
        extra.push(0);
        assert_eq!(parse_xsettings(&extra), None);
        let mut too_many = good.clone();
        too_many[8..12].copy_from_slice(&(MAX_SETTINGS + 1).to_le_bytes());
        assert_eq!(parse_xsettings(&too_many), None);
        assert_eq!(parse_xsettings(&vec![0; MAX_PROPERTY + 1]), None);
        for kind in [1, 2] {
            let mut wrong_type = good.clone();
            wrong_type[12] = kind;
            wrong_type.extend([0; 8]);
            assert_eq!(parse_xsettings(&wrong_type), None);
        }
    }
}
