//! Pure-Rust software HTML5 `<canvas>` 2D rendering engine (`CanvasRenderingContext2D`),
//! compressed PNG `toDataURL` / `toBlob` serialization, and live DOM bitmap projection.
//! Zero OS/file/network I/O; safe inside the restricted `RLIMIT_NOFILE = 3` script worker.

use super::*;
use modern::boa_engine::object::{
    ObjectInitializer,
    builtins::{JsArray, JsArrayBuffer, JsTypedArray, JsUint8Array},
};

pub(super) const CANVAS_GET_CONTEXT: &str = "host.canvas.getContext";
pub(super) const CANVAS_TO_DATA_URL: &str = "host.canvas.toDataURL";
pub(super) const CANVAS_TO_BLOB: &str = "host.canvas.toBlob";

const DEFAULT_WIDTH: u32 = 300;
const DEFAULT_HEIGHT: u32 = 150;
const MAX_CANVAS_SIDE: u32 = 1024;
const MAX_CANVAS_PIXELS: usize = 524_288; // 2 MiB RGBA per canvas
const MAX_REALM_CANVAS_BYTES: usize = 4 * 1024 * 1024; // 4 MiB RGBA across realm
const MAX_PROJECTION_DATA_URL_BYTES: usize = 128 * 1024;
const MAX_STATE_STACK: usize = 128;
const MAX_SUBPATHS: usize = 4096;
const MAX_POINTS_PER_SUBPATH: usize = 8192;
const MAX_GRADIENTS: usize = 512;

#[derive(Trace, Finalize)]
struct RetainedContext2D {
    #[unsafe_ignore_trace]
    world_id: u32,
    #[unsafe_ignore_trace]
    node_id: usize,
    object: JsObject,
}

#[derive(Trace, Finalize)]
struct RetainedGradient {
    #[unsafe_ignore_trace]
    world_id: u32,
    #[unsafe_ignore_trace]
    grad_id: usize,
    object: JsObject,
}

#[derive(Default, Trace, Finalize)]
pub(super) struct CanvasStore {
    #[unsafe_ignore_trace]
    surfaces: HashMap<usize, CanvasSurface>,
    #[unsafe_ignore_trace]
    total_bytes: usize,
    #[unsafe_ignore_trace]
    gradients: Vec<GradientDef>,
    contexts: Vec<RetainedContext2D>,
    gradient_objects: Vec<RetainedGradient>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompositeOp {
    SourceOver,
    Copy,
    DestinationOver,
    Lighter,
    Clear,
    SourceIn,
    SourceOut,
    SourceAtop,
    DestinationIn,
    DestinationOut,
    DestinationAtop,
    Xor,
}

impl CompositeOp {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "source-over" => Some(Self::SourceOver),
            "copy" => Some(Self::Copy),
            "destination-over" => Some(Self::DestinationOver),
            "lighter" | "plus-lighter" => Some(Self::Lighter),
            "clear" => Some(Self::Clear),
            "source-in" => Some(Self::SourceIn),
            "source-out" => Some(Self::SourceOut),
            "source-atop" => Some(Self::SourceAtop),
            "destination-in" => Some(Self::DestinationIn),
            "destination-out" => Some(Self::DestinationOut),
            "destination-atop" => Some(Self::DestinationAtop),
            "xor" => Some(Self::Xor),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SourceOver => "source-over",
            Self::Copy => "copy",
            Self::DestinationOver => "destination-over",
            Self::Lighter => "lighter",
            Self::Clear => "clear",
            Self::SourceIn => "source-in",
            Self::SourceOut => "source-out",
            Self::SourceAtop => "source-atop",
            Self::DestinationIn => "destination-in",
            Self::DestinationOut => "destination-out",
            Self::DestinationAtop => "destination-atop",
            Self::Xor => "xor",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
}

impl TextAlign {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "start" => Some(Self::Start),
            "end" => Some(Self::End),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "center" => Some(Self::Center),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
            Self::Left => "left",
            Self::Right => "right",
            Self::Center => "center",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextBaseline {
    Top,
    Hanging,
    Middle,
    Alphabetic,
    Ideographic,
    Bottom,
}

impl TextBaseline {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "top" => Some(Self::Top),
            "hanging" => Some(Self::Hanging),
            "middle" => Some(Self::Middle),
            "alphabetic" => Some(Self::Alphabetic),
            "ideographic" => Some(Self::Ideographic),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Hanging => "hanging",
            Self::Middle => "middle",
            Self::Alphabetic => "alphabetic",
            Self::Ideographic => "ideographic",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Clone)]
enum PaintStyle {
    Color([u8; 4]),
    Gradient(usize),
}

#[derive(Clone)]
enum GradientKind {
    Linear {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    Radial {
        x0: f64,
        y0: f64,
        r0: f64,
        x1: f64,
        y1: f64,
        r1: f64,
    },
}

#[derive(Clone)]
struct GradientDef {
    kind: GradientKind,
    stops: Vec<(f64, [u8; 4])>,
}

impl GradientDef {
    fn sample(&self, t_raw: f64) -> [u8; 4] {
        if self.stops.is_empty() {
            return [0, 0, 0, 0];
        }
        let t = t_raw.clamp(0.0, 1.0);
        if t <= self.stops[0].0 || self.stops.len() == 1 {
            return self.stops[0].1;
        }
        let last = self.stops.len() - 1;
        if t >= self.stops[last].0 {
            return self.stops[last].1;
        }
        for pair in self.stops.windows(2) {
            let (t0, c0) = pair[0];
            let (t1, c1) = pair[1];
            if t >= t0 && t <= t1 {
                let span = t1 - t0;
                if span <= 1e-9 {
                    return c1;
                }
                let u = ((t - t0) / span).clamp(0.0, 1.0);
                return [
                    (c0[0] as f64 + (c1[0] as f64 - c0[0] as f64) * u).round() as u8,
                    (c0[1] as f64 + (c1[1] as f64 - c0[1] as f64) * u).round() as u8,
                    (c0[2] as f64 + (c1[2] as f64 - c0[2] as f64) * u).round() as u8,
                    (c0[3] as f64 + (c1[3] as f64 - c0[3] as f64) * u).round() as u8,
                ];
            }
        }
        self.stops[last].1
    }

    fn sample_at(&self, ux: f64, uy: f64) -> [u8; 4] {
        match self.kind {
            GradientKind::Linear { x0, y0, x1, y1 } => {
                let dx = x1 - x0;
                let dy = y1 - y0;
                let len_sq = dx * dx + dy * dy;
                if len_sq <= 1e-12 {
                    return self.sample(0.0);
                }
                let t = ((ux - x0) * dx + (uy - y0) * dy) / len_sq;
                self.sample(t)
            }
            GradientKind::Radial {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
            } => {
                if (x0 - x1).abs() <= 1e-9 && (y0 - y1).abs() <= 1e-9 && (r0 - r1).abs() <= 1e-9 {
                    return [0, 0, 0, 0];
                }
                let dx = x1 - x0;
                let dy = y1 - y0;
                let dr = r1 - r0;
                let px = ux - x0;
                let py = uy - y0;
                let a = dx * dx + dy * dy - dr * dr;
                let b = -2.0 * (px * dx + py * dy + r0 * dr);
                let c = px * px + py * py - r0 * r0;
                let mut best_t: Option<f64> = None;
                if a.abs() <= 1e-12 {
                    if b.abs() > 1e-12 {
                        let t = -c / b;
                        if r0 + t * dr >= 0.0 {
                            best_t = Some(t);
                        }
                    }
                } else {
                    let disc = b * b - 4.0 * a * c;
                    if disc >= 0.0 {
                        let sq = disc.sqrt();
                        let t1 = (-b - sq) / (2.0 * a);
                        let t2 = (-b + sq) / (2.0 * a);
                        for cand in [t1.max(t2), t1.min(t2)] {
                            if r0 + cand * dr >= 0.0 {
                                best_t = Some(cand);
                                break;
                            }
                        }
                    }
                }
                match best_t {
                    Some(t) => self.sample(t),
                    None => [0, 0, 0, 0],
                }
            }
        }
    }
}

#[derive(Clone)]
struct ContextState {
    transform: [f64; 6],
    fill_style: PaintStyle,
    stroke_style: PaintStyle,
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f64,
    line_dash: Vec<f64>,
    line_dash_offset: f64,
    global_alpha: f64,
    composite_op: CompositeOp,
    font_raw: String,
    font_px: f64,
    font_bold: bool,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    shadow_color: [u8; 4],
    shadow_blur: f64,
    shadow_offset_x: f64,
    shadow_offset_y: f64,
    clip_mask: Option<Vec<u8>>,
}

impl Default for ContextState {
    fn default() -> Self {
        Self {
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_style: PaintStyle::Color([0, 0, 0, 255]),
            stroke_style: PaintStyle::Color([0, 0, 0, 255]),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
            global_alpha: 1.0,
            composite_op: CompositeOp::SourceOver,
            font_raw: "10px sans-serif".to_owned(),
            font_px: 10.0,
            font_bold: false,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            shadow_color: [0, 0, 0, 0],
            shadow_blur: 0.0,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            clip_mask: None,
        }
    }
}

#[derive(Clone)]
struct Subpath {
    points: Vec<(f64, f64)>,
    closed: bool,
}

struct CanvasSurface {
    logical_w: u32,
    logical_h: u32,
    buf_w: u32,
    buf_h: u32,
    scale_x: f64,
    scale_y: f64,
    pixels: Vec<u8>,
    state: ContextState,
    state_stack: Vec<ContextState>,
    subpaths: Vec<Subpath>,
    has_context_2d: bool,
    dirty: bool,
}

fn compute_backing_dims(logical_w: u32, logical_h: u32) -> (u32, u32, f64, f64) {
    if logical_w == 0 || logical_h == 0 {
        return (0, 0, 1.0, 1.0);
    }
    let mut s = 1.0f64;
    if logical_w > MAX_CANVAS_SIDE {
        s = s.min(MAX_CANVAS_SIDE as f64 / logical_w as f64);
    }
    if logical_h > MAX_CANVAS_SIDE {
        s = s.min(MAX_CANVAS_SIDE as f64 / logical_h as f64);
    }
    let area = (logical_w as f64 * s) * (logical_h as f64 * s);
    if area > MAX_CANVAS_PIXELS as f64 {
        s *= (MAX_CANVAS_PIXELS as f64 / area).sqrt();
    }
    let mut bw = (logical_w as f64 * s).round() as u32;
    let mut bh = (logical_h as f64 * s).round() as u32;
    bw = bw.clamp(1, MAX_CANVAS_SIDE);
    bh = bh.clamp(1, MAX_CANVAS_SIDE);
    while (bw as usize) * (bh as usize) > MAX_CANVAS_PIXELS {
        if bw >= bh && bw > 1 {
            bw -= 1;
        } else if bh > 1 {
            bh -= 1;
        } else {
            break;
        }
    }
    (
        bw,
        bh,
        bw as f64 / logical_w as f64,
        bh as f64 / logical_h as f64,
    )
}

pub(super) fn canvas_logical_dimensions(node: &crate::document::Node) -> (u32, u32) {
    let w = node
        .attr("width")
        .and_then(parse_canvas_uint)
        .unwrap_or(DEFAULT_WIDTH);
    let h = node
        .attr("height")
        .and_then(parse_canvas_uint)
        .unwrap_or(DEFAULT_HEIGHT);
    (w, h)
}

fn parse_canvas_uint(raw: &str) -> Option<u32> {
    let trimmed = raw.trim_start_matches(['\t', '\n', '\x0C', '\r', ' ']);
    let trimmed = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

fn ensure_surface(data: &mut State, node_id: usize) -> Result<(), String> {
    let (lw, lh) = canvas_logical_dimensions(&data.host.document.nodes[node_id]);
    if let Some(existing) = data.canvas.surfaces.get(&node_id)
        && existing.logical_w == lw
        && existing.logical_h == lh
    {
        return Ok(());
    }
    reset_or_resize_surface(data, node_id, lw, lh, false)
}

pub(super) fn on_canvas_dimension_mutated(data: &mut State, node_id: usize) -> Result<(), String> {
    if data
        .host
        .document
        .nodes
        .get(node_id)
        .map(|n| n.tag.as_str())
        != Some("canvas")
    {
        return Ok(());
    }
    let (lw, lh) = canvas_logical_dimensions(&data.host.document.nodes[node_id]);
    let mark_dirty = data.canvas.surfaces.contains_key(&node_id)
        || data.host.document.nodes[node_id]
            .attr(crate::document::INTERNAL_CANVAS_ATTR)
            .is_some();
    reset_or_resize_surface(data, node_id, lw, lh, mark_dirty)
}

fn reset_or_resize_surface(
    data: &mut State,
    node_id: usize,
    logical_w: u32,
    logical_h: u32,
    mark_dirty: bool,
) -> Result<(), String> {
    let (buf_w, buf_h, scale_x, scale_y) = compute_backing_dims(logical_w, logical_h);
    let new_bytes = (buf_w as usize)
        .saturating_mul(buf_h as usize)
        .saturating_mul(4);
    let old_bytes = data
        .canvas
        .surfaces
        .get(&node_id)
        .map_or(0, |s| s.pixels.len());
    let has_ctx = data
        .canvas
        .surfaces
        .get(&node_id)
        .is_some_and(|s| s.has_context_2d);
    let next_total = data
        .canvas
        .total_bytes
        .saturating_sub(old_bytes)
        .saturating_add(new_bytes);
    if next_total > MAX_REALM_CANVAS_BYTES {
        return Err("Canvas backing store quota exceeded (4 MiB realm limit)".into());
    }
    data.canvas.total_bytes = next_total;
    data.canvas.surfaces.insert(
        node_id,
        CanvasSurface {
            logical_w,
            logical_h,
            buf_w,
            buf_h,
            scale_x,
            scale_y,
            pixels: vec![0u8; new_bytes],
            state: ContextState::default(),
            state_stack: Vec::new(),
            subpaths: Vec::new(),
            has_context_2d: has_ctx,
            dirty: mark_dirty,
        },
    );
    Ok(())
}

pub(super) fn flush_dirty_canvases_to_dom(data: &mut State) {
    let dirty_ids: Vec<usize> = data
        .canvas
        .surfaces
        .iter()
        .filter_map(|(&id, surf)| surf.dirty.then_some(id))
        .collect();
    for id in dirty_ids {
        let Some(surf) = data.canvas.surfaces.get_mut(&id) else {
            continue;
        };
        surf.dirty = false;
        if id >= data.host.document.nodes.len() || data.host.document.nodes[id].tag != "canvas" {
            continue;
        }
        if surf.buf_w == 0 || surf.buf_h == 0 || surf.pixels.iter().all(|&b| b == 0) {
            data.host.document.nodes[id]
                .attributes
                .retain(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR);
            continue;
        }
        let png = encode_png_rgba(surf.buf_w, surf.buf_h, &surf.pixels);
        let data_url = format!("data:image/png;base64,{}", base64_encode(&png));
        if data_url.len() <= MAX_PROJECTION_DATA_URL_BYTES {
            let attrs = &mut data.host.document.nodes[id].attributes;
            if let Some((_, existing)) = attrs
                .iter_mut()
                .find(|(k, _)| k == crate::document::INTERNAL_CANVAS_ATTR)
            {
                *existing = data_url;
            } else if attrs.len() < 256 {
                attrs.push((crate::document::INTERNAL_CANVAS_ATTR.to_owned(), data_url));
            }
        }
    }
}

// --- Pure-Rust Compressed PNG Encoder (RFC 1950 zlib + RFC 1951 Fixed Huffman LZ77 + PNG) ---

fn crc32_ieee(chunks: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for chunk in chunks {
        for &b in *chunk {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for chunk in data.chunks(5552) {
        for &b in chunk {
            s1 += u32::from(b);
            s2 += s1;
        }
        s1 %= 65521;
        s2 %= 65521;
    }
    (s2 << 16) | s1
}

struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    bits: u32,
}

impl BitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self {
            out: Vec::with_capacity(cap),
            acc: 0,
            bits: 0,
        }
    }

    #[inline]
    fn write_lsb(&mut self, val: u32, count: u32) {
        self.acc |= (val & ((1u32 << count) - 1)) << self.bits;
        self.bits += count;
        while self.bits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.bits -= 8;
        }
    }

    #[inline]
    fn write_huff_msb(&mut self, code: u32, len: u32) {
        let mut rev = 0u32;
        for i in 0..len {
            if (code >> (len - 1 - i)) & 1 != 0 {
                rev |= 1 << i;
            }
        }
        self.write_lsb(rev, len);
    }

