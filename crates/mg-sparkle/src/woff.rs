//! Pure-Rust WOFF 1.0 (`wOFF`) and WOFF 2.0 (`wOF2`) SFNT reconstruction.
//!
//! All decompression and table reconstruction runs in safe Rust with bounded
//! output and scratch allocations before `fontdue` or `rustybuzz` parses the
//! resulting OpenType/TrueType SFNT bytes.
#![forbid(unsafe_code)]

use std::io::Read;

use brotli_decompressor::{
    Allocator, BrotliDecompressStream, BrotliResult, BrotliState, HuffmanCode, SliceWrapper,
    SliceWrapperMut,
};
use flate2::read::ZlibDecoder;

/// Maximum reconstructed SFNT or uncompressed WOFF2 table stream size (4 MiB).
const MAX_DECOMPRESSED_FONT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum cumulative byte-slice allocation granted to a single Brotli state.
const MAX_BROTLI_U8_ALLOC_BYTES: usize = 8 * 1024 * 1024;
const MAX_TABLES: usize = 256;

/// W3C WOFF2 §4.1 known table tag directory (indices `0..63`).
const KNOWN_TAGS: [[u8; 4]; 63] = [
    *b"cmap", *b"head", *b"hhea", *b"hmtx", *b"maxp", *b"name", *b"OS/2", *b"post", *b"cvt ",
    *b"fpgm", *b"glyf", *b"loca", *b"prep", *b"CFF ", *b"VORG", *b"EBDT", *b"EBLC", *b"gasp",
    *b"hdmx", *b"kern", *b"LTSH", *b"PCLT", *b"VDMX", *b"vhea", *b"vmtx", *b"BASE", *b"GDEF",
    *b"GPOS", *b"GSUB", *b"EBSC", *b"JSTF", *b"MATH", *b"CBDT", *b"CBLC", *b"COLR", *b"CPAL",
    *b"SVG ", *b"sbix", *b"acnt", *b"avar", *b"bdat", *b"bloc", *b"bsln", *b"cvar", *b"fdsc",
    *b"feat", *b"fmtx", *b"fvar", *b"gvar", *b"hsty", *b"just", *b"lcar", *b"mort", *b"morx",
    *b"opbd", *b"prop", *b"trak", *b"Zapf", *b"Silf", *b"Glat", *b"Gloc", *b"Feat", *b"Sill",
];

pub(crate) fn decode_font_bytes(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(magic) = bytes.get(..4) else {
        return Err("Font data is truncated".into());
    };
    match magic {
        b"\x00\x01\x00\x00" | b"OTTO" | b"true" | b"typ1" | b"ttcf" => Ok(bytes),
        b"wOFF" => decode_woff1(&bytes),
        b"wOF2" => decode_woff2(&bytes),
        _ => Err("Unsupported font container signature".into()),
    }
}

fn valid_sfnt_flavor(flavor: u32) -> bool {
    matches!(
        &flavor.to_be_bytes(),
        b"\x00\x01\x00\x00" | b"OTTO" | b"true" | b"typ1"
    )
}

#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| "Unexpected end of font stream".to_owned())?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let b = self.read_bytes(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_uint_base128(&mut self) -> Result<u32, String> {
        let mut accum = 0u32;
        for i in 0..5 {
            let b = self.read_u8()?;
            if i == 0 && b == 0x80 {
                return Err("Invalid leading zero in WOFF2 UIntBase128".into());
            }
            if accum & 0xfe00_0000 != 0 {
                return Err("WOFF2 UIntBase128 overflow".into());
            }
            accum = (accum << 7) | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(accum);
            }
        }
        Err("Unterminated WOFF2 UIntBase128".into())
    }

    fn read_255_ushort(&mut self) -> Result<u16, String> {
        match self.read_u8()? {
            253 => self.read_u16(),
            254 => Ok(506u16 + u16::from(self.read_u8()?)),
            255 => Ok(253u16 + u16::from(self.read_u8()?)),
            code => Ok(u16::from(code)),
        }
    }
}

fn decode_woff1(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 44 {
        return Err("WOFF1 header is truncated".into());
    }
    let mut r = Reader::new(bytes);
    let _sig = r.read_bytes(4)?;
    let flavor = r.read_u32()?;
    if !valid_sfnt_flavor(flavor) {
        return Err("Unsupported WOFF1 SFNT flavor".into());
    }
    let length = r.read_u32()? as usize;
    if length != bytes.len() {
        return Err("WOFF1 length header mismatch".into());
    }
    let num_tables = r.read_u16()? as usize;
    let reserved = r.read_u16()?;
    if reserved != 0 || num_tables == 0 || num_tables > MAX_TABLES {
        return Err("Invalid WOFF1 header fields".into());
    }
    let total_sfnt_size = r.read_u32()? as usize;
    if total_sfnt_size > MAX_DECOMPRESSED_FONT_BYTES {
        return Err("WOFF1 decompressed font exceeds 4 MiB limit".into());
    }
    let _ = r.read_bytes(24)?;

    let mut total_uncompressed = 12usize + num_tables * 16;
    let mut tables = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let tag_slice = r.read_bytes(4)?;
        let tag = [tag_slice[0], tag_slice[1], tag_slice[2], tag_slice[3]];
        let offset = r.read_u32()? as usize;
        let comp_len = r.read_u32()? as usize;
        let orig_len = r.read_u32()? as usize;
        let _orig_checksum = r.read_u32()?;
        if comp_len > orig_len || orig_len > MAX_DECOMPRESSED_FONT_BYTES {
            return Err("Invalid WOFF1 table lengths".into());
        }
        let padded = (orig_len + 3) & !3;
        total_uncompressed = total_uncompressed
            .checked_add(padded)
            .filter(|&n| n <= MAX_DECOMPRESSED_FONT_BYTES)
            .ok_or_else(|| "WOFF1 tables exceed 4 MiB limit".to_owned())?;
        let end = offset
            .checked_add(comp_len)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| "WOFF1 table slice out of bounds".to_owned())?;
        let raw = &bytes[offset..end];
        let table_bytes = if comp_len == orig_len {
            raw.to_vec()
        } else {
            let mut decoded = Vec::with_capacity(orig_len);
            ZlibDecoder::new(raw)
                .take((orig_len as u64) + 1)
                .read_to_end(&mut decoded)
                .map_err(|e| format!("Invalid WOFF1 zlib stream: {e}"))?;
            if decoded.len() != orig_len {
                return Err("WOFF1 table decompressed length mismatch".into());
            }
            decoded
        };
        tables.push((tag, table_bytes));
    }
    build_sfnt(flavor, tables, None)
}