    #[inline]
    fn write_fixed_lit_len(&mut self, sym: usize) {
        if sym <= 143 {
            self.write_huff_msb(0x30 + sym as u32, 8);
        } else if sym <= 255 {
            self.write_huff_msb(0x190 + (sym as u32 - 144), 9);
        } else if sym <= 279 {
            self.write_huff_msb(sym as u32 - 256, 7);
        } else {
            self.write_huff_msb(0xC0 + (sym as u32 - 280), 8);
        }
    }

    fn flush_byte_align(&mut self) {
        if self.bits > 0 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.bits = 0;
        }
    }
}

fn deflate_fixed_lz77(input: &[u8]) -> Vec<u8> {
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    let mut bw = BitWriter::with_capacity((input.len() / 4).max(64) + 16);
    bw.out.push(0x78);
    bw.out.push(0x01);
    // Final block (BFINAL=1), Fixed Huffman (BTYPE=01)
    bw.write_lsb(0b011, 3);

    let mut head = [usize::MAX; 4096];
    let mut prev = vec![usize::MAX; input.len().clamp(1, 32768)];
    let prev_mask = prev.len() - 1;
    let hash3 = |pos: usize| -> usize {
        (((input[pos] as usize) << 8)
            ^ ((input[pos + 1] as usize) << 4)
            ^ (input[pos + 2] as usize))
            & 4095
    };

    let mut pos = 0usize;
    while pos < input.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if pos + 3 <= input.len() {
            let h = hash3(pos);
            let mut cand = head[h];
            head[h] = pos;
            let max_len = (input.len() - pos).min(258);
            let mut steps = 0usize;
            while cand != usize::MAX && pos - cand <= 32768 && steps < 16 {
                if input[cand + best_len.min(max_len - 1)] == input[pos + best_len.min(max_len - 1)]
                    && input[cand..cand + 3] == input[pos..pos + 3]
                {
                    let mut mlen = 3usize;
                    while mlen < max_len && input[cand + mlen] == input[pos + mlen] {
                        mlen += 1;
                    }
                    if mlen > best_len {
                        best_len = mlen;
                        best_dist = pos - cand;
                        if best_len == 258 {
                            break;
                        }
                    }
                }
                if prev.len() < 32768 {
                    break;
                }
                let next_cand = prev[cand & prev_mask];
                if next_cand >= cand {
                    break;
                }
                cand = next_cand;
                steps += 1;
            }
            if prev.len() == 32768 {
                prev[pos & prev_mask] = cand;
            }
        }
        if best_len >= 3 {
            let l_idx = LEN_BASE
                .partition_point(|&b| usize::from(b) <= best_len)
                .saturating_sub(1);
            bw.write_fixed_lit_len(257 + l_idx);
            let lextra = u32::from(LEN_EXTRA[l_idx]);
            if lextra > 0 {
                bw.write_lsb((best_len - usize::from(LEN_BASE[l_idx])) as u32, lextra);
            }
            let d_idx = DIST_BASE
                .partition_point(|&b| usize::from(b) <= best_dist)
                .saturating_sub(1);
            bw.write_huff_msb(d_idx as u32, 5);
            let dextra = u32::from(DIST_EXTRA[d_idx]);
            if dextra > 0 {
                bw.write_lsb((best_dist - usize::from(DIST_BASE[d_idx])) as u32, dextra);
            }
            let end_step = (pos + best_len).min(input.len());
            pos += 1;
            while pos < end_step {
                if pos + 3 <= input.len() {
                    let h = hash3(pos);
                    if prev.len() == 32768 {
                        prev[pos & prev_mask] = head[h];
                    }
                    head[h] = pos;
                }
                pos += 1;
            }
        } else {
            bw.write_fixed_lit_len(usize::from(input[pos]));
            pos += 1;
        }
    }
    bw.write_fixed_lit_len(256); // End-of-block
    bw.flush_byte_align();
    bw.out.extend_from_slice(&adler32(input).to_be_bytes());
    bw.out
}

pub(crate) fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row_bytes = (width as usize) * 4;
    let mut raw_scanlines = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for y in 0..(height as usize) {
        let row = &rgba[y * row_bytes..(y + 1) * row_bytes];
        // Filter type 1 (Sub) compresses horizontal runs and gradients very compactly
        raw_scanlines.push(1u8);
        for i in 0..row_bytes {
            let prev = if i >= 4 { row[i - 4] } else { 0 };
            raw_scanlines.push(row[i].wrapping_sub(prev));
        }
    }
    let idat_payload = deflate_fixed_lz77(&raw_scanlines);

    let mut png = Vec::with_capacity(64 + idat_payload.len());
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let write_chunk = |out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(body);
        let crc = crc32_ieee(&[tag, body]);
        out.extend_from_slice(&crc.to_be_bytes());
    };

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8; // 8-bit channel depth
    ihdr[9] = 6; // Truecolor with alpha (RGBA)
    ihdr[10] = 0; // Deflate compression
    ihdr[11] = 0; // Adaptive filtering
    ihdr[12] = 0; // No interlace
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &idat_payload);
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// --- CSS Color Parsing & HTML5 Serialization ---

fn serialize_canvas_color(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else if c[3] == 0 {
        format!("rgba({}, {}, {}, 0)", c[0], c[1], c[2])
    } else {
        let a = (f64::from(c[3]) / 255.0 * 1_000_000.0).round() / 1_000_000.0;
        format!("rgba({}, {}, {}, {})", c[0], c[1], c[2], a)
    }
}

fn parse_canvas_color(input: &str) -> Option<[u8; 4]> {
    let s = input.trim();
    if s.is_empty() || s.len() > 256 {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        let bytes = hex.as_bytes();
        let h = |b: u8| -> Option<u8> {
            match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            }
        };
        return match bytes.len() {
            3 => Some([h(bytes[0])? * 17, h(bytes[1])? * 17, h(bytes[2])? * 17, 255]),
            4 => Some([
                h(bytes[0])? * 17,
                h(bytes[1])? * 17,
                h(bytes[2])? * 17,
                h(bytes[3])? * 17,
            ]),
            6 => Some([
                (h(bytes[0])? << 4) | h(bytes[1])?,
                (h(bytes[2])? << 4) | h(bytes[3])?,
                (h(bytes[4])? << 4) | h(bytes[5])?,
                255,
            ]),
            8 => Some([
                (h(bytes[0])? << 4) | h(bytes[1])?,
                (h(bytes[2])? << 4) | h(bytes[3])?,
                (h(bytes[4])? << 4) | h(bytes[5])?,
                (h(bytes[6])? << 4) | h(bytes[7])?,
            ]),
            _ => None,
        };
    }

    let lower = s.to_ascii_lowercase();
    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        return parse_rgb_args(inner);
    }
    if let Some(inner) = lower
        .strip_prefix("hsla(")
        .or_else(|| lower.strip_prefix("hsl("))
        .and_then(|r| r.strip_suffix(')'))
    {
        return parse_hsl_args(inner);
    }
    named_css_color(&lower)
}

fn parse_alpha_token(tok: &str) -> Option<u8> {
    let t = tok.trim();
    if let Some(pct) = t.strip_suffix('%') {
        let v: f64 = pct.trim().parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        Some(((v.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8)
    } else {
        let v: f64 = t.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        Some((v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

fn parse_rgb_channel(tok: &str) -> Option<u8> {
    let t = tok.trim();
    if let Some(pct) = t.strip_suffix('%') {
        let v: f64 = pct.trim().parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        Some(((v.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8)
    } else {
        let v: f64 = t.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        Some(v.round().clamp(0.0, 255.0) as u8)
    }
}

fn parse_rgb_args(inner: &str) -> Option<[u8; 4]> {
    if inner.contains(',') {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let r = parse_rgb_channel(parts[0])?;
        let g = parse_rgb_channel(parts[1])?;
        let b = parse_rgb_channel(parts[2])?;
        let a = if parts.len() == 4 {
            parse_alpha_token(parts[3])?
        } else {
            255
        };
        Some([r, g, b, a])
    } else {
        let (rgb_part, alpha_part) = match inner.split_once('/') {
            Some((l, r)) => (l, Some(r)),
            None => (inner, None),
        };
        let toks: Vec<&str> = rgb_part.split_ascii_whitespace().collect();
        if toks.len() != 3 {
            return None;
        }
        let r = parse_rgb_channel(toks[0])?;
        let g = parse_rgb_channel(toks[1])?;
        let b = parse_rgb_channel(toks[2])?;
        let a = match alpha_part {
            Some(ap) => parse_alpha_token(ap)?,
            None => 255,
        };
        Some([r, g, b, a])
    }
}

fn parse_hsl_args(inner: &str) -> Option<[u8; 4]> {
    let (hsl_part, alpha_opt) = if inner.contains(',') {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let a = if parts.len() == 4 {
            Some(parse_alpha_token(parts[3])?)
        } else {
            None
        };
        ((parts[0], parts[1], parts[2]), a)
    } else {
        let (l, r) = match inner.split_once('/') {
            Some((l, r)) => (l, Some(parse_alpha_token(r)?)),
            None => (inner, None),
        };
        let toks: Vec<&str> = l.split_ascii_whitespace().collect();
        if toks.len() != 3 {
            return None;
        }
        ((toks[0], toks[1], toks[2]), r)
    };
    let h_str = hsl_part.0.trim();
    let h_deg = if let Some(d) = h_str.strip_suffix("deg") {
        d.trim().parse::<f64>().ok()?
    } else if let Some(t) = h_str.strip_suffix("turn") {
        t.trim().parse::<f64>().ok()? * 360.0
    } else if let Some(r) = h_str.strip_suffix("rad") {
        r.trim().parse::<f64>().ok()?.to_degrees()
    } else {
        h_str.parse::<f64>().ok()?
    };
    let sat = hsl_part
        .1
        .trim()
        .strip_suffix('%')?
        .trim()
        .parse::<f64>()
        .ok()?
        / 100.0;
    let lig = hsl_part
        .2
        .trim()
        .strip_suffix('%')?
        .trim()
        .parse::<f64>()
        .ok()?
        / 100.0;
    if !h_deg.is_finite() || !sat.is_finite() || !lig.is_finite() {
        return None;
    }
    let s = sat.clamp(0.0, 1.0);
    let l = lig.clamp(0.0, 1.0);
    let h = h_deg.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Some([
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        alpha_opt.unwrap_or(255),
    ])
}

fn named_css_color(name: &str) -> Option<[u8; 4]> {
    if name == "transparent" {
        return Some([0, 0, 0, 0]);
    }
    const COLORS: &[(&str, u32)] = &[
        ("black", 0x000000),
        ("white", 0xffffff),
        ("red", 0xff0000),
        ("lime", 0x00ff00),
        ("green", 0x008000),
        ("blue", 0x0000ff),
        ("yellow", 0xffff00),
        ("cyan", 0x00ffff),
        ("aqua", 0x00ffff),
        ("magenta", 0xff00ff),
        ("fuchsia", 0xff00ff),
        ("silver", 0xc0c0c0),
        ("gray", 0x808080),
        ("grey", 0x808080),
        ("darkgray", 0xa9a9a9),
        ("darkgrey", 0xa9a9a9),
        ("lightgray", 0xd3d3d3),
        ("lightgrey", 0xd3d3d3),
        ("dimgray", 0x696969),
        ("dimgrey", 0x696969),
        ("slategray", 0x708090),
        ("slategrey", 0x708090),
        ("darkslategray", 0x2f4f4f),
        ("darkslategrey", 0x2f4f4f),
        ("lightslategray", 0x778899),
        ("lightslategrey", 0x778899),
        ("whitesmoke", 0xf5f5f5),
        ("gainsboro", 0xdcdcdc),
        ("maroon", 0x800000),
        ("darkred", 0x8b0000),
        ("firebrick", 0xb22222),
        ("crimson", 0xdc143c),
        ("tomato", 0xff6347),
        ("coral", 0xff7f50),
        ("salmon", 0xfa8072),
        ("orangered", 0xff4500),
        ("darkorange", 0xff8c00),
        ("orange", 0xffa500),
        ("gold", 0xffd700),
        ("goldenrod", 0xdaa520),
        ("khaki", 0xf0e68c),
        ("olive", 0x808000),
        ("yellowgreen", 0x9acd32),
        ("limegreen", 0x32cd32),
        ("forestgreen", 0x228b22),
        ("darkgreen", 0x006400),
        ("seagreen", 0x2e8b57),
        ("mediumseagreen", 0x3cb371),
        ("springgreen", 0x00ff7f),
        ("aquamarine", 0x7fffd4),
        ("turquoise", 0x40e0d0),
        ("teal", 0x008080),
        ("darkcyan", 0x008b8b),
        ("skyblue", 0x87ceeb),
        ("deepskyblue", 0x00bfff),
        ("dodgerblue", 0x1e90ff),
        ("cornflowerblue", 0x6495ed),
        ("steelblue", 0x4682b4),
        ("royalblue", 0x4169e1),
        ("navy", 0x000080),
        ("darkblue", 0x00008b),
        ("midnightblue", 0x191970),
        ("indigo", 0x4b0082),
        ("purple", 0x800080),
        ("rebeccapurple", 0x663399),
        ("blueviolet", 0x8a2be2),
        ("darkmagenta", 0x8b008b),
        ("violet", 0xee82ee),
        ("plum", 0xdda0dd),
        ("orchid", 0xda70d6),
        ("deeppink", 0xff1493),
        ("hotpink", 0xff69b4),
        ("pink", 0xffc0cb),
        ("brown", 0xa52a2a),
        ("saddlebrown", 0x8b4513),
        ("sienna", 0xa0522d),
        ("chocolate", 0xd2691e),
        ("peru", 0xcd853f),
        ("tan", 0xd2b48c),
        ("wheat", 0xf5deb3),
        ("beige", 0xf5f5dc),
        ("ivory", 0xfffff0),
        ("snow", 0xfffafa),
        ("aliceblue", 0xf0f8ff),
        ("lavender", 0xe6e6fa),
    ];
    let hex = COLORS.iter().find(|&&(k, _)| k == name)?.1;
    Some([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255])
}

// --- Built-in Compact 8x8 ASCII Bitmap Font (0x20..=0x7E, MSB = left column) ---

const FONT_8X8: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // '!'
    [0x6C, 0x6C, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00], // '"'
    [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00], // '#'
    [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00], // '$'
    [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00], // '%'
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00], // '&'
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00], // '\''
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00], // '('
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00], // ')'
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // '*'
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00], // '+'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // ','
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00], // '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // '.'
    [0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00], // '/'
    [0x7C, 0xC6, 0xCE, 0xD6, 0xE6, 0xC6, 0x7C, 0x00], // '0'
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // '1'
    [0x7C, 0xC6, 0x06, 0x1C, 0x70, 0xC6, 0xFE, 0x00], // '2'
    [0x7C, 0xC6, 0x06, 0x3C, 0x06, 0xC6, 0x7C, 0x00], // '3'
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00], // '4'
    [0xFE, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00], // '5'
    [0x38, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0x7C, 0x00], // '6'
    [0xFE, 0xC6, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00], // '7'
    [0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0x7C, 0x00], // '8'
    [0x7C, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x78, 0x00], // '9'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00], // ':'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30], // ';'
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00], // '<'
    [0x00, 0x00, 0x7E, 0x00, 0x00, 0x7E, 0x00, 0x00], // '='
    [0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00], // '>'
    [0x7C, 0xC6, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x00], // '?'
    [0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xC0, 0x78, 0x00], // '@'
    [0x38, 0x6C, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00], // 'A'
    [0xFC, 0x66, 0x66, 0x7C, 0x66, 0x66, 0xFC, 0x00], // 'B'
    [0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0x66, 0x3C, 0x00], // 'C'
    [0xF8, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00], // 'D'
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x62, 0xFE, 0x00], // 'E'
    [0xFE, 0x62, 0x68, 0x78, 0x68, 0x60, 0xF0, 0x00], // 'F'
    [0x3C, 0x66, 0xC0, 0xC0, 0xCE, 0x66, 0x3E, 0x00], // 'G'
    [0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0x00], // 'H'
    [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'I'
    [0x1E, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00], // 'J'
    [0xE6, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0xE6, 0x00], // 'K'
    [0xF0, 0x60, 0x60, 0x60, 0x62, 0x66, 0xFE, 0x00], // 'L'
    [0xC6, 0xEE, 0xFE, 0xFE, 0xD6, 0xC6, 0xC6, 0x00], // 'M'
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00], // 'N'
    [0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 'O'
    [0xFC, 0x66, 0x66, 0x7C, 0x60, 0x60, 0xF0, 0x00], // 'P'
    [0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x0E], // 'Q'
    [0xFC, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0xE6, 0x00], // 'R'
    [0x7C, 0xC6, 0x60, 0x38, 0x0C, 0xC6, 0x7C, 0x00], // 'S'
    [0x7E, 0x5A, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'T'
    [0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 'U'
    [0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00], // 'V'
    [0xC6, 0xC6, 0xD6, 0xFE, 0xFE, 0xEE, 0xC6, 0x00], // 'W'
    [0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00], // 'X'
    [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x3C, 0x00], // 'Y'
    [0xFE, 0xC6, 0x8C, 0x18, 0x32, 0x66, 0xFE, 0x00], // 'Z'
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00], // '['
    [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00], // '\\'
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00], // ']'
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00], // '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // '_'
    [0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00], // '`'
    [0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0x76, 0x00], // 'a'
    [0xE0, 0x60, 0x7C, 0x66, 0x66, 0x66, 0xDC, 0x00], // 'b'
    [0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC6, 0x7C, 0x00], // 'c'
    [0x1C, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0x76, 0x00], // 'd'
    [0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0x7C, 0x00], // 'e'
    [0x3C, 0x66, 0x60, 0xF8, 0x60, 0x60, 0xF0, 0x00], // 'f'
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0xF8], // 'g'
    [0xE0, 0x60, 0x6C, 0x76, 0x66, 0x66, 0xE6, 0x00], // 'h'
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'i'
    [0x06, 0x00, 0x0E, 0x06, 0x06, 0x66, 0x66, 0x3C], // 'j'
    [0xE0, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0xE6, 0x00], // 'k'
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'l'
    [0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xC6, 0xC6, 0x00], // 'm'
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x66, 0x66, 0x00], // 'n'
    [0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0x00], // 'o'
    [0x00, 0x00, 0xDC, 0x66, 0x66, 0x7C, 0x60, 0xF0], // 'p'
    [0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0x1E], // 'q'
    [0x00, 0x00, 0xDC, 0x76, 0x60, 0x60, 0xF0, 0x00], // 'r'
    [0x00, 0x00, 0x7E, 0xC0, 0x7C, 0x06, 0xFC, 0x00], // 's'
    [0x30, 0x30, 0xFC, 0x30, 0x30, 0x36, 0x1C, 0x00], // 't'
    [0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00], // 'u'
    [0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x00], // 'v'
    [0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xFE, 0x6C, 0x00], // 'w'
    [0x00, 0x00, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0x00], // 'x'
    [0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0xFC], // 'y'
    [0x00, 0x00, 0xFE, 0x8C, 0x18, 0x32, 0xFE, 0x00], // 'z'
    [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00], // '{'
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // '|'
    [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00], // '}'
    [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // '~'
];

fn parse_css_font_spec(raw: &str) -> Option<(f64, bool, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return None;
    }
    let mut bold = false;
    let mut px: Option<f64> = None;
    let mut has_family = false;
    for tok in trimmed.split_ascii_whitespace() {
        let lower = tok.to_ascii_lowercase();
        if px.is_none() {
            if matches!(
                lower.as_str(),
                "bold" | "bolder" | "600" | "700" | "800" | "900"
            ) {
                bold = true;
                continue;
            }
            if matches!(
                lower.as_str(),
                "normal"
                    | "italic"
                    | "oblique"
                    | "lighter"
                    | "small-caps"
                    | "100"
                    | "200"
                    | "300"
                    | "400"
                    | "500"
            ) {
                continue;
            }
            let size_part = lower.split('/').next().unwrap_or(&lower);
            if let Some(num) = size_part.strip_suffix("px")
                && let Ok(v) = num.parse::<f64>()
                && v > 0.0
                && v.is_finite()
            {
                px = Some(v.clamp(1.0, 256.0));
                continue;
            }
            if let Some(num) = size_part.strip_suffix("pt")
                && let Ok(v) = num.parse::<f64>()
                && v > 0.0
                && v.is_finite()
            {
                px = Some((v * (96.0 / 72.0)).clamp(1.0, 256.0));
                continue;
            }
            if let Some(num) = size_part
                .strip_suffix("rem")
                .or_else(|| size_part.strip_suffix("em"))
                && let Ok(v) = num.parse::<f64>()
                && v > 0.0
                && v.is_finite()
            {
                px = Some((v * 16.0).clamp(1.0, 256.0));
                continue;
            }
            return None;
        } else {
            has_family = true;
        }
    }
    let size = px?;
    if !has_family {
        return None;
    }
    Some((size, bold, trimmed.to_owned()))
}

fn measure_text_width(text: &str, font_px: f64, bold: bool) -> f64 {
    let count = text.chars().filter(|&c| c != '\r' && c != '\n').count();
    let adv = font_px * if bold { 0.65 } else { 0.6 };
    (count as f64) * adv
}

// --- 2D Geometry, Scanline Polygon Fill, Stroke Builder, & Compositor ---

#[inline]
fn apply_matrix(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

#[inline]
fn mul_matrix(m1: &[f64; 6], m2: &[f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[2] * m2[1],
        m1[1] * m2[0] + m1[3] * m2[1],
        m1[0] * m2[2] + m1[2] * m2[3],
        m1[1] * m2[2] + m1[3] * m2[3],
        m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
        m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
    ]
}

#[inline]
fn invert_matrix(m: &[f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() <= 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        m[3] * inv,
        -m[1] * inv,
        -m[2] * inv,
        m[0] * inv,
        (m[2] * m[5] - m[3] * m[4]) * inv,
        (m[1] * m[4] - m[0] * m[5]) * inv,
    ])
}

fn composite_pixel(dst: &mut [u8], src: [u8; 4], op: CompositeOp) {
    let sa = f64::from(src[3]) / 255.0;
    let da = f64::from(dst[3]) / 255.0;
    let sr = f64::from(src[0]) / 255.0 * sa;
    let sg = f64::from(src[1]) / 255.0 * sa;
    let sb = f64::from(src[2]) / 255.0 * sa;
    let dr = f64::from(dst[0]) / 255.0 * da;
    let dg = f64::from(dst[1]) / 255.0 * da;
    let db = f64::from(dst[2]) / 255.0 * da;

    let (fa, fb) = match op {
        CompositeOp::Clear => (0.0, 0.0),
        CompositeOp::Copy => (1.0, 0.0),
        CompositeOp::SourceOver => (1.0, 1.0 - sa),
        CompositeOp::DestinationOver => (1.0 - da, 1.0),
        CompositeOp::SourceIn => (da, 0.0),
        CompositeOp::DestinationIn => (0.0, sa),
        CompositeOp::SourceOut => (1.0 - da, 0.0),
        CompositeOp::DestinationOut => (0.0, 1.0 - sa),
        CompositeOp::SourceAtop => (da, 1.0 - sa),
        CompositeOp::DestinationAtop => (1.0 - da, sa),
        CompositeOp::Xor => (1.0 - da, 1.0 - sa),
        CompositeOp::Lighter => (1.0, 1.0),
    };

    let oa = (sa * fa + da * fb).clamp(0.0, 1.0);
    if oa <= 1e-7 {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    let or = ((sr * fa + dr * fb).clamp(0.0, 1.0) / oa).clamp(0.0, 1.0);
    let og = ((sg * fa + dg * fb).clamp(0.0, 1.0) / oa).clamp(0.0, 1.0);
    let ob = ((sb * fa + db * fb).clamp(0.0, 1.0) / oa).clamp(0.0, 1.0);
    dst[0] = (or * 255.0).round() as u8;
    dst[1] = (og * 255.0).round() as u8;
    dst[2] = (ob * 255.0).round() as u8;
    dst[3] = (oa * 255.0).round() as u8;
}

fn rasterize_polygons_mask(
    buf_w: u32,
    buf_h: u32,
    scale_x: f64,
    scale_y: f64,
    polygons: &[Vec<(f64, f64)>],
    even_odd: bool,
) -> Vec<u8> {
    let w = buf_w as usize;
    let h = buf_h as usize;
    let mut mask = vec![0u8; w * h];
    if w == 0 || h == 0 || polygons.is_empty() {
        return mask;
    }

    // Build edge list in backing buffer coordinates
    struct Edge {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        dir: i32,
    }
    let mut edges = Vec::new();
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for poly in polygons {
        if poly.len() < 3 {
            continue;
        }
        for i in 0..poly.len() {
            let p0 = poly[i];
            let p1 = poly[(i + 1) % poly.len()];
            let x0 = p0.0 * scale_x;
            let y0 = p0.1 * scale_y;
            let x1 = p1.0 * scale_x;
            let y1 = p1.1 * scale_y;
            if (y1 - y0).abs() <= 1e-9
                || !x0.is_finite()
                || !y0.is_finite()
                || !x1.is_finite()
                || !y1.is_finite()
            {
                continue;
            }
            min_y = min_y.min(y0.min(y1));
            max_y = max_y.max(y0.max(y1));
            if y0 < y1 {
                edges.push(Edge {
                    x0,
                    y0,
                    x1,
                    y1,
                    dir: 1,
                });
            } else {
                edges.push(Edge {
                    x0: x1,
                    y0: y1,
                    x1: x0,
                    y1: y0,
                    dir: -1,
                });
            }
        }
    }
    if edges.is_empty() {
        return mask;
    }
    let y_start = (min_y.floor() as i64).clamp(0, h as i64) as usize;
    let y_end = (max_y.ceil() as i64).clamp(0, h as i64) as usize;
    let mut hits: Vec<(f64, i32)> = Vec::with_capacity(edges.len().min(64));
    let mut row_cov = vec![0u16; w];

    // 4x vertical supersampling + subpixel horizontal coverage
    const SUB_Y: usize = 4;
    for py in y_start..y_end {
        row_cov[..].fill(0);
        for sy in 0..SUB_Y {
            let scan_y = py as f64 + (sy as f64 + 0.5) / (SUB_Y as f64);
            hits.clear();
            for e in &edges {
                if scan_y >= e.y0 && scan_y < e.y1 {
                    let t = (scan_y - e.y0) / (e.y1 - e.y0);
                    let ix = e.x0 + t * (e.x1 - e.x0);
                    hits.push((ix, e.dir));
                }
            }
            if hits.len() < 2 {
                continue;
            }
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0i32;
            let mut parity = false;
            let mut span_start: Option<f64> = None;
            for &(ix, dir) in &hits {
                let was_inside = if even_odd { parity } else { winding != 0 };
                winding += dir;
                parity = !parity;
                let now_inside = if even_odd { parity } else { winding != 0 };
                if !was_inside && now_inside {
                    span_start = Some(ix);
                } else if was_inside
                    && !now_inside
                    && let Some(x_left) = span_start.take()
                {
                    let xl = x_left.clamp(0.0, w as f64);
                    let xr = ix.clamp(0.0, w as f64);
                    if xr > xl {
                        let c0 = xl.floor() as usize;
                        let c1 = xr.ceil().min(w as f64) as usize;
                        for (px, slot) in row_cov.iter_mut().enumerate().take(c1).skip(c0) {
                            let left = xl.max(px as f64);
                            let right = xr.min((px + 1) as f64);
                            if right > left {
                                *slot += ((right - left) * 64.0).round() as u16;
                            }
                        }
                    }
                }
            }
        }
        let row_out = &mut mask[py * w..(py + 1) * w];
        for px in 0..w {
            if row_cov[px] > 0 {
                row_out[px] = ((u32::from(row_cov[px]) * 255 + 128) / 256).min(255) as u8;
            }
        }
    }
    mask
}

fn point_in_subpaths(x: f64, y: f64, subpaths: &[Subpath], even_odd: bool) -> bool {
    let mut winding = 0i32;
    let mut crossings = 0usize;
    for sp in subpaths {
        if sp.points.len() < 3 {
            continue;
        }
        for i in 0..sp.points.len() {
            let p0 = sp.points[i];
            let p1 = sp.points[(i + 1) % sp.points.len()];
            if (p0.1 <= y && p1.1 > y) || (p1.1 <= y && p0.1 > y) {
                let t = (y - p0.1) / (p1.1 - p0.1);
                let ix = p0.0 + t * (p1.0 - p0.0);
                if x < ix {
                    crossings += 1;
                    if p0.1 <= y {
                        winding += 1;
                    } else {
                        winding -= 1;
                    }
                }
            }
        }
    }
    if even_odd {
        crossings % 2 == 1
    } else {
        winding != 0
    }
}

fn build_stroke_polygons(subpaths: &[Subpath], st: &ContextState) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    let half_w = st.line_width * 0.5;
    if half_w <= 1e-6 || !half_w.is_finite() {
        return out;
    }
    let inv_ctm = invert_matrix(&st.transform);

    for sp in subpaths {
        if sp.points.len() < 2 {
            continue;
        }
        // Convert device points back to user space if CTM is invertible so lineWidth & dashes apply in user space before CTM!
        let mut user_pts: Vec<(f64, f64)> = if let Some(ref inv) = inv_ctm {
            sp.points
                .iter()
                .map(|&(px, py)| apply_matrix(inv, px, py))
                .collect()
        } else {
            sp.points.clone()
        };
        if sp.closed
            && let Some(&first) = user_pts.first()
            && user_pts
                .last()
                .is_some_and(|&l| (l.0 - first.0).hypot(l.1 - first.1) > 1e-6)
        {
            user_pts.push(first);
        }

        // Apply line dashing in user space if active
        let polylines: Vec<(Vec<(f64, f64)>, bool)> = if st.line_dash.is_empty() {
            vec![(user_pts, sp.closed)]
        } else {
            dash_polyline(&user_pts, &st.line_dash, st.line_dash_offset)
                .into_iter()
                .map(|pl| (pl, false))
                .collect()
        };

        let xform_poly = |poly: Vec<(f64, f64)>| -> Vec<(f64, f64)> {
            if inv_ctm.is_some() {
                poly.into_iter()
                    .map(|(ux, uy)| apply_matrix(&st.transform, ux, uy))
                    .collect()
            } else {
                poly
            }
        };

        for (pts, is_closed) in polylines {
            if pts.len() < 2 {
                continue;
            }
            type StrokeSegmentDir = (usize, (f64, f64), (f64, f64));
            let seg_count = pts.len() - 1;
            let mut seg_dirs: Vec<StrokeSegmentDir> = Vec::new();
            for i in 0..seg_count {
                let dx = pts[i + 1].0 - pts[i].0;
                let dy = pts[i + 1].1 - pts[i].1;
                let len = dx.hypot(dy);
                if len > 1e-7 {
                    let ux = dx / len;
                    let uy = dy / len;
                    let nx = -uy * half_w;
                    let ny = ux * half_w;
                    // Segment quad
                    out.push(xform_poly(vec![
                        (pts[i].0 + nx, pts[i].1 + ny),
                        (pts[i + 1].0 + nx, pts[i + 1].1 + ny),
                        (pts[i + 1].0 - nx, pts[i + 1].1 - ny),
                        (pts[i].0 - nx, pts[i].1 - ny),
                    ]));
                    seg_dirs.push((i, (ux, uy), (nx, ny)));
                }
            }
            if seg_dirs.is_empty() {
                continue;
            }
            // End caps if open
            if !is_closed {
                let &(first_i, (ux0, uy0), (nx0, ny0)) = seg_dirs.first().unwrap();
                let p_start = pts[first_i];
                emit_cap(
                    &mut out,
                    p_start,
                    (-ux0, -uy0),
                    (-nx0, -ny0),
                    half_w,
                    st.line_cap,
                    &xform_poly,
                );

                let &(last_i, (ux1, uy1), (nx1, ny1)) = seg_dirs.last().unwrap();
                let p_end = pts[last_i + 1];
                emit_cap(
                    &mut out,
                    p_end,
                    (ux1, uy1),
                    (nx1, ny1),
                    half_w,
                    st.line_cap,
                    &xform_poly,
                );
            }
            // Line joins between consecutive segments
            let join_pairs = if is_closed && seg_dirs.len() >= 2 {
                seg_dirs.len()
            } else {
                seg_dirs.len().saturating_sub(1)
            };
            for j in 0..join_pairs {
                let (i0, u0, n0) = seg_dirs[j];
                let (_, u1, n1) = seg_dirs[(j + 1) % seg_dirs.len()];
                let vertex = pts[i0 + 1];
                emit_join(
                    &mut out,
                    vertex,
                    u0,
                    n0,
                    u1,
                    n1,
                    half_w,
                    st.line_join,
                    st.miter_limit,
                    &xform_poly,
                );
            }
        }
    }
    out
}