struct VecCell<T>(Vec<T>);

impl<T> Default for VecCell<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> std::ops::Index<usize> for VecCell<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        &self.0[index]
    }
}

impl<T> std::ops::IndexMut<usize> for VecCell<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.0[index]
    }
}

impl<T> SliceWrapper<T> for VecCell<T> {
    fn slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> SliceWrapperMut<T> for VecCell<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

struct BoundedU8Alloc {
    used: usize,
}

impl Allocator<u8> for BoundedU8Alloc {
    type AllocatedMemory = VecCell<u8>;
    fn alloc_cell(&mut self, len: usize) -> VecCell<u8> {
        let Some(next) = self.used.checked_add(len) else {
            return VecCell(Vec::new());
        };
        if next > MAX_BROTLI_U8_ALLOC_BYTES {
            return VecCell(Vec::new());
        }
        self.used = next;
        VecCell(vec![0u8; len])
    }
    fn free_cell(&mut self, data: VecCell<u8>) {
        self.used = self.used.saturating_sub(data.0.len());
    }
}

struct DefaultVecAlloc<T: Clone + Default>(std::marker::PhantomData<T>);

impl<T: Clone + Default> DefaultVecAlloc<T> {
    fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T: Clone + Default> Allocator<T> for DefaultVecAlloc<T> {
    type AllocatedMemory = VecCell<T>;
    fn alloc_cell(&mut self, len: usize) -> VecCell<T> {
        VecCell(vec![T::default(); len])
    }
    fn free_cell(&mut self, _data: VecCell<T>) {}
}

fn brotli_decompress_bounded(compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, String> {
    if expected_len > MAX_DECOMPRESSED_FONT_BYTES {
        return Err("WOFF2 stream exceeds 4 MiB limit".into());
    }
    let mut state = BrotliState::new_strict(
        BoundedU8Alloc { used: 0 },
        DefaultVecAlloc::<u32>::new(),
        DefaultVecAlloc::<HuffmanCode>::new(),
    );
    let mut out = vec![0u8; expected_len];
    let mut available_in = compressed.len();
    let mut input_offset = 0usize;
    let mut available_out = out.len();
    let mut output_offset = 0usize;
    let mut total_out = 0usize;
    loop {
        match BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            compressed,
            &mut available_out,
            &mut output_offset,
            &mut out,
            &mut total_out,
            &mut state,
        ) {
            BrotliResult::ResultSuccess => {
                if output_offset != expected_len {
                    return Err("WOFF2 Brotli output length mismatch".into());
                }
                return Ok(out);
            }
            BrotliResult::NeedsMoreOutput => {
                return Err("WOFF2 Brotli stream exceeds declared table length".into());
            }
            BrotliResult::NeedsMoreInput => {
                return Err("Truncated WOFF2 Brotli stream".into());
            }
            BrotliResult::ResultFailure => {
                return Err("Malformed or oversized WOFF2 Brotli stream".into());
            }
        }
    }
}

struct Woff2TableEntry {
    tag: [u8; 4],
    is_transformed: bool,
    stream_offset: usize,
    transform_len: usize,
}

fn decode_woff2(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 48 {
        return Err("WOFF2 header is truncated".into());
    }
    let mut r = Reader::new(bytes);
    let _sig = r.read_bytes(4)?;
    let flavor = r.read_u32()?;
    if &flavor.to_be_bytes() == b"ttcf" {
        return Err("WOFF2 font collections are not supported".into());
    }
    if !valid_sfnt_flavor(flavor) {
        return Err("Unsupported WOFF2 SFNT flavor".into());
    }
    let length = r.read_u32()? as usize;
    if length < 48 || length > bytes.len() {
        return Err("WOFF2 length header mismatch".into());
    }
    let num_tables = r.read_u16()? as usize;
    let reserved = r.read_u16()?;
    if reserved != 0 || num_tables == 0 || num_tables > MAX_TABLES {
        return Err("Invalid WOFF2 header fields".into());
    }
    let _total_sfnt_size = r.read_u32()?;
    let total_compressed_size = r.read_u32()? as usize;
    let _ = r.read_bytes(24)?;

    let mut entries = Vec::with_capacity(num_tables);
    let mut stream_total = 0usize;
    let mut glyf_transformed = false;
    let mut loca_transformed = false;
    let mut has_glyf = false;
    let mut has_loca = false;

    for _ in 0..num_tables {
        let flags = r.read_u8()?;
        let tag_idx = flags & 0x3f;
        let tag = if tag_idx == 63 {
            let raw = r.read_bytes(4)?;
            [raw[0], raw[1], raw[2], raw[3]]
        } else {
            KNOWN_TAGS[usize::from(tag_idx)]
        };
        let xform_version = (flags >> 6) & 0x03;
        let is_transformed = match &tag {
            b"glyf" | b"loca" => match xform_version {
                0 => true,
                3 => false,
                _ => return Err("Invalid WOFF2 glyf/loca transform version".into()),
            },
            b"hmtx" => match xform_version {
                0 => false,
                1 => true,
                _ => return Err("Invalid WOFF2 hmtx transform version".into()),
            },
            _ => {
                if xform_version != 0 {
                    return Err("Unsupported WOFF2 table transform".into());
                }
                false
            }
        };
        let orig_len = r.read_uint_base128()? as usize;
        let transform_len = if is_transformed {
            r.read_uint_base128()? as usize
        } else {
            orig_len
        };
        if orig_len > MAX_DECOMPRESSED_FONT_BYTES || transform_len > MAX_DECOMPRESSED_FONT_BYTES {
            return Err("WOFF2 table exceeds 4 MiB limit".into());
        }
        if &tag == b"glyf" {
            has_glyf = true;
            glyf_transformed = is_transformed;
        } else if &tag == b"loca" {
            has_loca = true;
            loca_transformed = is_transformed;
            if is_transformed && transform_len != 0 {
                return Err("Transformed WOFF2 loca table must have zero stream length".into());
            }
        }
        let stream_offset = stream_total;
        stream_total = stream_total
            .checked_add(transform_len)
            .filter(|&n| n <= MAX_DECOMPRESSED_FONT_BYTES)
            .ok_or_else(|| "WOFF2 uncompressed stream exceeds 4 MiB limit".to_owned())?;
        entries.push(Woff2TableEntry {
            tag,
            is_transformed,
            stream_offset,
            transform_len,
        });
    }

    if glyf_transformed != loca_transformed || (glyf_transformed && (!has_glyf || !has_loca)) {
        return Err("Mismatched WOFF2 glyf/loca transform state".into());
    }

    let compressed = r.read_bytes(total_compressed_size)?;
    if r.pos > length {
        return Err("WOFF2 compressed data exceeds declared file length".into());
    }
    let stream = brotli_decompress_bounded(compressed, stream_total)?;

    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(num_tables);
    let mut reconstructed_index_format = None;
    let mut glyph_xmins: Option<Vec<i16>> = None;
    let mut deferred_hmtx: Option<&[u8]> = None;

    if glyf_transformed {
        let glyf_entry = entries
            .iter()
            .find(|e| &e.tag == b"glyf")
            .ok_or_else(|| "Missing WOFF2 glyf table".to_owned())?;
        let raw_glyf =
            &stream[glyf_entry.stream_offset..glyf_entry.stream_offset + glyf_entry.transform_len];
        let (glyf_bytes, loca_bytes, index_format, xmins) = reconstruct_glyf_loca(raw_glyf)?;
        reconstructed_index_format = Some(index_format);
        glyph_xmins = Some(xmins);
        for entry in &entries {
            let slice = &stream[entry.stream_offset..entry.stream_offset + entry.transform_len];
            if &entry.tag == b"glyf" {
                tables.push((*b"glyf", glyf_bytes.clone()));
            } else if &entry.tag == b"loca" {
                tables.push((*b"loca", loca_bytes.clone()));
            } else if &entry.tag == b"hmtx" && entry.is_transformed {
                deferred_hmtx = Some(slice);
            } else {
                tables.push((entry.tag, slice.to_vec()));
            }
        }
    } else {
        for entry in &entries {
            let slice = &stream[entry.stream_offset..entry.stream_offset + entry.transform_len];
            if &entry.tag == b"hmtx" && entry.is_transformed {
                deferred_hmtx = Some(slice);
            } else {
                tables.push((entry.tag, slice.to_vec()));
            }
        }
    }

    if let Some(hmtx_stream) = deferred_hmtx {
        let maxp = tables
            .iter()
            .find(|(t, _)| t == b"maxp")
            .ok_or_else(|| "Missing maxp for transformed WOFF2 hmtx".to_owned())?;
        let hhea = tables
            .iter()
            .find(|(t, _)| t == b"hhea")
            .ok_or_else(|| "Missing hhea for transformed WOFF2 hmtx".to_owned())?;
        if maxp.1.len() < 6 || hhea.1.len() < 36 {
            return Err("Truncated maxp/hhea for transformed WOFF2 hmtx".into());
        }
        let num_glyphs = u16::from_be_bytes([maxp.1[4], maxp.1[5]]) as usize;
        let num_h_metrics = u16::from_be_bytes([hhea.1[34], hhea.1[35]]) as usize;
        let xmins = match glyph_xmins {
            Some(xmins) => xmins,
            None => extract_untransformed_xmins(&tables, num_glyphs)?,
        };
        let hmtx_bytes = reconstruct_hmtx(hmtx_stream, num_glyphs, num_h_metrics, &xmins)?;
        tables.push((*b"hmtx", hmtx_bytes));
    }

    build_sfnt(flavor, tables, reconstructed_index_format)
}

fn decode_xy_triplet(flag: u8, glyph_stream: &mut Reader<'_>) -> Result<(i32, i32), String> {
    let f = flag & 0x7f;
    let (dx_mag, dy_mag, x_neg, y_neg) = if f < 10 {
        let b0 = i32::from(glyph_stream.read_u8()?);
        (0, ((i32::from(f >> 1)) << 8) + b0, false, (f & 1) == 0)
    } else if f < 20 {
        let k = f - 10;
        let b0 = i32::from(glyph_stream.read_u8()?);
        (((i32::from(k >> 1)) << 8) + b0, 0, (k & 1) == 0, false)
    } else if f < 84 {
        let k = f - 20;
        let b0 = glyph_stream.read_u8()?;
        let dx = 1 + i32::from(k & 0x30) + i32::from(b0 >> 4);
        let dy = 1 + (i32::from((k >> 2) & 3) << 4) + i32::from(b0 & 0x0f);
        (dx, dy, (k & 1) == 0, (k & 2) == 0)
    } else if f < 120 {
        let k = f - 84;
        let b = glyph_stream.read_bytes(2)?;
        let dx = 1 + (i32::from(k / 12) << 8) + i32::from(b[0]);
        let dy = 1 + (i32::from((k % 12) >> 2) << 8) + i32::from(b[1]);
        (dx, dy, (k & 1) == 0, (k & 2) == 0)
    } else if f < 124 {
        let k = f - 120;
        let b = glyph_stream.read_bytes(3)?;
        let dx = (i32::from(b[0]) << 4) | (i32::from(b[1]) >> 4);
        let dy = ((i32::from(b[1] & 0x0f)) << 8) | i32::from(b[2]);
        (dx, dy, (k & 1) == 0, (k & 2) == 0)
    } else {
        let k = f - 124;
        let b = glyph_stream.read_bytes(4)?;
        let dx = (i32::from(b[0]) << 8) | i32::from(b[1]);
        let dy = (i32::from(b[2]) << 8) | i32::from(b[3]);
        (dx, dy, (k & 1) == 0, (k & 2) == 0)
    };
    let dx = if x_neg { -dx_mag } else { dx_mag };
    let dy = if y_neg { -dy_mag } else { dy_mag };
    Ok((dx, dy))
}

fn reconstruct_glyf_loca(raw: &[u8]) -> Result<(Vec<u8>, Vec<u8>, u16, Vec<i16>), String> {
    let mut header = Reader::new(raw);
    let reserved = header.read_u16()?;
    if reserved != 0 {
        return Err("Nonzero WOFF2 glyf reserved header".into());
    }
    let option_flags = header.read_u16()?;
    let num_glyphs = header.read_u16()? as usize;
    let index_format = header.read_u16()?;
    if index_format > 1 {
        return Err("Invalid WOFF2 loca indexFormat".into());
    }
    let n_contour_size = header.read_u32()? as usize;
    let n_points_size = header.read_u32()? as usize;
    let flag_size = header.read_u32()? as usize;
    let glyph_size = header.read_u32()? as usize;
    let composite_size = header.read_u32()? as usize;
    let bbox_size = header.read_u32()? as usize;
    let instruction_size = header.read_u32()? as usize;

    let mut n_contour_stream = Reader::new(header.read_bytes(n_contour_size)?);
    let mut n_points_stream = Reader::new(header.read_bytes(n_points_size)?);
    let mut flag_stream = Reader::new(header.read_bytes(flag_size)?);
    let mut glyph_stream = Reader::new(header.read_bytes(glyph_size)?);
    let mut composite_stream = Reader::new(header.read_bytes(composite_size)?);
    let mut bbox_stream = Reader::new(header.read_bytes(bbox_size)?);
    let mut instruction_stream = Reader::new(header.read_bytes(instruction_size)?);
    let overlap_bitmap = if option_flags & 1 != 0 {
        Some(header.read_bytes((num_glyphs + 7) >> 3)?)
    } else {
        None
    };

    let bbox_bitmap_len = ((num_glyphs + 31) >> 5) << 2;
    let bbox_bitmap = bbox_stream.read_bytes(bbox_bitmap_len)?;

    let mut glyf_out = Vec::new();
    let mut offsets = Vec::with_capacity(num_glyphs + 1);
    let mut xmins = Vec::with_capacity(num_glyphs);

    let mut end_pts = Vec::new();
    let mut points: Vec<(i16, i16, bool)> = Vec::new();
    let mut tt_flags: Vec<u8> = Vec::new();
    let mut x_bytes: Vec<u8> = Vec::new();
    let mut y_bytes: Vec<u8> = Vec::new();

    for gid in 0..num_glyphs {
        offsets.push(glyf_out.len());
        let has_bbox = (bbox_bitmap[gid >> 3] & (0x80 >> (gid & 7))) != 0;
        let n_contours = n_contour_stream.read_i16()?;

        if n_contours == 0 {
            if has_bbox {
                return Err("Empty WOFF2 glyph must not declare an explicit bbox".into());
            }
            xmins.push(0);
            continue;
        }

        if n_contours < 0 {
            if !has_bbox {
                return Err("Composite WOFF2 glyph missing required explicit bbox".into());
            }
            let x_min = bbox_stream.read_i16()?;
            let y_min = bbox_stream.read_i16()?;
            let x_max = bbox_stream.read_i16()?;
            let y_max = bbox_stream.read_i16()?;
            xmins.push(x_min);

            let comp_start = composite_stream.pos;
            let mut have_instructions = false;
            loop {
                let comp_flags = composite_stream.read_u16()?;
                let _component_gid = composite_stream.read_u16()?;
                if comp_flags & 0x0100 != 0 {
                    have_instructions = true;
                }
                let arg_len = if comp_flags & 0x0001 != 0 { 4 } else { 2 };
                let xform_len = if comp_flags & 0x0008 != 0 {
                    2
                } else if comp_flags & 0x0040 != 0 {
                    4
                } else if comp_flags & 0x0080 != 0 {
                    8
                } else {
                    0
                };
                let _ = composite_stream.read_bytes(arg_len + xform_len)?;
                if comp_flags & 0x0020 == 0 {
                    break;
                }
            }
            let comp_bytes = &composite_stream.data[comp_start..composite_stream.pos];
            let instructions = if have_instructions {
                let inst_len = glyph_stream.read_255_ushort()? as usize;
                Some(instruction_stream.read_bytes(inst_len)?)
            } else {
                None
            };

            glyf_out.extend_from_slice(&(-1i16).to_be_bytes());
            for v in [x_min, y_min, x_max, y_max] {
                glyf_out.extend_from_slice(&v.to_be_bytes());
            }
            glyf_out.extend_from_slice(comp_bytes);
            if let Some(inst) = instructions {
                glyf_out.extend_from_slice(&(inst.len() as u16).to_be_bytes());
                glyf_out.extend_from_slice(inst);
            }
        } else {
            let contour_count = n_contours as usize;
            end_pts.clear();
            end_pts.reserve(contour_count);
            let mut total_points = 0usize;
            for _ in 0..contour_count {
                let pts = n_points_stream.read_255_ushort()? as usize;
                if pts == 0 {
                    return Err("WOFF2 simple contour has zero points".into());
                }
                total_points = total_points
                    .checked_add(pts)
                    .filter(|&n| n <= usize::from(u16::MAX) + 1)
                    .ok_or_else(|| "WOFF2 simple glyph point count overflow".to_owned())?;
                end_pts.push((total_points - 1) as u16);
            }

            let raw_flags = flag_stream.read_bytes(total_points)?;
            points.clear();
            points.reserve(total_points);
            let mut x = 0i32;
            let mut y = 0i32;
            let mut calc_min_x = i16::MAX;
            let mut calc_min_y = i16::MAX;
            let mut calc_max_x = i16::MIN;
            let mut calc_max_y = i16::MIN;

            for &flag in raw_flags {
                let (dx, dy) = decode_xy_triplet(flag, &mut glyph_stream)?;
                x = x.wrapping_add(dx);
                y = y.wrapping_add(dy);
                let px = x as i16;
                let py = y as i16;
                calc_min_x = calc_min_x.min(px);
                calc_min_y = calc_min_y.min(py);
                calc_max_x = calc_max_x.max(px);
                calc_max_y = calc_max_y.max(py);
                points.push((px, py, (flag & 0x80) == 0));
            }

            let inst_len = glyph_stream.read_255_ushort()? as usize;
            let instructions = instruction_stream.read_bytes(inst_len)?;

            let (x_min, y_min, x_max, y_max) = if has_bbox {
                (
                    bbox_stream.read_i16()?,
                    bbox_stream.read_i16()?,
                    bbox_stream.read_i16()?,
                    bbox_stream.read_i16()?,
                )
            } else {
                (calc_min_x, calc_min_y, calc_max_x, calc_max_y)
            };
            xmins.push(x_min);

            let overlap_simple =
                overlap_bitmap.is_some_and(|bm| (bm[gid >> 3] & (0x80 >> (gid & 7))) != 0);

            tt_flags.clear();
            x_bytes.clear();
            y_bytes.clear();
            let mut prev_x = 0i16;
            let mut prev_y = 0i16;
            for (idx, &(px, py, on_curve)) in points.iter().enumerate() {
                let dx = px.wrapping_sub(prev_x);
                let dy = py.wrapping_sub(prev_y);
                prev_x = px;
                prev_y = py;
                let mut f = u8::from(on_curve);
                if idx == 0 && overlap_simple {
                    f |= 0x40;
                }
                if dx == 0 {
                    f |= 0x10;
                } else if (-255..=255).contains(&dx) {
                    f |= 0x02;
                    if dx > 0 {
                        f |= 0x10;
                    }
                    x_bytes.push(dx.unsigned_abs() as u8);
                } else {
                    x_bytes.extend_from_slice(&dx.to_be_bytes());
                }
                if dy == 0 {
                    f |= 0x20;
                } else if (-255..=255).contains(&dy) {
                    f |= 0x04;
                    if dy > 0 {
                        f |= 0x20;
                    }
                    y_bytes.push(dy.unsigned_abs() as u8);
                } else {
                    y_bytes.extend_from_slice(&dy.to_be_bytes());
                }
                tt_flags.push(f);
            }

            glyf_out.extend_from_slice(&n_contours.to_be_bytes());
            for v in [x_min, y_min, x_max, y_max] {
                glyf_out.extend_from_slice(&v.to_be_bytes());
            }
            for &end_pt in &end_pts {
                glyf_out.extend_from_slice(&end_pt.to_be_bytes());
            }
            glyf_out.extend_from_slice(&(instructions.len() as u16).to_be_bytes());
            glyf_out.extend_from_slice(instructions);
            let mut idx = 0;
            while idx < tt_flags.len() {
                let f = tt_flags[idx];
                let mut repeat = 0u8;
                while idx + 1 + usize::from(repeat) < tt_flags.len()
                    && tt_flags[idx + 1 + usize::from(repeat)] == f
                    && repeat < 255
                {
                    repeat += 1;
                }
                if repeat > 0 {
                    glyf_out.push(f | 0x08);
                    glyf_out.push(repeat);
                    idx += 1 + usize::from(repeat);
                } else {
                    glyf_out.push(f);
                    idx += 1;
                }
            }
            glyf_out.extend_from_slice(&x_bytes);
            glyf_out.extend_from_slice(&y_bytes);
        }

        while glyf_out.len() % 2 != 0 {
            glyf_out.push(0);
        }
        if glyf_out.len() > MAX_DECOMPRESSED_FONT_BYTES {
            return Err("Reconstructed WOFF2 glyf table exceeds 4 MiB limit".into());
        }
    }

    offsets.push(glyf_out.len());
    let effective_index_format = if index_format == 0 && glyf_out.len() / 2 <= usize::from(u16::MAX)
    {
        0u16
    } else {
        1u16
    };
    let mut loca_out =
        Vec::with_capacity(offsets.len() * if effective_index_format == 0 { 2 } else { 4 });
    if effective_index_format == 0 {
        for off in offsets {
            loca_out.extend_from_slice(&((off / 2) as u16).to_be_bytes());
        }
    } else {
        for off in offsets {
            loca_out.extend_from_slice(&(off as u32).to_be_bytes());
        }
    }

    Ok((glyf_out, loca_out, effective_index_format, xmins))
}

fn extract_untransformed_xmins(
    tables: &[([u8; 4], Vec<u8>)],
    num_glyphs: usize,
) -> Result<Vec<i16>, String> {
    let head = tables
        .iter()
        .find(|(t, _)| t == b"head")
        .ok_or_else(|| "Missing head table".to_owned())?;
    let loca = tables
        .iter()
        .find(|(t, _)| t == b"loca")
        .ok_or_else(|| "Missing loca table".to_owned())?;
    let glyf = tables
        .iter()
        .find(|(t, _)| t == b"glyf")
        .ok_or_else(|| "Missing glyf table".to_owned())?;
    if head.1.len() < 52 {
        return Err("Truncated head table".into());
    }
    let index_format = i16::from_be_bytes([head.1[50], head.1[51]]);
    let mut xmins = Vec::with_capacity(num_glyphs);
    for gid in 0..num_glyphs {
        let (start, end) = if index_format == 0 {
            let need = (gid + 2) * 2;
            if loca.1.len() < need {
                return Err("Truncated short loca table".into());
            }
            let s = usize::from(u16::from_be_bytes([loca.1[gid * 2], loca.1[gid * 2 + 1]])) * 2;
            let e = usize::from(u16::from_be_bytes([
                loca.1[gid * 2 + 2],
                loca.1[gid * 2 + 3],
            ])) * 2;
            (s, e)
        } else {
            let need = (gid + 2) * 4;
            if loca.1.len() < need {
                return Err("Truncated long loca table".into());
            }
            let s = u32::from_be_bytes([
                loca.1[gid * 4],
                loca.1[gid * 4 + 1],
                loca.1[gid * 4 + 2],
                loca.1[gid * 4 + 3],
            ]) as usize;
            let e = u32::from_be_bytes([
                loca.1[gid * 4 + 4],
                loca.1[gid * 4 + 5],
                loca.1[gid * 4 + 6],
                loca.1[gid * 4 + 7],
            ]) as usize;
            (s, e)
        };
        if start == end {
            xmins.push(0);
        } else if start + 4 <= end && end <= glyf.1.len() {
            xmins.push(i16::from_be_bytes([glyf.1[start + 2], glyf.1[start + 3]]));
        } else {
            return Err("Invalid glyph slice for hmtx reconstruction".into());
        }
    }
    Ok(xmins)
}

fn reconstruct_hmtx(
    raw: &[u8],
    num_glyphs: usize,
    num_h_metrics: usize,
    xmins: &[i16],
) -> Result<Vec<u8>, String> {
    if num_h_metrics == 0 || num_h_metrics > num_glyphs || xmins.len() < num_glyphs {
        return Err("Invalid glyph counts for WOFF2 hmtx reconstruction".into());
    }
    let mut r = Reader::new(raw);
    let flags = r.read_u8()?;
    if flags & 0xfc != 0 {
        return Err("Invalid WOFF2 hmtx transform flags".into());
    }
    let has_prop_lsbs = (flags & 1) == 0;
    let has_mono_lsbs = (flags & 2) == 0;
    let mut advances = Vec::with_capacity(num_h_metrics);
    for _ in 0..num_h_metrics {
        advances.push(r.read_u16()?);
    }
    let mut out = Vec::with_capacity(num_h_metrics * 4 + (num_glyphs - num_h_metrics) * 2);
    for (gid, &adv) in advances.iter().enumerate() {
        let lsb = if has_prop_lsbs {
            r.read_i16()?
        } else {
            xmins[gid]
        };
        out.extend_from_slice(&adv.to_be_bytes());
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    for &xmin in &xmins[num_h_metrics..num_glyphs] {
        let lsb = if has_mono_lsbs { r.read_i16()? } else { xmin };
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    Ok(out)
}

fn sfnt_checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunks = bytes.chunks_exact(4);
    for c in &mut chunks {
        sum = sum.wrapping_add(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        let mut tail = [0u8; 4];
        tail[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(tail));
    }
    sum
}

fn build_sfnt(
    flavor: u32,
    mut tables: Vec<([u8; 4], Vec<u8>)>,
    index_format: Option<u16>,
) -> Result<Vec<u8>, String> {
    if tables.is_empty() || tables.len() > MAX_TABLES {
        return Err("Invalid SFNT table count".into());
    }
    for (tag, data) in &mut tables {
        if tag == b"head" {
            if data.len() < 54 {
                return Err("Truncated SFNT head table".into());
            }
            data[8..12].fill(0);
            if let Some(fmt) = index_format {
                data[50..52].copy_from_slice(&fmt.to_be_bytes());
            }
        }
    }
    tables.sort_by_key(|(tag, _)| *tag);
    for pair in tables.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err("Duplicate SFNT table tag".into());
        }
    }

    let num_tables = tables.len() as u16;
    let entry_selector = num_tables.ilog2() as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = num_tables * 16 - search_range;

    let dir_size = 12usize + usize::from(num_tables) * 16;
    let mut total_size = dir_size;
    for (_, data) in &tables {
        let padded = (data.len() + 3) & !3;
        total_size = total_size
            .checked_add(padded)
            .filter(|&n| n <= MAX_DECOMPRESSED_FONT_BYTES)
            .ok_or_else(|| "Reconstructed SFNT exceeds 4 MiB limit".to_owned())?;
    }

    let mut out = Vec::with_capacity(total_size);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut offset = dir_size;
    let mut head_offset = None;
    for (tag, data) in &tables {
        if tag == b"head" {
            head_offset = Some(offset);
        }
        let checksum = sfnt_checksum(data);
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += (data.len() + 3) & !3;
    }

    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    if let Some(h_off) = head_offset {
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(sfnt_checksum(&out));
        out[h_off + 8..h_off + 12].copy_from_slice(&adjust.to_be_bytes());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn system_font_bytes() -> Vec<u8> {
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap()
    }

    fn wrap_sfnt_as_woff1(sfnt: &[u8]) -> Vec<u8> {
        let flavor = u32::from_be_bytes([sfnt[0], sfnt[1], sfnt[2], sfnt[3]]);
        let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
        let mut records = Vec::with_capacity(num_tables);
        for i in 0..num_tables {
            let rec = &sfnt[12 + i * 16..12 + (i + 1) * 16];
            let tag = [rec[0], rec[1], rec[2], rec[3]];
            let checksum = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]);
            let off = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
            let len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
            let orig = &sfnt[off..off + len];
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
            enc.write_all(orig).unwrap();
            let comp = enc.finish().unwrap();
            let payload = if comp.len() < orig.len() {
                comp
            } else {
                orig.to_vec()
            };
            records.push((tag, payload, len as u32, checksum));
        }

        let mut out = vec![0u8; 44 + num_tables * 20];
        let mut total_sfnt = 12u32 + (num_tables as u32) * 16;
        for (i, (tag, payload, orig_len, checksum)) in records.iter().enumerate() {
            let offset = out.len() as u32;
            out.extend_from_slice(payload);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            total_sfnt += (*orig_len + 3) & !3;
            let dir = &mut out[44 + i * 20..44 + (i + 1) * 20];
            dir[0..4].copy_from_slice(tag);
            dir[4..8].copy_from_slice(&offset.to_be_bytes());
            dir[8..12].copy_from_slice(&(payload.len() as u32).to_be_bytes());
            dir[12..16].copy_from_slice(&orig_len.to_be_bytes());
            dir[16..20].copy_from_slice(&checksum.to_be_bytes());
        }
        let total_len = out.len() as u32;
        out[0..4].copy_from_slice(b"wOFF");
        out[4..8].copy_from_slice(&flavor.to_be_bytes());
        out[8..12].copy_from_slice(&total_len.to_be_bytes());
        out[12..14].copy_from_slice(&(num_tables as u16).to_be_bytes());
        out[16..20].copy_from_slice(&total_sfnt.to_be_bytes());
        out
    }