fn emit_cap<F: Fn(Vec<(f64, f64)>) -> Vec<(f64, f64)>>(
    out: &mut Vec<Vec<(f64, f64)>>,
    p: (f64, f64),
    dir: (f64, f64),
    norm: (f64, f64),
    half_w: f64,
    cap: LineCap,
    xform: &F,
) {
    match cap {
        LineCap::Butt => {}
        LineCap::Square => {
            let ex = dir.0 * half_w;
            let ey = dir.1 * half_w;
            out.push(xform(vec![
                (p.0 - norm.0, p.1 - norm.1),
                (p.0 + norm.0, p.1 + norm.1),
                (p.0 + norm.0 + ex, p.1 + norm.1 + ey),
                (p.0 - norm.0 + ex, p.1 - norm.1 + ey),
            ]));
        }
        LineCap::Round => {
            let steps = 12usize;
            let mut poly = Vec::with_capacity(steps + 2);
            poly.push(p);
            let base_ang = (-norm.1).atan2(-norm.0);
            for s in 0..=steps {
                let ang = base_ang - std::f64::consts::PI * (s as f64 / steps as f64);
                poly.push((p.0 + ang.cos() * half_w, p.1 + ang.sin() * half_w));
            }
            out.push(xform(poly));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_join<F: Fn(Vec<(f64, f64)>) -> Vec<(f64, f64)>>(
    out: &mut Vec<Vec<(f64, f64)>>,
    v: (f64, f64),
    u0: (f64, f64),
    n0: (f64, f64),
    u1: (f64, f64),
    n1: (f64, f64),
    half_w: f64,
    join: LineJoin,
    miter_limit: f64,
    xform: &F,
) {
    let cross = u0.0 * u1.1 - u0.1 * u1.0;
    if cross.abs() <= 1e-6 {
        return;
    }
    let (on0, on1) = if cross > 0.0 {
        ((-n0.0, -n0.1), (-n1.0, -n1.1))
    } else {
        (n0, n1)
    };
    match join {
        LineJoin::Round => {
            let steps = 10usize;
            let mut poly = Vec::with_capacity(steps + 2);
            poly.push(v);
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let dx = on0.0 * (1.0 - t) + on1.0 * t;
                let dy = on0.1 * (1.0 - t) + on1.1 * t;
                let len = dx.hypot(dy).max(1e-9);
                poly.push((v.0 + dx / len * half_w, v.1 + dy / len * half_w));
            }
            out.push(xform(poly));
        }
        LineJoin::Miter => {
            // Solve p0 + t * u0 = p1 + s * u1
            let p0 = (v.0 + on0.0, v.1 + on0.1);
            let p1 = (v.0 + on1.0, v.1 + on1.1);
            let det = u0.0 * (-u1.1) - u0.1 * (-u1.0);
            if det.abs() > 1e-7 {
                let rhs = (p1.0 - p0.0, p1.1 - p0.1);
                let t = (rhs.0 * (-u1.1) - rhs.1 * (-u1.0)) / det;
                let mx = p0.0 + t * u0.0;
                let my = p0.1 + t * u0.1;
                let miter_len = (mx - v.0).hypot(my - v.1);
                if miter_len <= miter_limit * half_w {
                    out.push(xform(vec![v, p0, (mx, my), p1]));
                    return;
                }
            }
            out.push(xform(vec![v, p0, p1]));
        }
        LineJoin::Bevel => {
            out.push(xform(vec![
                v,
                (v.0 + on0.0, v.1 + on0.1),
                (v.0 + on1.0, v.1 + on1.1),
            ]));
        }
    }
}

fn dash_polyline(pts: &[(f64, f64)], dash: &[f64], offset: f64) -> Vec<Vec<(f64, f64)>> {
    let mut pattern = dash.to_vec();
    if pattern.len() % 2 == 1 {
        let copy = pattern.clone();
        pattern.extend(copy);
    }
    let total_pat: f64 = pattern.iter().sum();
    if total_pat <= 1e-6 || !total_pat.is_finite() {
        return vec![pts.to_vec()];
    }
    let mut rem_off = offset.rem_euclid(total_pat);
    let mut pat_idx = 0usize;
    while pat_idx < pattern.len() && rem_off >= pattern[pat_idx] {
        rem_off -= pattern[pat_idx];
        pat_idx = (pat_idx + 1) % pattern.len();
    }
    let mut pat_rem = pattern[pat_idx] - rem_off;
    let mut out = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    for seg in pts.windows(2) {
        let mut p0 = seg[0];
        let p1 = seg[1];
        let mut seg_rem = (p1.0 - p0.0).hypot(p1.1 - p0.1);
        if seg_rem <= 1e-9 {
            continue;
        }
        let ux = (p1.0 - p0.0) / seg_rem;
        let uy = (p1.1 - p0.1) / seg_rem;
        while seg_rem > 1e-7 {
            let step = seg_rem.min(pat_rem.max(1e-7));
            let next_p = (p0.0 + ux * step, p0.1 + uy * step);
            if pat_idx.is_multiple_of(2) {
                if cur.is_empty() {
                    cur.push(p0);
                }
                cur.push(next_p);
            }
            p0 = next_p;
            seg_rem -= step;
            pat_rem -= step;
            if pat_rem <= 1e-7 {
                if pat_idx.is_multiple_of(2) && cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                pat_idx = (pat_idx + 1) % pattern.len();
                pat_rem = pattern[pat_idx];
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

fn box_blur_alpha(mask: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    if radius == 0 || w == 0 || h == 0 {
        return mask.to_vec();
    }
    let r = radius.min(32);
    let diam = (2 * r + 1) as u32;
    let mut tmp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        let row = &mask[y * w..(y + 1) * w];
        let trow = &mut tmp[y * w..(y + 1) * w];
        for (x, slot) in trow.iter_mut().enumerate() {
            let mut sum = 0u32;
            for k in 0..=(2 * r) {
                let sx = (x as isize + k as isize - r as isize).clamp(0, (w - 1) as isize) as usize;
                sum += u32::from(row[sx]);
            }
            *slot = (sum / diam) as u8;
        }
    }
    for x in 0..w {
        for y in 0..h {
            let mut sum = 0u32;
            for k in 0..=(2 * r) {
                let sy = (y as isize + k as isize - r as isize).clamp(0, (h - 1) as isize) as usize;
                sum += u32::from(tmp[sy * w + x]);
            }
            out[y * w + x] = (sum / diam) as u8;
        }
    }
    out
}

fn apply_mask_paint(
    surf: &mut CanvasSurface,
    gradients: &[GradientDef],
    mask: &[u8],
    paint: &PaintStyle,
) {
    let w = surf.buf_w as usize;
    let h = surf.buf_h as usize;
    if w == 0 || h == 0 || surf.state.global_alpha <= 0.0 {
        return;
    }

    // Optional drop shadow pass when shadow alpha > 0 and blur/offset active
    if surf.state.shadow_color[3] > 0
        && (surf.state.shadow_blur > 0.0
            || surf.state.shadow_offset_x != 0.0
            || surf.state.shadow_offset_y != 0.0)
    {
        let ox = (surf.state.shadow_offset_x * surf.scale_x).round() as isize;
        let oy = (surf.state.shadow_offset_y * surf.scale_y).round() as isize;
        let blur_r = ((surf.state.shadow_blur * 0.5 * surf.scale_x).round() as usize).min(24);
        let blurred = box_blur_alpha(mask, w, h, blur_r);
        let sc = surf.state.shadow_color;
        let ga = surf.state.global_alpha;
        for dy in 0..h {
            let sy = dy as isize - oy;
            if sy < 0 || sy >= h as isize {
                continue;
            }
            for dx in 0..w {
                let sx = dx as isize - ox;
                if sx < 0 || sx >= w as isize {
                    continue;
                }
                let cov = blurred[(sy as usize) * w + (sx as usize)];
                if cov == 0 {
                    continue;
                }
                let clip_cov = surf
                    .state
                    .clip_mask
                    .as_ref()
                    .map_or(255, |cm| cm[dy * w + dx]);
                if clip_cov == 0 {
                    continue;
                }
                let eff_a = (f64::from(sc[3])
                    * (f64::from(cov) / 255.0)
                    * (f64::from(clip_cov) / 255.0)
                    * ga)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                if eff_a > 0 {
                    let idx = (dy * w + dx) * 4;
                    composite_pixel(
                        &mut surf.pixels[idx..idx + 4],
                        [sc[0], sc[1], sc[2], eff_a],
                        surf.state.composite_op,
                    );
                }
            }
        }
    }

    let inv_ctm = invert_matrix(&surf.state.transform);
    let ga = surf.state.global_alpha;
    let is_unbounded_op = matches!(
        surf.state.composite_op,
        CompositeOp::Copy
            | CompositeOp::Clear
            | CompositeOp::SourceIn
            | CompositeOp::DestinationIn
            | CompositeOp::SourceOut
            | CompositeOp::DestinationAtop
    );

    for py in 0..h {
        for px in 0..w {
            let cov = mask[py * w + px];
            if cov == 0 && !is_unbounded_op {
                continue;
            }
            let clip_cov = surf
                .state
                .clip_mask
                .as_ref()
                .map_or(255, |cm| cm[py * w + px]);
            if clip_cov == 0 {
                continue;
            }
            let base_color = match paint {
                PaintStyle::Color(c) => *c,
                PaintStyle::Gradient(gid) => {
                    let Some(grad) = gradients.get(*gid) else {
                        continue;
                    };
                    let lx = (px as f64 + 0.5) / surf.scale_x;
                    let ly = (py as f64 + 0.5) / surf.scale_y;
                    let (ux, uy) = match inv_ctm {
                        Some(ref inv) => apply_matrix(inv, lx, ly),
                        None => (lx, ly),
                    };
                    grad.sample_at(ux, uy)
                }
            };
            let eff_a = (f64::from(base_color[3])
                * (f64::from(cov) / 255.0)
                * (f64::from(clip_cov) / 255.0)
                * ga)
                .round()
                .clamp(0.0, 255.0) as u8;
            let idx = (py * w + px) * 4;
            composite_pixel(
                &mut surf.pixels[idx..idx + 4],
                [base_color[0], base_color[1], base_color[2], eff_a],
                surf.state.composite_op,
            );
        }
    }
    surf.dirty = true;
}

fn draw_text_bitmap(
    surf: &mut CanvasSurface,
    gradients: &[GradientDef],
    text: &str,
    x: f64,
    y: f64,
    max_width: Option<f64>,
    is_stroke: bool,
) {
    let w = surf.buf_w as usize;
    let h = surf.buf_h as usize;
    if w == 0 || h == 0 || text.is_empty() {
        return;
    }
    let font_px = surf.state.font_px;
    let bold = surf.state.font_bold;
    let raw_w = measure_text_width(text, font_px, bold);
    if raw_w <= 0.0 {
        return;
    }
    let horiz_squeeze = match max_width {
        Some(mw) if mw <= 0.0 || !mw.is_finite() => return,
        Some(mw) if raw_w > mw => mw / raw_w,
        _ => 1.0,
    };
    let eff_w = raw_w * horiz_squeeze;
    let align_dx = match surf.state.text_align {
        TextAlign::Left | TextAlign::Start => 0.0,
        TextAlign::Center => -eff_w * 0.5,
        TextAlign::Right | TextAlign::End => -eff_w,
    };
    let baseline_dy = match surf.state.text_baseline {
        TextBaseline::Top => 0.0,
        TextBaseline::Hanging => -font_px * 0.12,
        TextBaseline::Middle => -font_px * 0.5,
        TextBaseline::Alphabetic => -font_px * 0.78,
        TextBaseline::Ideographic => -font_px * 0.88,
        TextBaseline::Bottom => -font_px,
    };

    let cell_w = font_px * (if bold { 0.65 } else { 0.6 }) * horiz_squeeze;
    let cell_h = font_px;
    let pix_w = (cell_w / 8.0).max(0.5);
    let pix_h = (cell_h / 8.0).max(0.5);
    let bold_extra = if bold { pix_w * 0.35 } else { 0.0 };

    let mut quads: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cursor_x = x + align_dx;
    let top_y = y + baseline_dy;
    let m = surf.state.transform;

    for ch in text.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        let code = u32::from(ch);
        let glyph_idx = if (0x20..=0x7E).contains(&code) {
            (code - 0x20) as usize
        } else {
            (b'?' - 0x20) as usize
        };
        let rows = FONT_8X8[glyph_idx];
        for (ry, &row_bits) in rows.iter().enumerate() {
            if row_bits == 0 {
                continue;
            }
            let mut col = 0usize;
            while col < 8 {
                if (row_bits & (0x80 >> col)) != 0 {
                    let run_start = col;
                    while col < 8 && (row_bits & (0x80 >> col)) != 0 {
                        col += 1;
                    }
                    let gx0 = cursor_x + (run_start as f64) * pix_w;
                    let gx1 = cursor_x + (col as f64) * pix_w + bold_extra;
                    let gy0 = top_y + (ry as f64) * pix_h;
                    let gy1 = gy0 + pix_h;
                    quads.push(vec![
                        apply_matrix(&m, gx0, gy0),
                        apply_matrix(&m, gx1, gy0),
                        apply_matrix(&m, gx1, gy1),
                        apply_matrix(&m, gx0, gy1),
                    ]);
                } else {
                    col += 1;
                }
            }
        }
        cursor_x += cell_w;
    }

    if quads.is_empty() {
        return;
    }
    let mask = rasterize_polygons_mask(
        surf.buf_w,
        surf.buf_h,
        surf.scale_x,
        surf.scale_y,
        &quads,
        false,
    );
    let paint = if is_stroke {
        surf.state.stroke_style.clone()
    } else {
        surf.state.fill_style.clone()
    };
    apply_mask_paint(surf, gradients, &mask, &paint);
}

// --- Path Construction Helpers ---

fn push_point_to_subpath(surf: &mut CanvasSurface, dx: f64, dy: f64) {
    if !dx.is_finite() || !dy.is_finite() {
        return;
    }
    if let Some(last) = surf.subpaths.last_mut()
        && !last.closed
    {
        if last.points.len() < MAX_POINTS_PER_SUBPATH {
            last.points.push((dx, dy));
        }
        return;
    }
    if surf.subpaths.len() < MAX_SUBPATHS {
        surf.subpaths.push(Subpath {
            points: vec![(dx, dy)],
            closed: false,
        });
    }
}

fn ensure_subpath_at(surf: &mut CanvasSurface, dx: f64, dy: f64) {
    if !dx.is_finite() || !dy.is_finite() {
        return;
    }
    let has_open = surf
        .subpaths
        .last()
        .is_some_and(|sp| !sp.closed && !sp.points.is_empty());
    if !has_open && surf.subpaths.len() < MAX_SUBPATHS {
        surf.subpaths.push(Subpath {
            points: vec![(dx, dy)],
            closed: false,
        });
    }
}

fn subpath_current_point(surf: &CanvasSurface) -> Option<(f64, f64)> {
    surf.subpaths
        .last()
        .and_then(|sp| sp.points.last().copied())
}

fn append_quadratic(surf: &mut CanvasSurface, p0: (f64, f64), cp: (f64, f64), p2: (f64, f64)) {
    let chord = (p2.0 - p0.0).hypot(p2.1 - p0.1) + (cp.0 - p0.0).hypot(cp.1 - p0.1);
    let steps = ((chord * 0.5).ceil() as usize).clamp(8, 64);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let x = mt * mt * p0.0 + 2.0 * mt * t * cp.0 + t * t * p2.0;
        let y = mt * mt * p0.1 + 2.0 * mt * t * cp.1 + t * t * p2.1;
        push_point_to_subpath(surf, x, y);
    }
}

fn append_cubic(
    surf: &mut CanvasSurface,
    p0: (f64, f64),
    c1: (f64, f64),
    c2: (f64, f64),
    p3: (f64, f64),
) {
    let chord = (c1.0 - p0.0).hypot(c1.1 - p0.1)
        + (c2.0 - c1.0).hypot(c2.1 - c1.1)
        + (p3.0 - c2.0).hypot(p3.1 - c2.1);
    let steps = ((chord * 0.5).ceil() as usize).clamp(12, 96);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.0
            + 3.0 * mt * mt * t * c1.0
            + 3.0 * mt * t * t * c2.0
            + t * t * t * p3.0;
        let y = mt * mt * mt * p0.1
            + 3.0 * mt * mt * t * c1.1
            + 3.0 * mt * t * t * c2.1
            + t * t * t * p3.1;
        push_point_to_subpath(surf, x, y);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_ellipse_arc(
    surf: &mut CanvasSurface,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    rotation: f64,
    start_angle: f64,
    end_angle: f64,
    ccw: bool,
) {
    let tau = std::f64::consts::TAU;
    let delta = if !ccw && end_angle - start_angle >= tau {
        tau
    } else if ccw && start_angle - end_angle >= tau {
        -tau
    } else if !ccw {
        let mut d = (end_angle - start_angle).rem_euclid(tau);
        if d == 0.0 && end_angle != start_angle {
            d = tau;
        }
        d
    } else {
        let mut d = (start_angle - end_angle).rem_euclid(tau);
        if d == 0.0 && end_angle != start_angle {
            d = tau;
        }
        -d
    };

    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    let m = surf.state.transform;
    let eval_pt = |ang: f64| -> (f64, f64) {
        let ex = rx * ang.cos();
        let ey = ry * ang.sin();
        let ux = cx + ex * cos_r - ey * sin_r;
        let uy = cy + ex * sin_r + ey * cos_r;
        apply_matrix(&m, ux, uy)
    };

    let p_first = eval_pt(start_angle);
    if surf
        .subpaths
        .last()
        .is_some_and(|sp| !sp.closed && !sp.points.is_empty())
    {
        push_point_to_subpath(surf, p_first.0, p_first.1);
    } else if surf.subpaths.len() < MAX_SUBPATHS {
        surf.subpaths.push(Subpath {
            points: vec![p_first],
            closed: false,
        });
    }

    let max_r = rx.max(ry).max(1.0);
    let steps = ((delta.abs() * max_r * 0.75).ceil() as usize).clamp(16, 180);
    for i in 1..=steps {
        let ang = start_angle + delta * (i as f64 / steps as f64);
        let p = eval_pt(ang);
        push_point_to_subpath(surf, p.0, p.1);
    }
}

// --- JS Bindings (`<canvas>` methods + `CanvasRenderingContext2D`) ---

pub(super) fn install_globals(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    for ctor_name in ["CanvasRenderingContext2D", "CanvasGradient", "ImageData"] {
        let proto = JsObject::with_object_proto(context.intrinsics());
        let is_image_data = ctor_name == "ImageData";
        let ctor: JsObject = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |new_target, args, &is_img, ctx| {
                    modern::check_context(ctx)?;
                    if new_target.is_undefined() || !is_img {
                        return Err(failure("Illegal constructor"));
                    }
                    construct_image_data_from_args(args, ctx)
                },
                is_image_data,
            ),
        )
        .name(JsString::from(ctor_name))
        .length(if is_image_data { 2 } else { 0 })
        .constructor(true)
        .build()
        .into();
        link_ctor_global(&global, ctor_name, &ctor, &proto, context)?;
    }
    Ok(())
}

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        CANVAS_GET_CONTEXT => Some(("getContext", 1)),
        CANVAS_TO_DATA_URL => Some(("toDataURL", 0)),
        CANVAS_TO_BLOB => Some(("toBlob", 1)),
        _ => None,
    }
}

fn receiver_canvas_node(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Some(obj) = this.as_object() else {
        return Err(failure(
            "Canvas method requires a <canvas> element receiver",
        ));
    };
    let handle = {
        let brands = state(context).borrow().brands.clone();
        brands
            .get(&obj, context)?
            .as_string()
            .map(|s| s.to_std_string_escaped())
    };
    let Some(h) = handle else {
        return Err(failure(
            "Canvas method requires a <canvas> element receiver",
        ));
    };
    let shared = state(context);
    let data = shared.borrow();
    let id = data.host.node(&h).map_err(failure)?;
    if data.host.document.nodes[id].tag != "canvas" {
        return Err(failure(
            "Canvas method requires a <canvas> element receiver",
        ));
    }
    Ok(id)
}

pub(super) fn call_canvas_element_method(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let node_id = receiver_canvas_node(this, context)?;
    match name {
        CANVAS_GET_CONTEXT => {
            let ctx_type = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            modern::check_context(context)?;
            if ctx_type != "2d" {
                return Ok(JsValue::null());
            }
            let world_id = state(context).borrow().active_world;
            if let Some(existing) = state(context)
                .borrow()
                .canvas
                .contexts
                .iter()
                .find(|c| c.world_id == world_id && c.node_id == node_id)
                .map(|c| c.object.clone())
            {
                return Ok(existing.into());
            }
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                ensure_surface(&mut data, node_id).map_err(failure)?;
                if let Some(surf) = data.canvas.surfaces.get_mut(&node_id) {
                    surf.has_context_2d = true;
                }
            }
            let ctx_obj = build_context_2d_object(node_id, context)?;
            state(context)
                .borrow_mut()
                .canvas
                .contexts
                .push(RetainedContext2D {
                    world_id,
                    node_id,
                    object: ctx_obj.clone(),
                });
            Ok(ctx_obj.into())
        }
        CANVAS_TO_DATA_URL => {
            if let Some(arg0) = args.first()
                && !arg0.is_undefined()
            {
                let _ = utf8(arg0, context)?;
                modern::check_context(context)?;
            }
            let data_url = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                ensure_surface(&mut data, node_id).map_err(failure)?;
                let surf = data.canvas.surfaces.get(&node_id).unwrap();
                if surf.logical_w == 0 || surf.logical_h == 0 || surf.buf_w == 0 || surf.buf_h == 0
                {
                    "data:,".to_owned()
                } else {
                    let png = encode_png_rgba(surf.buf_w, surf.buf_h, &surf.pixels);
                    data.host.charge(png.len()).map_err(failure)?;
                    format!("data:image/png;base64,{}", base64_encode(&png))
                }
            };
            Ok(JsString::from(data_url.as_str()).into())
        }
        CANVAS_TO_BLOB => {
            let Some(cb) = args.first().and_then(JsValue::as_callable) else {
                return Err(failure(
                    "HTMLCanvasElement.toBlob requires a callback function",
                ));
            };
            if let Some(arg1) = args.get(1)
                && !arg1.is_undefined()
            {
                let _ = utf8(arg1, context)?;
                modern::check_context(context)?;
            }
            let png_opt = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                ensure_surface(&mut data, node_id).map_err(failure)?;
                let surf = data.canvas.surfaces.get(&node_id).unwrap();
                if surf.logical_w == 0 || surf.logical_h == 0 || surf.buf_w == 0 || surf.buf_h == 0
                {
                    None
                } else {
                    let png = encode_png_rgba(surf.buf_w, surf.buf_h, &surf.pixels);
                    data.host.charge(png.len()).map_err(failure)?;
                    Some(png)
                }
            };
            let blob_val = match png_opt {
                None => JsValue::null(),
                Some(png) => build_png_blob(&png, context)?,
            };
            let set_timeout = context
                .global_object()
                .get(js_string!("setTimeout"), context)?
                .as_callable()
                .ok_or_else(|| failure("setTimeout unavailable"))?;
            set_timeout.call(
                &context.global_object().into(),
                &[cb.into(), 0.into(), blob_val],
                context,
            )?;
            Ok(JsValue::undefined())
        }
        _ => Err(failure("Unknown canvas element method")),
    }
}

fn build_png_blob(png: &[u8], context: &mut Context) -> JsResult<JsValue> {
    let u8a = JsUint8Array::from_iter(png.iter().copied(), context)?;
    if let Ok(blob_ctor_val) = context.global_object().get(js_string!("Blob"), context)
        && let Some(blob_ctor) = blob_ctor_val.as_object()
        && blob_ctor.is_constructor()
    {
        let parts = JsArray::from_iter([JsValue::from(JsObject::from(u8a))], context);
        let opts = ObjectInitializer::new(context)
            .property(
                js_string!("type"),
                js_string!("image/png"),
                Attribute::ENUMERABLE | Attribute::CONFIGURABLE | Attribute::WRITABLE,
            )
            .build();
        return blob_ctor
            .construct(&[parts.into(), opts.into()], None, context)
            .map(JsValue::from);
    }
    let size = png.len() as f64;
    let obj = ObjectInitializer::new(context)
        .property(js_string!("size"), size, Attribute::ENUMERABLE)
        .property(
            js_string!("type"),
            js_string!("image/png"),
            Attribute::ENUMERABLE,
        )
        .build();
    Ok(obj.into())
}

fn ctx_node_id(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Some(obj) = this.as_object() else {
        return Err(failure(
            "CanvasRenderingContext2D method requires context receiver",
        ));
    };
    let nid_val = obj.get(js_string!("__mg_canvas_id"), context)?;
    let Some(nid_num) = nid_val.as_number() else {
        return Err(failure("Invalid CanvasRenderingContext2D receiver"));
    };
    let node_id = nid_num as usize;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if node_id >= data.host.document.nodes.len()
        || data.host.document.nodes[node_id].tag != "canvas"
    {
        return Err(failure("Stale CanvasRenderingContext2D target"));
    }
    ensure_surface(&mut data, node_id).map_err(failure)?;
    Ok(node_id)
}

fn finite_arg(args: &[JsValue], idx: usize, context: &mut Context) -> JsResult<Option<f64>> {
    let v = args
        .get(idx)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?;
    Ok(v.is_finite().then_some(v))
}

fn require_finite_arg(args: &[JsValue], idx: usize, context: &mut Context) -> JsResult<f64> {
    let v = args
        .get(idx)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?;
    if !v.is_finite() {
        return Err(JsNativeError::typ()
            .with_message("Non-finite floating-point value is not allowed")
            .into());
    }
    Ok(v)
}

fn build_gradient_object(grad_id: usize, context: &mut Context) -> JsResult<JsObject> {
    let world_id = state(context).borrow().active_world;
    if let Some(existing) = state(context)
        .borrow()
        .canvas
        .gradient_objects
        .iter()
        .find(|g| g.world_id == world_id && g.grad_id == grad_id)
        .map(|g| g.object.clone())
    {
        return Ok(existing);
    }
    let proto = context
        .global_object()
        .get(js_string!("CanvasGradient"), context)
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|c| c.get(js_string!("prototype"), context).ok())
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let gobj = JsObject::from_proto_and_data(proto, ());
    def_prop(
        &gobj,
        js_string!("__mg_grad_id"),
        grad_id as f64,
        false,
        false,
        false,
        context,
    )?;
    def_fn(
        &gobj,
        "addColorStop",
        2,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let Some(obj) = this.as_object() else {
                return Err(failure("CanvasGradient.addColorStop requires receiver"));
            };
            let gid = obj
                .get(js_string!("__mg_grad_id"), ctx)?
                .as_number()
                .ok_or_else(|| failure("Invalid CanvasGradient receiver"))?
                as usize;
            let offset = args
                .first()
                .unwrap_or(&JsValue::undefined())
                .to_number(ctx)?;
            if !offset.is_finite() || !(0.0..=1.0).contains(&offset) {
                return Err(dom_exception::throw(
                    "IndexSizeError",
                    "CanvasGradient color stop offset must be in [0, 1]",
                    ctx,
                ));
            }
            let color_str = utf8(args.get(1).unwrap_or(&JsValue::undefined()), ctx)?;
            modern::check_context(ctx)?;
            let Some(rgba) = parse_canvas_color(&color_str) else {
                return Err(dom_exception::throw(
                    "SyntaxError",
                    "Invalid CSS color string in addColorStop",
                    ctx,
                ));
            };
            let shared = state(ctx);
            let mut data = shared.borrow_mut();
            if let Some(grad) = data.canvas.gradients.get_mut(gid)
                && grad.stops.len() < 128
            {
                let pos = grad.stops.partition_point(|&(t, _)| t <= offset);
                grad.stops.insert(pos, (offset, rgba));
            }
            Ok(JsValue::undefined())
        },
        context,
    )?;
    state(context)
        .borrow_mut()
        .canvas
        .gradient_objects
        .push(RetainedGradient {
            world_id,
            grad_id,
            object: gobj.clone(),
        });
    Ok(gobj)
}

fn construct_image_data(
    width: u32,
    height: u32,
    rgba: Option<&[u8]>,
    context: &mut Context,
) -> JsResult<JsValue> {
    if width == 0 || height == 0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "ImageData dimensions must be non-zero",
            context,
        ));
    }
    if width > MAX_CANVAS_SIDE * 2
        || height > MAX_CANVAS_SIDE * 2
        || (width as usize).saturating_mul(height as usize) > MAX_CANVAS_PIXELS
    {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "ImageData exceeds maximum pixel budget",
            context,
        ));
    }
    let byte_len = (width as usize) * (height as usize) * 4;
    state(context)
        .borrow_mut()
        .host
        .charge(byte_len)
        .map_err(failure)?;
    let ab = JsArrayBuffer::new(byte_len, context)?;
    if let Some(src) = rgba
        && let Some(mut slice) = ab.data_mut()
    {
        let n = src.len().min(byte_len);
        slice[..n].copy_from_slice(&src[..n]);
    }
    let ab_obj: JsObject = ab.into();
    let clamped_obj = context
        .intrinsics()
        .constructors()
        .typed_uint8clamped_array()
        .constructor()
        .construct(
            &[
                JsValue::from(ab_obj),
                JsValue::from(0),
                JsValue::from(byte_len as f64),
            ],
            None,
            context,
        )?;
    let proto = context
        .global_object()
        .get(js_string!("ImageData"), context)
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|c| c.get(js_string!("prototype"), context).ok())
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let img_obj = JsObject::from_proto_and_data(proto, ());
    def_prop(
        &img_obj,
        js_string!("width"),
        width as f64,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &img_obj,
        js_string!("height"),
        height as f64,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &img_obj,
        js_string!("data"),
        clamped_obj,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &img_obj,
        js_string!("colorSpace"),
        js_string!("srgb"),
        false,
        true,
        true,
        context,
    )?;
    Ok(img_obj.into())
}

fn construct_image_data_from_args(args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    if args.len() < 2 {
        return Err(JsNativeError::typ()
            .with_message("ImageData constructor requires at least 2 arguments")
            .into());
    }
    if let Some(first_obj) = args[0].as_object()
        && let Ok(ta) = JsTypedArray::from_object(first_obj)
    {
        let buf_val = ta.buffer(context)?;
        let buf_obj = buf_val
            .as_object()
            .ok_or_else(|| failure("Invalid TypedArray buffer"))?;
        let ab = JsArrayBuffer::from_object(buf_obj)?;
        let boff = ta.byte_offset(context)?;
        let blen = ta.byte_length(context)?;
        if blen == 0 || blen % 4 != 0 {
            return Err(dom_exception::throw(
                "IndexSizeError",
                "ImageData TypedArray length must be a non-zero multiple of 4",
                context,
            ));
        }
        let sw = args[1].to_u32(context)?;
        if sw == 0 || (blen / 4) % (sw as usize) != 0 {
            return Err(dom_exception::throw(
                "IndexSizeError",
                "ImageData width does not divide pixel count",
                context,
            ));
        }
        let sh = if let Some(a2) = args.get(2)
            && !a2.is_undefined()
        {
            a2.to_u32(context)?
        } else {
            ((blen / 4) / (sw as usize)) as u32
        };
        if (sw as usize) * (sh as usize) * 4 != blen {
            return Err(dom_exception::throw(
                "IndexSizeError",
                "ImageData dimensions do not match TypedArray length",
                context,
            ));
        }
        let bytes = {
            let guard = ab.data();
            let slice = guard
                .as_deref()
                .ok_or_else(|| failure("Detached TypedArray buffer"))?;
            slice[boff..boff + blen].to_vec()
        };
        return construct_image_data(sw, sh, Some(&bytes), context);
    }
    let sw = args[0].to_u32(context)?;
    let sh = args[1].to_u32(context)?;
    construct_image_data(sw, sh, None, context)
}

fn read_paint_style(style: &PaintStyle, context: &mut Context) -> JsResult<JsValue> {
    match style {
        PaintStyle::Color(c) => Ok(JsString::from(serialize_canvas_color(*c).as_str()).into()),
        PaintStyle::Gradient(gid) => build_gradient_object(*gid, context).map(JsValue::from),
    }
}

fn parse_paint_style_arg(val: &JsValue, context: &mut Context) -> JsResult<Option<PaintStyle>> {
    if let Some(obj) = val.as_object() {
        if let Ok(gid_val) = obj.get(js_string!("__mg_grad_id"), context)
            && let Some(gid) = gid_val.as_number()
        {
            return Ok(Some(PaintStyle::Gradient(gid as usize)));
        }
        return Ok(None);
    }
    if let Some(s) = val.as_string() {
        let std_s = s.to_std_string_escaped();
        return Ok(parse_canvas_color(&std_s).map(PaintStyle::Color));
    }
    Ok(None)
}