    #[test]
    fn woff1_roundtrips_real_truetype_for_shaping_and_rasterization() {
        let ttf = system_font_bytes();
        let woff1 = wrap_sfnt_as_woff1(&ttf);
        assert_eq!(&woff1[..4], b"wOFF");
        let decoded = decode_font_bytes(woff1).expect("WOFF1 must decode");
        let font = fontdue::Font::from_bytes(&*decoded, fontdue::FontSettings::default())
            .expect("fontdue must parse reconstructed SFNT");
        let face = rustybuzz::Face::from_slice(&decoded, 0)
            .expect("rustybuzz must parse reconstructed SFNT");
        assert!(face.units_per_em() > 0);
        let (metrics, bitmap) = font.rasterize('A', 24.0);
        assert!(metrics.width > 0 && bitmap.iter().any(|&px| px > 0));
    }

    fn brotli_store_uncompressed(data: &[u8]) -> Vec<u8> {
        struct Bits {
            out: Vec<u8>,
            cur: u8,
            used: u8,
        }
        impl Bits {
            fn push(&mut self, val: u32, count: u8) {
                for i in 0..count {
                    if (val >> i) & 1 != 0 {
                        self.cur |= 1 << self.used;
                    }
                    self.used += 1;
                    if self.used == 8 {
                        self.out.push(self.cur);
                        self.cur = 0;
                        self.used = 0;
                    }
                }
            }
            fn align_zero(&mut self) {
                if self.used != 0 {
                    self.out.push(self.cur);
                    self.cur = 0;
                    self.used = 0;
                }
            }
        }
        let mut b = Bits {
            out: Vec::with_capacity(data.len() + 64),
            cur: 0,
            used: 0,
        };
        // WBITS = 16 (single 0 bit).
        b.push(0, 1);
        for chunk in data.chunks(65536) {
            // ISLAST = 0, MNIBBLES = 00 (4 nibbles = 16 bits), MLEN - 1, ISUNCOMPRESSED = 1.
            b.push(0, 1);
            b.push(0, 2);
            b.push((chunk.len() - 1) as u32, 16);
            b.push(1, 1);
            b.align_zero();
            b.out.extend_from_slice(chunk);
        }
        // Final empty meta-block: ISLAST = 1, ISEMPTY = 1.
        b.push(1, 1);
        b.push(1, 1);
        b.align_zero();
        b.out
    }