fn ctx_prop_get(this: &JsValue, prop: &str, ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    match prop {
        "fillStyle" => {
            let s = state(ctx).borrow().canvas.surfaces[&nid]
                .state
                .fill_style
                .clone();
            read_paint_style(&s, ctx)
        }
        "strokeStyle" => {
            let s = state(ctx).borrow().canvas.surfaces[&nid]
                .state
                .stroke_style
                .clone();
            read_paint_style(&s, ctx)
        }
        _ => {
            let s = state(ctx);
            let d = s.borrow();
            let st = &d.canvas.surfaces[&nid].state;
            Ok(match prop {
                "lineWidth" => st.line_width.into(),
                "lineCap" => JsString::from(match st.line_cap {
                    LineCap::Butt => "butt",
                    LineCap::Round => "round",
                    LineCap::Square => "square",
                })
                .into(),
                "lineJoin" => JsString::from(match st.line_join {
                    LineJoin::Miter => "miter",
                    LineJoin::Round => "round",
                    LineJoin::Bevel => "bevel",
                })
                .into(),
                "miterLimit" => st.miter_limit.into(),
                "lineDashOffset" => st.line_dash_offset.into(),
                "globalAlpha" => st.global_alpha.into(),
                "globalCompositeOperation" => JsString::from(st.composite_op.as_str()).into(),
                "font" => JsString::from(st.font_raw.as_str()).into(),
                "textAlign" => JsString::from(st.text_align.as_str()).into(),
                "textBaseline" => JsString::from(st.text_baseline.as_str()).into(),
                "shadowColor" => {
                    JsString::from(serialize_canvas_color(st.shadow_color).as_str()).into()
                }
                "shadowBlur" => st.shadow_blur.into(),
                "shadowOffsetX" => st.shadow_offset_x.into(),
                "shadowOffsetY" => st.shadow_offset_y.into(),
                _ => JsValue::undefined(),
            })
        }
    }
}

fn ctx_prop_set(
    this: &JsValue,
    args: &[JsValue],
    prop: &str,
    ctx: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    match prop {
        "fillStyle" | "strokeStyle" => {
            if let Some(val) = args.first()
                && let Some(parsed) = parse_paint_style_arg(val, ctx)?
                && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
            {
                if prop == "fillStyle" {
                    surf.state.fill_style = parsed;
                } else {
                    surf.state.stroke_style = parsed;
                }
            }
        }
        "lineWidth" | "miterLimit" | "lineDashOffset" | "globalAlpha" | "shadowBlur"
        | "shadowOffsetX" | "shadowOffsetY" => {
            if let Some(v) = finite_arg(args, 0, ctx)?
                && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
            {
                let st = &mut surf.state;
                match prop {
                    "lineWidth" if v > 0.0 => st.line_width = v,
                    "miterLimit" if v > 0.0 => st.miter_limit = v,
                    "lineDashOffset" => st.line_dash_offset = v,
                    "globalAlpha" if (0.0..=1.0).contains(&v) => st.global_alpha = v,
                    "shadowBlur" if v >= 0.0 => st.shadow_blur = v,
                    "shadowOffsetX" => st.shadow_offset_x = v,
                    "shadowOffsetY" => st.shadow_offset_y = v,
                    _ => {}
                }
            }
        }
        _ => {
            let raw = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
                let st = &mut surf.state;
                match prop {
                    "lineCap" => match raw.as_str() {
                        "butt" => st.line_cap = LineCap::Butt,
                        "round" => st.line_cap = LineCap::Round,
                        "square" => st.line_cap = LineCap::Square,
                        _ => {}
                    },
                    "lineJoin" => match raw.as_str() {
                        "miter" => st.line_join = LineJoin::Miter,
                        "round" => st.line_join = LineJoin::Round,
                        "bevel" => st.line_join = LineJoin::Bevel,
                        _ => {}
                    },
                    "globalCompositeOperation" => {
                        if let Some(op) = CompositeOp::parse(&raw) {
                            st.composite_op = op;
                        }
                    }
                    "font" => {
                        if let Some((px, bold, norm)) = parse_css_font_spec(&raw) {
                            st.font_px = px;
                            st.font_bold = bold;
                            st.font_raw = norm;
                        }
                    }
                    "textAlign" => {
                        if let Some(ta) = TextAlign::parse(&raw) {
                            st.text_align = ta;
                        }
                    }
                    "textBaseline" => {
                        if let Some(tb) = TextBaseline::parse(&raw) {
                            st.text_baseline = tb;
                        }
                    }
                    "shadowColor" => {
                        if let Some(c) = parse_canvas_color(&raw) {
                            st.shadow_color = c;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(JsValue::undefined())
}

// --- State Stack & Transforms ---

fn ctx_save(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid)
        && surf.state_stack.len() < MAX_STATE_STACK
    {
        let cloned = surf.state.clone();
        surf.state_stack.push(cloned);
    }
    Ok(JsValue::undefined())
}

fn ctx_restore(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid)
        && let Some(prev) = surf.state_stack.pop()
    {
        surf.state = prev;
    }
    Ok(JsValue::undefined())
}

fn ctx_scale(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(sx), Some(sy)) = (finite_arg(args, 0, ctx)?, finite_arg(args, 1, ctx)?)
        && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        surf.state.transform = mul_matrix(&surf.state.transform, &[sx, 0.0, 0.0, sy, 0.0, 0.0]);
    }
    Ok(JsValue::undefined())
}

fn ctx_rotate(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let Some(ang) = finite_arg(args, 0, ctx)?
        && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        let (s, c) = ang.sin_cos();
        surf.state.transform = mul_matrix(&surf.state.transform, &[c, s, -s, c, 0.0, 0.0]);
    }
    Ok(JsValue::undefined())
}

fn ctx_translate(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(tx), Some(ty)) = (finite_arg(args, 0, ctx)?, finite_arg(args, 1, ctx)?)
        && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        surf.state.transform = mul_matrix(&surf.state.transform, &[1.0, 0.0, 0.0, 1.0, tx, ty]);
    }
    Ok(JsValue::undefined())
}

fn ctx_transform(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
        finite_arg(args, 5, ctx)?,
    ) && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        surf.state.transform = mul_matrix(&surf.state.transform, &[a, b, c, d, e, f]);
    }
    Ok(JsValue::undefined())
}

fn ctx_set_transform(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if args.is_empty() {
        if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
            surf.state.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        }
        return Ok(JsValue::undefined());
    }
    if args.len() == 1
        && let Some(obj) = args[0].as_object()
    {
        let get_f =
            |k1: &'static str, k2: &'static str, def: f64, c: &mut Context| -> JsResult<f64> {
                let v1 = obj.get(JsString::from(k1), c)?;
                if !v1.is_undefined() {
                    return v1.to_number(c);
                }
                let v2 = obj.get(JsString::from(k2), c)?;
                if !v2.is_undefined() {
                    return v2.to_number(c);
                }
                Ok(def)
            };
        let a = get_f("a", "m11", 1.0, ctx)?;
        let b = get_f("b", "m12", 0.0, ctx)?;
        let c = get_f("c", "m21", 0.0, ctx)?;
        let d = get_f("d", "m22", 1.0, ctx)?;
        let e = get_f("e", "m41", 0.0, ctx)?;
        let f = get_f("f", "m42", 0.0, ctx)?;
        if [a, b, c, d, e, f].iter().all(|v| v.is_finite())
            && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
        {
            surf.state.transform = [a, b, c, d, e, f];
        }
        return Ok(JsValue::undefined());
    }
    if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
        finite_arg(args, 5, ctx)?,
    ) && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        surf.state.transform = [a, b, c, d, e, f];
    }
    Ok(JsValue::undefined())
}

fn ctx_reset_transform(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
        surf.state.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    Ok(JsValue::undefined())
}

fn ctx_get_transform(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let m = state(ctx).borrow().canvas.surfaces[&nid].state.transform;
    let is_2d_identity = (m[0] - 1.0).abs() <= 1e-12
        && m[1].abs() <= 1e-12
        && m[2].abs() <= 1e-12
        && (m[3] - 1.0).abs() <= 1e-12
        && m[4].abs() <= 1e-12
        && m[5].abs() <= 1e-12;
    let attr = Attribute::ENUMERABLE | Attribute::CONFIGURABLE | Attribute::WRITABLE;
    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("a"), m[0], attr)
        .property(js_string!("b"), m[1], attr)
        .property(js_string!("c"), m[2], attr)
        .property(js_string!("d"), m[3], attr)
        .property(js_string!("e"), m[4], attr)
        .property(js_string!("f"), m[5], attr)
        .property(js_string!("m11"), m[0], attr)
        .property(js_string!("m12"), m[1], attr)
        .property(js_string!("m21"), m[2], attr)
        .property(js_string!("m22"), m[3], attr)
        .property(js_string!("m41"), m[4], attr)
        .property(js_string!("m42"), m[5], attr)
        .property(js_string!("is2D"), true, attr)
        .property(js_string!("isIdentity"), is_2d_identity, attr)
        .build();
    Ok(obj.into())
}

// --- Rectangles ---

fn rect_quad_points(m: &[f64; 6], x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    vec![
        apply_matrix(m, x, y),
        apply_matrix(m, x + w, y),
        apply_matrix(m, x + w, y + h),
        apply_matrix(m, x, y + h),
    ]
}

fn ctx_clear_rect(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(w), Some(h)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    if w == 0.0 || h == 0.0 {
        return Ok(JsValue::undefined());
    }
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let Some(surf) = data.canvas.surfaces.get_mut(&nid) else {
        return Ok(JsValue::undefined());
    };
    let bw = surf.buf_w as usize;
    let bh = surf.buf_h as usize;
    if bw == 0 || bh == 0 {
        return Ok(JsValue::undefined());
    }
    let quad = rect_quad_points(&surf.state.transform, x, y, w, h);
    let mask = rasterize_polygons_mask(
        surf.buf_w,
        surf.buf_h,
        surf.scale_x,
        surf.scale_y,
        &[quad],
        false,
    );
    for py in 0..bh {
        for px in 0..bw {
            let cov = mask[py * bw + px];
            if cov == 0 {
                continue;
            }
            let clip_cov = surf
                .state
                .clip_mask
                .as_ref()
                .map_or(255, |cm| cm[py * bw + px]);
            if clip_cov == 0 {
                continue;
            }
            let idx = (py * bw + px) * 4;
            let keep = 1.0 - (f64::from(cov) / 255.0) * (f64::from(clip_cov) / 255.0);
            if keep <= 1e-4 {
                surf.pixels[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
            } else {
                surf.pixels[idx + 3] = (f64::from(surf.pixels[idx + 3]) * keep).round() as u8;
            }
        }
    }
    surf.dirty = true;
    Ok(JsValue::undefined())
}

fn ctx_fill_rect(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(w), Some(h)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    if w == 0.0 || h == 0.0 {
        return Ok(JsValue::undefined());
    }
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        let quad = rect_quad_points(&surf.state.transform, x, y, w, h);
        let mask = rasterize_polygons_mask(
            surf.buf_w,
            surf.buf_h,
            surf.scale_x,
            surf.scale_y,
            &[quad],
            false,
        );
        let paint = surf.state.fill_style.clone();
        apply_mask_paint(surf, gradients, &mask, &paint);
    }
    Ok(JsValue::undefined())
}

fn ctx_stroke_rect(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(w), Some(h)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    if w == 0.0 && h == 0.0 {
        return Ok(JsValue::undefined());
    }
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        let temp_sp = Subpath {
            points: rect_quad_points(&surf.state.transform, x, y, w, h),
            closed: true,
        };
        let polys = build_stroke_polygons(&[temp_sp], &surf.state);
        if !polys.is_empty() {
            let mask = rasterize_polygons_mask(
                surf.buf_w,
                surf.buf_h,
                surf.scale_x,
                surf.scale_y,
                &polys,
                false,
            );
            let paint = surf.state.stroke_style.clone();
            apply_mask_paint(surf, gradients, &mask, &paint);
        }
    }
    Ok(JsValue::undefined())
}

// --- Paths & Line Dash ---

fn ctx_begin_path(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
        surf.subpaths.clear();
    }
    Ok(JsValue::undefined())
}

fn ctx_close_path(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid)
        && let Some(last) = surf.subpaths.last_mut()
        && !last.points.is_empty()
    {
        let start = last.points[0];
        last.closed = true;
        if surf.subpaths.len() < MAX_SUBPATHS {
            surf.subpaths.push(Subpath {
                points: vec![start],
                closed: false,
            });
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_move_to(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(x), Some(y)) = (finite_arg(args, 0, ctx)?, finite_arg(args, 1, ctx)?)
        && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
        && surf.subpaths.len() < MAX_SUBPATHS
    {
        let pt = apply_matrix(&surf.state.transform, x, y);
        surf.subpaths.push(Subpath {
            points: vec![pt],
            closed: false,
        });
    }
    Ok(JsValue::undefined())
}

fn ctx_line_to(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(x), Some(y)) = (finite_arg(args, 0, ctx)?, finite_arg(args, 1, ctx)?)
        && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        let pt = apply_matrix(&surf.state.transform, x, y);
        push_point_to_subpath(surf, pt.0, pt.1);
    }
    Ok(JsValue::undefined())
}

fn ctx_rect(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(x), Some(y), Some(w), Some(h)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
        && surf.subpaths.len() < MAX_SUBPATHS
    {
        let pts = rect_quad_points(&surf.state.transform, x, y, w, h);
        let p0 = pts[0];
        surf.subpaths.push(Subpath {
            points: pts,
            closed: true,
        });
        if surf.subpaths.len() < MAX_SUBPATHS {
            surf.subpaths.push(Subpath {
                points: vec![p0],
                closed: false,
            });
        }
    }
    Ok(JsValue::undefined())
}

fn parse_round_rect_radii(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<[f64; 4]> {
    let Some(val) = arg else {
        return Ok([0.0; 4]);
    };
    if val.is_undefined() {
        return Ok([0.0; 4]);
    }
    let read_radius = |v: &JsValue, c: &mut Context| -> JsResult<f64> {
        let r = if let Some(o) = v.as_object() {
            let rx = o.get(js_string!("x"), c)?.to_number(c)?;
            let ry = o.get(js_string!("y"), c)?.to_number(c)?;
            if !rx.is_finite() || !ry.is_finite() {
                return Ok(f64::NAN);
            }
            if rx < 0.0 || ry < 0.0 {
                return Err(JsNativeError::range()
                    .with_message("roundRect radius cannot be negative")
                    .into());
            }
            (rx + ry) * 0.5
        } else {
            let n = v.to_number(c)?;
            if !n.is_finite() {
                return Ok(f64::NAN);
            }
            if n < 0.0 {
                return Err(JsNativeError::range()
                    .with_message("roundRect radius cannot be negative")
                    .into());
            }
            n
        };
        Ok(r)
    };
    if let Some(arr_obj) = val.as_object()
        && arr_obj.is_array()
    {
        let len = arr_obj.get(js_string!("length"), ctx)?.to_length(ctx)? as usize;
        if len == 0 || len > 4 {
            return Err(JsNativeError::range()
                .with_message("roundRect radii list must have 1..=4 elements")
                .into());
        }
        let mut vals = [0.0f64; 4];
        for (i, slot) in vals.iter_mut().enumerate().take(len) {
            let item = arr_obj.get(i as u32, ctx)?;
            *slot = read_radius(&item, ctx)?;
        }
        return Ok(match len {
            1 => [vals[0], vals[0], vals[0], vals[0]],
            2 => [vals[0], vals[1], vals[0], vals[1]],
            3 => [vals[0], vals[1], vals[2], vals[1]],
            _ => vals,
        });
    }
    let r = read_radius(val, ctx)?;
    Ok([r, r, r, r])
}

fn ctx_round_rect(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(w), Some(h)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    let mut radii = parse_round_rect_radii(args.get(4), ctx)?;
    if radii.iter().any(|r| !r.is_finite()) {
        return Ok(JsValue::undefined());
    }
    let x0 = if w >= 0.0 { x } else { x + w };
    let y0 = if h >= 0.0 { y } else { y + h };
    let aw = w.abs();
    let ah = h.abs();
    if aw > 0.0 && ah > 0.0 {
        let top = radii[0] + radii[1];
        let right = radii[1] + radii[2];
        let bottom = radii[2] + radii[3];
        let left = radii[3] + radii[0];
        let mut scale = 1.0f64;
        if top > aw {
            scale = scale.min(aw / top);
        }
        if bottom > aw {
            scale = scale.min(aw / bottom);
        }
        if left > ah {
            scale = scale.min(ah / left);
        }
        if right > ah {
            scale = scale.min(ah / right);
        }
        for r in &mut radii {
            *r *= scale;
        }
    } else {
        radii = [0.0; 4];
    }

    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid)
        && surf.subpaths.len() < MAX_SUBPATHS
    {
        let m = surf.state.transform;
        let [rtl, rtr, rbr, rbl] = radii;
        let mut pts = Vec::with_capacity(40);
        let push_corner =
            |pts: &mut Vec<(f64, f64)>, cx: f64, cy: f64, r: f64, a0: f64, a1: f64| {
                if r <= 1e-6 {
                    pts.push(apply_matrix(&m, cx, cy));
                } else {
                    let steps = 8usize;
                    for i in 0..=steps {
                        let a = a0 + (a1 - a0) * (i as f64 / steps as f64);
                        pts.push(apply_matrix(&m, cx + r * a.cos(), cy + r * a.sin()));
                    }
                }
            };
        let pi = std::f64::consts::PI;
        let half_pi = std::f64::consts::FRAC_PI_2;
        push_corner(&mut pts, x0 + rtl, y0 + rtl, rtl, pi, 1.5 * pi);
        push_corner(&mut pts, x0 + aw - rtr, y0 + rtr, rtr, -half_pi, 0.0);
        push_corner(&mut pts, x0 + aw - rbr, y0 + ah - rbr, rbr, 0.0, half_pi);
        push_corner(&mut pts, x0 + rbl, y0 + ah - rbl, rbl, half_pi, pi);
        let p_first = pts[0];
        surf.subpaths.push(Subpath {
            points: pts,
            closed: true,
        });
        if surf.subpaths.len() < MAX_SUBPATHS {
            surf.subpaths.push(Subpath {
                points: vec![p_first],
                closed: false,
            });
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_arc(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(radius), Some(start_angle), Some(end_angle)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    let ccw = args.get(5).is_some_and(JsValue::to_boolean);
    if radius < 0.0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "arc radius cannot be negative",
            ctx,
        ));
    }
    if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
        append_ellipse_arc(surf, x, y, radius, radius, 0.0, start_angle, end_angle, ccw);
    }
    Ok(JsValue::undefined())
}

fn ctx_ellipse(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y), Some(rx), Some(ry), Some(rot), Some(sa), Some(ea)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
        finite_arg(args, 5, ctx)?,
        finite_arg(args, 6, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    let ccw = args.get(7).is_some_and(JsValue::to_boolean);
    if rx < 0.0 || ry < 0.0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "ellipse radii cannot be negative",
            ctx,
        ));
    }
    if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
        append_ellipse_arc(surf, x, y, rx, ry, rot, sa, ea, ccw);
    }
    Ok(JsValue::undefined())
}

fn ctx_arc_to(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x1), Some(y1), Some(x2), Some(y2), Some(radius)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
    ) else {
        return Ok(JsValue::undefined());
    };
    if radius < 0.0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "arcTo radius cannot be negative",
            ctx,
        ));
    }
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let Some(surf) = data.canvas.surfaces.get_mut(&nid) else {
        return Ok(JsValue::undefined());
    };
    let m = surf.state.transform;
    let p1_dev = apply_matrix(&m, x1, y1);
    ensure_subpath_at(surf, p1_dev.0, p1_dev.1);
    let Some(p0_dev) = subpath_current_point(surf) else {
        return Ok(JsValue::undefined());
    };
    let Some(inv) = invert_matrix(&m) else {
        push_point_to_subpath(surf, p1_dev.0, p1_dev.1);
        return Ok(JsValue::undefined());
    };
    let (x0, y0) = apply_matrix(&inv, p0_dev.0, p0_dev.1);
    let d0 = (x0 - x1).hypot(y0 - y1);
    let d2 = (x2 - x1).hypot(y2 - y1);
    if radius <= 1e-7 || d0 <= 1e-7 || d2 <= 1e-7 {
        push_point_to_subpath(surf, p1_dev.0, p1_dev.1);
        return Ok(JsValue::undefined());
    }
    let u0 = ((x0 - x1) / d0, (y0 - y1) / d0);
    let u2 = ((x2 - x1) / d2, (y2 - y1) / d2);
    let cross = u0.0 * u2.1 - u0.1 * u2.0;
    if cross.abs() <= 1e-7 {
        push_point_to_subpath(surf, p1_dev.0, p1_dev.1);
        return Ok(JsValue::undefined());
    }
    let cos_ang = (u0.0 * u2.0 + u0.1 * u2.1).clamp(-1.0, 1.0);
    let half_ang = cos_ang.acos() * 0.5;
    let dist = radius / half_ang.tan();
    let t0 = (x1 + u0.0 * dist, y1 + u0.1 * dist);
    let t2 = (x1 + u2.0 * dist, y1 + u2.1 * dist);
    let bis = (u0.0 + u2.0, u0.1 + u2.1);
    let bis_len = bis.0.hypot(bis.1).max(1e-9);
    let center_dist = radius / half_ang.sin();
    let cx = x1 + (bis.0 / bis_len) * center_dist;
    let cy = y1 + (bis.1 / bis_len) * center_dist;
    let a0 = (t0.1 - cy).atan2(t0.0 - cx);
    let a2 = (t2.1 - cy).atan2(t2.0 - cx);
    let ccw = cross > 0.0;
    append_ellipse_arc(surf, cx, cy, radius, radius, 0.0, a0, a2, ccw);
    Ok(JsValue::undefined())
}

fn ctx_quadratic_curve_to(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(cpx), Some(cpy), Some(x), Some(y)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
    ) && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        let m = surf.state.transform;
        let cp = apply_matrix(&m, cpx, cpy);
        let p2 = apply_matrix(&m, x, y);
        ensure_subpath_at(surf, cp.0, cp.1);
        if let Some(p0) = subpath_current_point(surf) {
            append_quadratic(surf, p0, cp, p2);
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_bezier_curve_to(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if let (Some(cp1x), Some(cp1y), Some(cp2x), Some(cp2y), Some(x), Some(y)) = (
        finite_arg(args, 0, ctx)?,
        finite_arg(args, 1, ctx)?,
        finite_arg(args, 2, ctx)?,
        finite_arg(args, 3, ctx)?,
        finite_arg(args, 4, ctx)?,
        finite_arg(args, 5, ctx)?,
    ) && let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid)
    {
        let m = surf.state.transform;
        let c1 = apply_matrix(&m, cp1x, cp1y);
        let c2 = apply_matrix(&m, cp2x, cp2y);
        let p3 = apply_matrix(&m, x, y);
        ensure_subpath_at(surf, c1.0, c1.1);
        if let Some(p0) = subpath_current_point(surf) {
            append_cubic(surf, p0, c1, c2, p3);
        }
    }
    Ok(JsValue::undefined())
}

fn parse_fill_rule_arg(arg: Option<&JsValue>, ctx: &mut Context) -> JsResult<bool> {
    let Some(val) = arg else {
        return Ok(false);
    };
    if val.is_undefined() {
        return Ok(false);
    }
    let s = utf8(val, ctx)?;
    match s.as_str() {
        "nonzero" => Ok(false),
        "evenodd" => Ok(true),
        _ => Err(JsNativeError::typ()
            .with_message("Invalid CanvasFillRule; expected 'nonzero' or 'evenodd'")
            .into()),
    }
}

fn subpaths_as_polygons(subpaths: &[Subpath]) -> Vec<Vec<(f64, f64)>> {
    subpaths
        .iter()
        .filter(|sp| sp.points.len() >= 3)
        .map(|sp| sp.points.clone())
        .collect()
}

fn ctx_fill(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let even_odd = parse_fill_rule_arg(args.first(), ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        let polys = subpaths_as_polygons(&surf.subpaths);
        if !polys.is_empty()
            || matches!(
                surf.state.composite_op,
                CompositeOp::Copy | CompositeOp::Clear
            )
        {
            let mask = rasterize_polygons_mask(
                surf.buf_w,
                surf.buf_h,
                surf.scale_x,
                surf.scale_y,
                &polys,
                even_odd,
            );
            let paint = surf.state.fill_style.clone();
            apply_mask_paint(surf, gradients, &mask, &paint);
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_stroke(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        let polys = build_stroke_polygons(&surf.subpaths, &surf.state);
        if !polys.is_empty() {
            let mask = rasterize_polygons_mask(
                surf.buf_w,
                surf.buf_h,
                surf.scale_x,
                surf.scale_y,
                &polys,
                false,
            );
            let paint = surf.state.stroke_style.clone();
            apply_mask_paint(surf, gradients, &mask, &paint);
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_clip(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let even_odd = parse_fill_rule_arg(args.first(), ctx)?;
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid) {
        let polys = subpaths_as_polygons(&surf.subpaths);
        let new_mask = rasterize_polygons_mask(
            surf.buf_w,
            surf.buf_h,
            surf.scale_x,
            surf.scale_y,
            &polys,
            even_odd,
        );
        if let Some(existing) = surf.state.clip_mask.as_mut() {
            for (dst, &src) in existing.iter_mut().zip(new_mask.iter()) {
                *dst = ((u32::from(*dst) * u32::from(src) + 127) / 255) as u8;
            }
        } else {
            surf.state.clip_mask = Some(new_mask);
        }
    }
    Ok(JsValue::undefined())
}

fn ctx_is_point_in_path(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let (Some(x), Some(y)) = (finite_arg(args, 0, ctx)?, finite_arg(args, 1, ctx)?) else {
        return Ok(false.into());
    };
    let even_odd = parse_fill_rule_arg(args.get(2), ctx)?;
    let shared = state(ctx);
    let data = shared.borrow();
    let Some(surf) = data.canvas.surfaces.get(&nid) else {
        return Ok(false.into());
    };
    // Per HTML5 Canvas spec, isPointInPath(x, y) tests coordinates in unaffected canvas pixel space!
    Ok(point_in_subpaths(x, y, &surf.subpaths, even_odd).into())
}

fn ctx_set_line_dash(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let Some(seq_obj) = args.first().and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("setLineDash requires a sequence argument")
            .into());
    };
    let len = seq_obj
        .get(js_string!("length"), ctx)?
        .to_length(ctx)?
        .min(256) as usize;
    let mut list = Vec::with_capacity(len);
    for i in 0..len {
        let v = seq_obj.get(i as u32, ctx)?.to_number(ctx)?;
        if !v.is_finite() || v < 0.0 {
            return Ok(JsValue::undefined());
        }
        list.push(v);
    }
    if list.len() % 2 == 1 {
        let copy = list.clone();
        list.extend(copy);
    }
    if let Some(surf) = state(ctx).borrow_mut().canvas.surfaces.get_mut(&nid) {
        surf.state.line_dash = list;
    }
    Ok(JsValue::undefined())
}

fn ctx_get_line_dash(this: &JsValue, _args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let dash = state(ctx).borrow().canvas.surfaces[&nid]
        .state
        .line_dash
        .clone();
    let arr = JsArray::from_iter(dash.into_iter().map(JsValue::from), ctx);
    Ok(arr.into())
}

// --- Text, Gradients, ImageData, & drawImage ---

fn ctx_fill_text(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let text = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    modern::check_context(ctx)?;
    let (Some(x), Some(y)) = (finite_arg(args, 1, ctx)?, finite_arg(args, 2, ctx)?) else {
        return Ok(JsValue::undefined());
    };
    let max_width = if let Some(a3) = args.get(3)
        && !a3.is_undefined()
    {
        Some(a3.to_number(ctx)?)
    } else {
        None
    };
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        draw_text_bitmap(surf, gradients, &text, x, y, max_width, false);
    }
    Ok(JsValue::undefined())
}

fn ctx_stroke_text(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let text = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    modern::check_context(ctx)?;
    let (Some(x), Some(y)) = (finite_arg(args, 1, ctx)?, finite_arg(args, 2, ctx)?) else {
        return Ok(JsValue::undefined());
    };
    let max_width = if let Some(a3) = args.get(3)
        && !a3.is_undefined()
    {
        Some(a3.to_number(ctx)?)
    } else {
        None
    };
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let CanvasStore {
        surfaces,
        gradients,
        ..
    } = &mut data.canvas;
    if let Some(surf) = surfaces.get_mut(&nid) {
        draw_text_bitmap(surf, gradients, &text, x, y, max_width, true);
    }
    Ok(JsValue::undefined())
}

fn ctx_measure_text(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let text = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
    modern::check_context(ctx)?;
    let (font_px, bold) = {
        let shared = state(ctx);
        let data = shared.borrow();
        let surf = &data.canvas.surfaces[&nid];
        (surf.state.font_px, surf.state.font_bold)
    };
    let width = measure_text_width(&text, font_px, bold);
    let ascent = font_px * 0.78;
    let descent = font_px * 0.22;
    let attr = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let metrics = ObjectInitializer::new(ctx)
        .property(js_string!("width"), width, attr)
        .property(js_string!("actualBoundingBoxLeft"), 0.0, attr)
        .property(js_string!("actualBoundingBoxRight"), width, attr)
        .property(js_string!("fontBoundingBoxAscent"), ascent, attr)
        .property(js_string!("fontBoundingBoxDescent"), descent, attr)
        .property(js_string!("actualBoundingBoxAscent"), ascent, attr)
        .property(js_string!("actualBoundingBoxDescent"), descent, attr)
        .build();
    Ok(metrics.into())
}

fn ctx_create_linear_gradient(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let _nid = ctx_node_id(this, ctx)?;
    let x0 = require_finite_arg(args, 0, ctx)?;
    let y0 = require_finite_arg(args, 1, ctx)?;
    let x1 = require_finite_arg(args, 2, ctx)?;
    let y1 = require_finite_arg(args, 3, ctx)?;
    let gid = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.canvas.gradients.len() >= MAX_GRADIENTS {
            return Err(failure("CanvasGradient limit reached"));
        }
        let id = data.canvas.gradients.len();
        data.canvas.gradients.push(GradientDef {
            kind: GradientKind::Linear { x0, y0, x1, y1 },
            stops: Vec::new(),
        });
        id
    };
    build_gradient_object(gid, ctx).map(JsValue::from)
}

fn ctx_create_radial_gradient(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let _nid = ctx_node_id(this, ctx)?;
    let x0 = require_finite_arg(args, 0, ctx)?;
    let y0 = require_finite_arg(args, 1, ctx)?;
    let r0 = require_finite_arg(args, 2, ctx)?;
    let x1 = require_finite_arg(args, 3, ctx)?;
    let y1 = require_finite_arg(args, 4, ctx)?;
    let r1 = require_finite_arg(args, 5, ctx)?;
    if r0 < 0.0 || r1 < 0.0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "Radial gradient radius cannot be negative",
            ctx,
        ));
    }
    let gid = {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        if data.canvas.gradients.len() >= MAX_GRADIENTS {
            return Err(failure("CanvasGradient limit reached"));
        }
        let id = data.canvas.gradients.len();
        data.canvas.gradients.push(GradientDef {
            kind: GradientKind::Radial {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
            },
            stops: Vec::new(),
        });
        id
    };
    build_gradient_object(gid, ctx).map(JsValue::from)
}

fn ctx_create_image_data(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let _nid = ctx_node_id(this, ctx)?;
    if args.len() == 1
        && let Some(obj) = args[0].as_object()
    {
        let w = obj.get(js_string!("width"), ctx)?.to_u32(ctx)?;
        let h = obj.get(js_string!("height"), ctx)?.to_u32(ctx)?;
        return construct_image_data(w, h, None, ctx);
    }
    if args.len() < 2 {
        return Err(JsNativeError::typ()
            .with_message("createImageData requires 1 ImageData or (sw, sh)")
            .into());
    }
    let sw = require_finite_arg(args, 0, ctx)?.abs().trunc() as u32;
    let sh = require_finite_arg(args, 1, ctx)?.abs().trunc() as u32;
    construct_image_data(sw, sh, None, ctx)
}

fn ctx_get_image_data(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let sx_f = require_finite_arg(args, 0, ctx)?;
    let sy_f = require_finite_arg(args, 1, ctx)?;
    let sw_f = require_finite_arg(args, 2, ctx)?;
    let sh_f = require_finite_arg(args, 3, ctx)?;
    let sw_abs = sw_f.abs().trunc() as u32;
    let sh_abs = sh_f.abs().trunc() as u32;
    if sw_abs == 0 || sh_abs == 0 {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "getImageData width and height must be non-zero",
            ctx,
        ));
    }
    if sw_abs > MAX_CANVAS_SIDE * 2
        || sh_abs > MAX_CANVAS_SIDE * 2
        || (sw_abs as usize).saturating_mul(sh_abs as usize) > MAX_CANVAS_PIXELS
    {
        return Err(dom_exception::throw(
            "IndexSizeError",
            "getImageData exceeds maximum pixel budget",
            ctx,
        ));
    }
    let rx0 = (if sw_f < 0.0 { sx_f + sw_f } else { sx_f }).trunc() as i64;
    let ry0 = (if sh_f < 0.0 { sy_f + sh_f } else { sy_f }).trunc() as i64;
    let rgba = {
        let shared = state(ctx);
        let data = shared.borrow();
        let surf = &data.canvas.surfaces[&nid];
        let mut out = vec![0u8; (sw_abs as usize) * (sh_abs as usize) * 4];
        if surf.buf_w > 0 && surf.buf_h > 0 {
            let bw = surf.buf_w as i64;
            let bh = surf.buf_h as i64;
            for row in 0..sh_abs as i64 {
                let ly = ry0 + row;
                let by = ((ly as f64 + 0.5) * surf.scale_y).floor() as i64;
                if by < 0 || by >= bh {
                    continue;
                }
                for col in 0..sw_abs as i64 {
                    let lx = rx0 + col;
                    let bx = ((lx as f64 + 0.5) * surf.scale_x).floor() as i64;
                    if bx < 0 || bx >= bw {
                        continue;
                    }
                    let src_idx = ((by as usize) * (surf.buf_w as usize) + (bx as usize)) * 4;
                    let dst_idx = ((row as usize) * (sw_abs as usize) + (col as usize)) * 4;
                    out[dst_idx..dst_idx + 4].copy_from_slice(&surf.pixels[src_idx..src_idx + 4]);
                }
            }
        }
        out
    };
    construct_image_data(sw_abs, sh_abs, Some(&rgba), ctx)
}