    fn push_uint_base128(out: &mut Vec<u8>, mut v: u32) {
        let mut buf = [0u8; 5];
        let mut n = 0usize;
        loop {
            buf[n] = (v & 0x7f) as u8;
            v >>= 7;
            n += 1;
            if v == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            out.push(buf[i] | if i > 0 { 0x80 } else { 0 });
        }
    }

    #[test]
    fn woff2_reconstructs_transformed_glyf_loca_and_hmtx() {
        // Build a minimal 2-glyph transformed glyf stream:
        // gid 0: empty (.notdef), gid 1: 4-point rectangle (100,0)-(500,700).
        let mut glyf_xform = Vec::new();
        glyf_xform.extend_from_slice(&0u16.to_be_bytes()); // reserved
        glyf_xform.extend_from_slice(&0u16.to_be_bytes()); // optionFlags
        glyf_xform.extend_from_slice(&2u16.to_be_bytes()); // numGlyphs = 2
        glyf_xform.extend_from_slice(&0u16.to_be_bytes()); // indexFormat = 0 (short loca)

        let n_contour = [0i16.to_be_bytes(), 1i16.to_be_bytes()].concat();
        let n_points = vec![4u8];
        // Four on-curve points using XYTriplet flags:
        // p0: (+100, 0) -> flag 11 (dx = +b0, dy = 0), b0 = 100
        // p1: (0, +700) -> flag 5  (dx = 0, dy = +(2<<8)+188 = +700), b0 = 188
        // p2: (+400, 0) -> flag 13 (dx = +(1<<8)+144 = +400, dy = 0), b0 = 144
        // p3: (0, -700) -> flag 4  (dx = 0, dy = -(2<<8)+188 = -700), b0 = 188
        let flags = vec![11u8, 5u8, 13u8, 4u8];
        let glyph_bytes = vec![100u8, 188u8, 144u8, 188u8, 0u8]; // + 0 instruction len
        let bbox = vec![0u8; 4]; // 4-byte aligned bitmap, all 0 (implicit bbox)

        for len in [
            n_contour.len(),
            n_points.len(),
            flags.len(),
            glyph_bytes.len(),
            0usize,
            bbox.len(),
            0usize,
        ] {
            glyf_xform.extend_from_slice(&(len as u32).to_be_bytes());
        }
        glyf_xform.extend_from_slice(&n_contour);
        glyf_xform.extend_from_slice(&n_points);
        glyf_xform.extend_from_slice(&flags);
        glyf_xform.extend_from_slice(&glyph_bytes);
        glyf_xform.extend_from_slice(&bbox);

        // Transformed hmtx (flags = 0x03 -> omit both lsb arrays, reconstruct from xmins [0, 100]).
        let mut hmtx_xform = vec![0x03u8];
        hmtx_xform.extend_from_slice(&500u16.to_be_bytes());
        hmtx_xform.extend_from_slice(&600u16.to_be_bytes());

        let mut head = vec![0u8; 54];
        head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
        head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm

        let mut hhea = vec![0u8; 36];
        hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        hhea[4..6].copy_from_slice(&800i16.to_be_bytes());
        hhea[6..8].copy_from_slice(&(-200i16).to_be_bytes());
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes()); // numberOfHMetrics = 2