fn extract_image_data_bytes(obj: &JsObject, ctx: &mut Context) -> JsResult<(u32, u32, Vec<u8>)> {
    let w = obj.get(js_string!("width"), ctx)?.to_u32(ctx)?;
    let h = obj.get(js_string!("height"), ctx)?.to_u32(ctx)?;
    let data_obj = obj
        .get(js_string!("data"), ctx)?
        .as_object()
        .ok_or_else(|| failure("putImageData requires an ImageData object"))?;
    let ta = JsTypedArray::from_object(data_obj)
        .map_err(|_| failure("putImageData requires an ImageData object with TypedArray data"))?;
    let buf_obj = ta
        .buffer(ctx)?
        .as_object()
        .ok_or_else(|| failure("Invalid ImageData buffer"))?;
    let ab = JsArrayBuffer::from_object(buf_obj)?;
    let boff = ta.byte_offset(ctx)?;
    let blen = ta.byte_length(ctx)?;
    let expected = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if w == 0 || h == 0 || blen < expected {
        return Err(dom_exception::throw(
            "InvalidStateError",
            "Malformed ImageData dimensions or detached buffer",
            ctx,
        ));
    }
    let bytes = {
        let guard = ab.data();
        let slice = guard.as_deref().ok_or_else(|| {
            dom_exception::throw("InvalidStateError", "Detached ImageData buffer", ctx)
        })?;
        slice[boff..boff + expected].to_vec()
    };
    Ok((w, h, bytes))
}

fn ctx_put_image_data(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    let Some(img_obj) = args.first().and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("putImageData requires an ImageData first argument")
            .into());
    };
    let (iw, ih, src_bytes) = extract_image_data_bytes(&img_obj, ctx)?;
    let dx = require_finite_arg(args, 1, ctx)?.trunc() as i64;
    let dy = require_finite_arg(args, 2, ctx)?.trunc() as i64;
    let (mut dirty_x, mut dirty_y, mut dirty_w, mut dirty_h) = if args.len() >= 7 {
        (
            require_finite_arg(args, 3, ctx)?.trunc() as i64,
            require_finite_arg(args, 4, ctx)?.trunc() as i64,
            require_finite_arg(args, 5, ctx)?.trunc() as i64,
            require_finite_arg(args, 6, ctx)?.trunc() as i64,
        )
    } else {
        (0, 0, iw as i64, ih as i64)
    };
    if dirty_w < 0 {
        dirty_x += dirty_w;
        dirty_w = -dirty_w;
    }
    if dirty_h < 0 {
        dirty_y += dirty_h;
        dirty_h = -dirty_h;
    }
    if dirty_x < 0 {
        dirty_w += dirty_x;
        dirty_x = 0;
    }
    if dirty_y < 0 {
        dirty_h += dirty_y;
        dirty_y = 0;
    }
    if dirty_x + dirty_w > iw as i64 {
        dirty_w = iw as i64 - dirty_x;
    }
    if dirty_y + dirty_h > ih as i64 {
        dirty_h = ih as i64 - dirty_y;
    }
    if dirty_w <= 0 || dirty_h <= 0 {
        return Ok(JsValue::undefined());
    }
    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    if let Some(surf) = data.canvas.surfaces.get_mut(&nid)
        && surf.buf_w > 0
        && surf.buf_h > 0
    {
        let bw = surf.buf_w as i64;
        let bh = surf.buf_h as i64;
        for row in dirty_y..(dirty_y + dirty_h) {
            let ly = dy + row;
            let by0 = ((ly as f64) * surf.scale_y).floor() as i64;
            let by1 = (((ly + 1) as f64) * surf.scale_y)
                .floor()
                .max((by0 + 1) as f64) as i64;
            for col in dirty_x..(dirty_x + dirty_w) {
                let lx = dx + col;
                let bx0 = ((lx as f64) * surf.scale_x).floor() as i64;
                let bx1 = (((lx + 1) as f64) * surf.scale_x)
                    .floor()
                    .max((bx0 + 1) as f64) as i64;
                let sidx = ((row as usize) * (iw as usize) + (col as usize)) * 4;
                let pix = &src_bytes[sidx..sidx + 4];
                for by in by0.max(0)..by1.min(bh) {
                    for bx in bx0.max(0)..bx1.min(bw) {
                        let didx = ((by as usize) * (surf.buf_w as usize) + (bx as usize)) * 4;
                        surf.pixels[didx..didx + 4].copy_from_slice(pix);
                    }
                }
            }
        }
        surf.dirty = true;
    }
    Ok(JsValue::undefined())
}

type DecodedBitmap = (u32, u32, f64, f64, Vec<u8>);

fn decode_source_bitmap(src_val: &JsValue, ctx: &mut Context) -> JsResult<Option<DecodedBitmap>> {
    let Some(obj) = src_val.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("drawImage requires a CanvasImageSource first argument")
            .into());
    };
    let handle = {
        let brands = state(ctx).borrow().brands.clone();
        brands
            .get(&obj, ctx)?
            .as_string()
            .map(|s| s.to_std_string_escaped())
    };
    if let Some(h) = handle {
        let shared = state(ctx);
        let mut data = shared.borrow_mut();
        let src_id = data.host.node(&h).map_err(failure)?;
        let tag = data.host.document.nodes[src_id].tag.clone();
        if tag == "canvas" {
            ensure_surface(&mut data, src_id).map_err(failure)?;
            let surf = &data.canvas.surfaces[&src_id];
            if surf.logical_w == 0 || surf.logical_h == 0 || surf.buf_w == 0 || surf.buf_h == 0 {
                return Ok(None);
            }
            return Ok(Some((
                surf.logical_w,
                surf.logical_h,
                surf.scale_x,
                surf.scale_y,
                surf.pixels.clone(),
            )));
        }
        if tag == "img" {
            let Some(raw_src) = data.host.document.nodes[src_id]
                .attr("src")
                .map(str::to_owned)
            else {
                return Ok(None);
            };
            if let Ok((ct, bytes)) = crate::images::decode_data_url(&raw_src)
                && let Ok(img) = crate::images::decode(&bytes, &ct, None, None)
                && img.width > 0
                && img.height > 0
            {
                return Ok(Some((img.width, img.height, 1.0, 1.0, img.pixels)));
            }
            let resolved = url::Url::parse(&data.host.document.base_url)
                .and_then(|u| u.join(&raw_src))
                .ok()
                .map(|u| u.as_str().to_owned())
                .unwrap_or(raw_src);
            if let Some(res) = data.host.document.resources.get(&resolved)
                && let Ok(img) = crate::images::decode(&res.bytes, &res.content_type, None, None)
                && img.width > 0
                && img.height > 0
            {
                return Ok(Some((img.width, img.height, 1.0, 1.0, img.pixels)));
            }
            return Ok(None);
        }
    }
    if let Ok((iw, ih, bytes)) = extract_image_data_bytes(&obj, ctx) {
        return Ok(Some((iw, ih, 1.0, 1.0, bytes)));
    }
    Err(JsNativeError::typ()
        .with_message("Unsupported CanvasImageSource in drawImage")
        .into())
}

fn ctx_draw_image(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    modern::check_context(ctx)?;
    let nid = ctx_node_id(this, ctx)?;
    if args.len() != 3 && args.len() != 5 && args.len() != 9 {
        return Err(JsNativeError::typ()
            .with_message("drawImage requires 3, 5, or 9 arguments")
            .into());
    }
    let src_opt = decode_source_bitmap(&args[0], ctx)?;
    let Some((src_lw, src_lh, src_sx, src_sy, src_pixels)) = src_opt else {
        return Ok(JsValue::undefined());
    };
    let (mut sx, mut sy, mut sw, mut sh, mut dx, mut dy, mut dw, mut dh) = match args.len() {
        3 => {
            let (Some(dx), Some(dy)) = (finite_arg(args, 1, ctx)?, finite_arg(args, 2, ctx)?)
            else {
                return Ok(JsValue::undefined());
            };
            (
                0.0,
                0.0,
                src_lw as f64,
                src_lh as f64,
                dx,
                dy,
                src_lw as f64,
                src_lh as f64,
            )
        }
        5 => {
            let (Some(dx), Some(dy), Some(dw), Some(dh)) = (
                finite_arg(args, 1, ctx)?,
                finite_arg(args, 2, ctx)?,
                finite_arg(args, 3, ctx)?,
                finite_arg(args, 4, ctx)?,
            ) else {
                return Ok(JsValue::undefined());
            };
            (0.0, 0.0, src_lw as f64, src_lh as f64, dx, dy, dw, dh)
        }
        _ => {
            let (Some(sx), Some(sy), Some(sw), Some(sh), Some(dx), Some(dy), Some(dw), Some(dh)) = (
                finite_arg(args, 1, ctx)?,
                finite_arg(args, 2, ctx)?,
                finite_arg(args, 3, ctx)?,
                finite_arg(args, 4, ctx)?,
                finite_arg(args, 5, ctx)?,
                finite_arg(args, 6, ctx)?,
                finite_arg(args, 7, ctx)?,
                finite_arg(args, 8, ctx)?,
            ) else {
                return Ok(JsValue::undefined());
            };
            (sx, sy, sw, sh, dx, dy, dw, dh)
        }
    };
    if sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
        return Ok(JsValue::undefined());
    }
    if sw < 0.0 {
        sx += sw;
        sw = -sw;
    }
    if sh < 0.0 {
        sy += sh;
        sh = -sh;
    }
    if dw < 0.0 {
        dx += dw;
        dw = -dw;
    }
    if dh < 0.0 {
        dy += dh;
        dh = -dh;
    }
    let src_bw = ((src_lw as f64) * src_sx).round().max(1.0) as usize;
    let src_bh = ((src_lh as f64) * src_sy).round().max(1.0) as usize;

    let shared = state(ctx);
    let mut data = shared.borrow_mut();
    let Some(surf) = data.canvas.surfaces.get_mut(&nid) else {
        return Ok(JsValue::undefined());
    };
    let bw = surf.buf_w as usize;
    let bh = surf.buf_h as usize;
    if bw == 0 || bh == 0 || surf.state.global_alpha <= 0.0 {
        return Ok(JsValue::undefined());
    }
    let Some(inv_ctm) = invert_matrix(&surf.state.transform) else {
        return Ok(JsValue::undefined());
    };
    let quad = rect_quad_points(&surf.state.transform, dx, dy, dw, dh);
    let mask = rasterize_polygons_mask(
        surf.buf_w,
        surf.buf_h,
        surf.scale_x,
        surf.scale_y,
        &[quad],
        false,
    );
    let ga = surf.state.global_alpha;

    for py in 0..bh {
        for px in 0..bw {
            let cov = mask[py * bw + px];
            if cov == 0 {
                continue;
            }
            let clip_cov = surf
                .state
                .clip_mask
                .as_ref()
                .map_or(255, |cm| cm[py * bw + px]);
            if clip_cov == 0 {
                continue;
            }
            let lx = (px as f64 + 0.5) / surf.scale_x;
            let ly = (py as f64 + 0.5) / surf.scale_y;
            let (ux, uy) = apply_matrix(&inv_ctm, lx, ly);
            let u = (ux - dx) / dw;
            let v = (uy - dy) / dh;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                continue;
            }
            let sample_lx = sx + u * sw;
            let sample_ly = sy + v * sh;
            if sample_lx < 0.0
                || sample_ly < 0.0
                || sample_lx >= src_lw as f64
                || sample_ly >= src_lh as f64
            {
                continue;
            }
            let sbx = ((sample_lx * src_sx).floor() as usize).min(src_bw - 1);
            let sby = ((sample_ly * src_sy).floor() as usize).min(src_bh - 1);
            let sidx = (sby * src_bw + sbx) * 4;
            let sp = [
                src_pixels[sidx],
                src_pixels[sidx + 1],
                src_pixels[sidx + 2],
                src_pixels[sidx + 3],
            ];
            let eff_a =
                (f64::from(sp[3]) * (f64::from(cov) / 255.0) * (f64::from(clip_cov) / 255.0) * ga)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            let didx = (py * bw + px) * 4;
            composite_pixel(
                &mut surf.pixels[didx..didx + 4],
                [sp[0], sp[1], sp[2], eff_a],
                surf.state.composite_op,
            );
        }
    }
    surf.dirty = true;
    Ok(JsValue::undefined())
}

fn build_context_2d_object(node_id: usize, context: &mut Context) -> JsResult<JsObject> {
    let proto = context
        .global_object()
        .get(js_string!("CanvasRenderingContext2D"), context)
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|c| c.get(js_string!("prototype"), context).ok())
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
    let obj = JsObject::from_proto_and_data(proto, ());
    def_prop(
        &obj,
        js_string!("__mg_canvas_id"),
        node_id as f64,
        false,
        false,
        false,
        context,
    )?;
    def_getter_fn(
        &obj,
        "canvas",
        true,
        |this, _, ctx| {
            modern::check_context(ctx)?;
            dom_ops::wrap_node_id(ctx_node_id(this, ctx)?, ctx)
        },
        context,
    )?;
    for prop in [
        "fillStyle",
        "strokeStyle",
        "lineWidth",
        "lineCap",
        "lineJoin",
        "miterLimit",
        "lineDashOffset",
        "globalAlpha",
        "globalCompositeOperation",
        "font",
        "textAlign",
        "textBaseline",
        "shadowColor",
        "shadowBlur",
        "shadowOffsetX",
        "shadowOffsetY",
    ] {
        def_get_set_cap(
            &obj,
            prop,
            true,
            prop,
            |this, _, &p, ctx| ctx_prop_get(this, p, ctx),
            |this, args, &p, ctx| ctx_prop_set(this, args, p, ctx),
            context,
        )?;
    }
    let methods: &[(
        &str,
        usize,
        fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>,
    )] = &[
        ("save", 0, ctx_save),
        ("restore", 0, ctx_restore),
        ("scale", 2, ctx_scale),
        ("rotate", 1, ctx_rotate),
        ("translate", 2, ctx_translate),
        ("transform", 6, ctx_transform),
        ("setTransform", 6, ctx_set_transform),
        ("resetTransform", 0, ctx_reset_transform),
        ("getTransform", 0, ctx_get_transform),
        ("clearRect", 4, ctx_clear_rect),
        ("fillRect", 4, ctx_fill_rect),
        ("strokeRect", 4, ctx_stroke_rect),
        ("beginPath", 0, ctx_begin_path),
        ("closePath", 0, ctx_close_path),
        ("moveTo", 2, ctx_move_to),
        ("lineTo", 2, ctx_line_to),
        ("rect", 4, ctx_rect),
        ("roundRect", 4, ctx_round_rect),
        ("arc", 5, ctx_arc),
        ("arcTo", 5, ctx_arc_to),
        ("ellipse", 7, ctx_ellipse),
        ("quadraticCurveTo", 4, ctx_quadratic_curve_to),
        ("bezierCurveTo", 6, ctx_bezier_curve_to),
        ("fill", 0, ctx_fill),
        ("stroke", 0, ctx_stroke),
        ("clip", 0, ctx_clip),
        ("isPointInPath", 2, ctx_is_point_in_path),
        ("setLineDash", 1, ctx_set_line_dash),
        ("getLineDash", 0, ctx_get_line_dash),
        ("fillText", 3, ctx_fill_text),
        ("strokeText", 3, ctx_stroke_text),
        ("measureText", 1, ctx_measure_text),
        ("createLinearGradient", 4, ctx_create_linear_gradient),
        ("createRadialGradient", 6, ctx_create_radial_gradient),
        ("createImageData", 2, ctx_create_image_data),
        ("getImageData", 4, ctx_get_image_data),
        ("putImageData", 3, ctx_put_image_data),
        ("drawImage", 3, ctx_draw_image),
    ];
    for &(mname, mlen, mfn) in methods {
        def_fn(&obj, mname, mlen, false, mfn, context)?;
    }
    Ok(obj)
}