        let mut maxp = vec![0u8; 32];
        maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&2u16.to_be_bytes()); // numGlyphs = 2

        // Minimal format-4 cmap mapping 'A' (65) -> gid 1.
        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&3u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&12u32.to_be_bytes());
        let mut sub = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&32u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&4u16.to_be_bytes()); // segCountX2 = 4 (2 segments)
        sub.extend_from_slice(&4u16.to_be_bytes());
        sub.extend_from_slice(&1u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&65u16.to_be_bytes()); // endCode[0] = 'A'
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&65u16.to_be_bytes()); // startCode[0] = 'A'
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
        sub.extend_from_slice(&(-64i16).to_be_bytes()); // idDelta[0]: 65 - 64 = gid 1
        sub.extend_from_slice(&1i16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&sub);

        // Known tag indices: cmap=0, head=1, hhea=2, hmtx=3 (xform=1 -> 0x43), maxp=4, glyf=10, loca=11.
        let dir_specs: &[(u8, usize, Option<usize>, &[u8])] = &[
            (0, cmap.len(), None, &cmap),
            (1, head.len(), None, &head),
            (2, hhea.len(), None, &hhea),
            (0x40 | 3, 8, Some(hmtx_xform.len()), &hmtx_xform),
            (4, maxp.len(), None, &maxp),
            (10, 32, Some(glyf_xform.len()), &glyf_xform),
            (11, 6, Some(0), &[]),
        ];

        let mut dir_bytes = Vec::new();
        let mut raw_stream = Vec::new();
        for &(flags, orig_len, xform_len, payload) in dir_specs {
            dir_bytes.push(flags);
            push_uint_base128(&mut dir_bytes, orig_len as u32);
            if let Some(xl) = xform_len {
                push_uint_base128(&mut dir_bytes, xl as u32);
            }
            raw_stream.extend_from_slice(payload);
        }
        let comp = brotli_store_uncompressed(&raw_stream);

        let mut woff2 = Vec::new();
        let total_len = 48 + dir_bytes.len() + comp.len();
        woff2.extend_from_slice(b"wOF2");
        woff2.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        woff2.extend_from_slice(&(total_len as u32).to_be_bytes());
        woff2.extend_from_slice(&(dir_specs.len() as u16).to_be_bytes());
        woff2.extend_from_slice(&0u16.to_be_bytes());
        woff2.extend_from_slice(&512u32.to_be_bytes());
        woff2.extend_from_slice(&(comp.len() as u32).to_be_bytes());
        woff2.extend_from_slice(&[0u8; 24]);
        woff2.extend_from_slice(&dir_bytes);
        woff2.extend_from_slice(&comp);

        let sfnt = decode_font_bytes(woff2).expect("WOFF2 must decode");
        let font = fontdue::Font::from_bytes(&*sfnt, fontdue::FontSettings::default())
            .expect("fontdue must parse reconstructed WOFF2 SFNT");
        let face = rustybuzz::Face::from_slice(&sfnt, 0)
            .expect("rustybuzz must parse reconstructed WOFF2 SFNT");
        assert_eq!(face.units_per_em(), 1000);
        let (metrics, bitmap) = font.rasterize('A', 32.0);
        assert!(metrics.width > 0 && metrics.height > 0);
        assert!(bitmap.iter().any(|&px| px > 0));
    }

    #[test]
    fn rejects_invalid_signatures_and_truncated_containers() {
        assert!(decode_font_bytes(vec![0, 1, 2]).is_err());
        assert!(decode_font_bytes(b"NOPE000000000000".to_vec()).is_err());
        assert!(decode_font_bytes(b"wOFFshort".to_vec()).is_err());
        assert!(decode_font_bytes(b"wOF2short".to_vec()).is_err());
    }
}
