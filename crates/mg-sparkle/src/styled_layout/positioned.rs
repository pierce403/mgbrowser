//! Bounded out-of-flow boxes after normal layout.

use super::*;
use crate::style::{BoxSizing, IntrinsicSize, Position, Visibility};

const MAX_POSITIONED: usize = 512;

struct Entry {
    id: usize,
    depth: usize,
    containing: Option<usize>,
    dependency: Option<usize>,
    fixed_ancestor: bool,
    z: i32,
    done: bool,
}

pub(super) fn is_inset_zero_passthrough(layout: &Layout<'_, '_>, id: usize) -> bool {
    let node = &layout.document.nodes[id];
    let style = &layout.styles[id];
    node.tag == "div"
        && style.layout.position == Position::Absolute
        && style.layout.inset == [Length::Px(0.0); 4]
        && style.width == Length::Auto
        && style.height == Length::Auto
        && style.margin == [Length::Px(0.0); 4]
        && style.padding == [Length::Px(0.0); 4]
        && style.border_width == [0.0; 4]
        && style.background_color.alpha == 0.0
        && style.background_images.is_empty()
        && !node.children.is_empty()
        && node.children.iter().all(|&child| {
            layout.document.nodes[child].tag == "#text"
                || matches!(
                    layout.styles[child].layout.position,
                    Position::Absolute | Position::Fixed
                )
        })
}

fn is_collapsed_ancestor(layout: &Layout<'_, '_>, cursor: usize) -> bool {
    let style = &layout.styles[cursor];
    if style.visibility != Visibility::Collapse {
        return false;
    }
    if matches!(
        style.display,
        Display::TableRow
            | Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableColumn
            | Display::TableColumnGroup
    ) {
        return true;
    }
    if cursor != 0 && !matches!(style.layout.position, Position::Absolute | Position::Fixed) {
        let mut p = layout.document.nodes[cursor].parent;
        for _ in 0..MAX_DEPTH {
            if p == 0 || layout.styles[p].display != Display::Contents {
                break;
            }
            p = layout.document.nodes[p].parent;
        }
        if matches!(
            layout.styles[p].display,
            Display::Flex | Display::InlineFlex
        ) {
            return true;
        }
    }
    false
}

pub(super) fn run(layout: &mut Layout<'_, '_>) {
    if layout.exhausted {
        return;
    }
    let mut entries = Vec::new();
    let mut count = 0;
    for id in 0..layout.document.nodes.len() {
        if !layout.spend(0) {
            layout.fail(id, "positioned discovery work limit");
            return;
        }
        if !matches!(
            layout.styles[id].layout.position,
            Position::Absolute | Position::Fixed
        ) || layout.document.nodes[id].tag == "#text"
            || is_inset_zero_passthrough(layout, id)
        {
            continue;
        }
        let mut cursor = id;
        let mut depth = 0;
        let mut hidden = false;
        let mut containing = None;
        let mut dependency = None;
        let mut fixed_ancestor = false;
        loop {
            if !layout.spend(depth) {
                layout.fail(id, "positioned ancestor depth or work limit");
                return;
            }
            let style = &layout.styles[cursor];
            hidden |= style.display == Display::None
                || style.opacity <= 0.0
                || (cursor != id && is_collapsed_ancestor(layout, cursor));
            if cursor != id && !is_inset_zero_passthrough(layout, cursor) {
                fixed_ancestor |= style.layout.position == Position::Fixed;
                if containing.is_none() && style.layout.position != Position::Static {
                    containing = Some(cursor);
                }
                if dependency.is_none()
                    && matches!(style.layout.position, Position::Absolute | Position::Fixed)
                {
                    dependency = Some(cursor);
                }
            }
            if cursor == 0 {
                break;
            }
            cursor = layout.document.nodes[cursor].parent;
            depth += 1;
        }
        if hidden {
            continue;
        }
        count += 1;
        if count > MAX_POSITIONED {
            layout.fail(id, "positioned node limit (512)");
            return;
        }
        if layout.styles[id].layout.unsupported && !layout.styles[id].layout.outer_calc_only {
            layout.fail(id, "positioned snapshot has an unrepresented CSS value");
            return;
        }
        if layout.styles[id].display == Display::Contents {
            layout.fail(id, "positioned display:contents has no containing box");
            return;
        }
        entries.push(Entry {
            id,
            depth,
            containing,
            dependency,
            fixed_ancestor,
            z: layout.styles[id].layout.z_index.unwrap_or(0),
            done: false,
        });
    }
    let indices: HashMap<_, _> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id, index))
        .collect();
    for entry in &mut entries {
        if let Some(parent) = entry.dependency {
            let Some(&index) = indices.get(&parent) else {
                layout.fail(entry.id, "positioned ancestor absent from admitted nodes");
                return;
            };
            entry.dependency = Some(index);
        }
    }
    if entries.is_empty() {
        return;
    }
    let Some(budget) = layout.budget.checked_sub(layout.scene.borders.len()) else {
        layout.fail(entries[0].id, "positioned border lookup work limit");
        return;
    };
    layout.budget = budget;
    let mut border_boxes = vec![None; layout.document.nodes.len()];
    for &(node, r) in &layout.scene.borders {
        if let Some(slot) = border_boxes.get_mut(node) {
            *slot = Some(r);
        }
    }
    // Ancestors must provide final geometry before descendants. Among ready
    // nodes, numeric z-index and then DOM order decide initial emission order.
    for _ in 0..entries.len() {
        let mut selected = None;
        for (index, entry) in entries.iter().enumerate() {
            if !layout.spend(0) {
                layout.fail(entry.id, "positioned ordering work limit");
                return;
            }
            if entry.done || entry.dependency.is_some_and(|parent| !entries[parent].done) {
                continue;
            }
            if selected
                .is_none_or(|previous: usize| (entry.z, index) < (entries[previous].z, previous))
            {
                selected = Some(index);
            }
        }
        let Some(index) = selected else {
            layout.fail(0, "positioned ancestor ordering cannot progress");
            return;
        };
        if !paint(layout, &mut border_boxes, &entries[index]) {
            layout.fail(entries[index].id, "positioned box rejected");
            return;
        }
        entries[index].done = true;
    }
}

fn resolve_containing_box(
    layout: &mut Layout<'_, '_>,
    border_boxes: &[Option<Rect>],
    ancestor: usize,
) -> Option<Rect> {
    if !layout.spend(0) {
        layout.fail(ancestor, "positioned border lookup work limit");
        return None;
    }
    if let Some(r) = border_boxes.get(ancestor).copied().flatten() {
        let b = layout.styles[ancestor].border_width;
        return Some(Rect {
            x: r.x + b[3],
            y: r.y + b[0],
            w: (r.w - b[1] - b[3]).max(0.0),
            h: (r.h - b[0] - b[2]).max(0.0),
        });
    }
    // CSS 2.1 §10.1: if the positioned ancestor is an inline element (or other
    // box without a block border-box record), form the containing block from
    // its recorded fragment boxes, or walk up to the nearest enclosing border
    // box / initial containing block rather than aborting the whole page.
    let mut frag_union: Option<(f32, f32, f32, f32)> = None;
    for bx in &layout.scene.boxes {
        if bx.node == ancestor {
            let x0 = bx.x as f32;
            let y0 = bx.y as f32;
            let x1 = x0 + bx.width as f32;
            let y1 = y0 + bx.height as f32;
            frag_union = Some(match frag_union {
                None => (x0, y0, x1, y1),
                Some((lx, ty, rx, by)) => (lx.min(x0), ty.min(y0), rx.max(x1), by.max(y1)),
            });
        }
    }
    if let Some((lx, ty, rx, by)) = frag_union {
        let b = layout.styles[ancestor].border_width;
        return Some(Rect {
            x: lx + b[3],
            y: ty + b[0],
            w: (rx - lx - b[1] - b[3]).max(0.0),
            h: (by - ty - b[0] - b[2]).max(0.0),
        });
    }
    let mut cur = layout.document.nodes.get(ancestor).map_or(0, |n| n.parent);
    for _ in 0..=MAX_DEPTH {
        if cur == 0 {
            break;
        }
        if let Some(r) = border_boxes.get(cur).copied().flatten() {
            let b = layout.styles[cur].border_width;
            return Some(Rect {
                x: r.x + b[3],
                y: r.y + b[0],
                w: (r.w - b[1] - b[3]).max(0.0),
                h: (r.h - b[0] - b[2]).max(0.0),
            });
        }
        let next = layout.document.nodes.get(cur).map_or(0, |n| n.parent);
        if next == cur {
            break;
        }
        cur = next;
    }
    Some(Rect {
        x: 0.0,
        y: 0.0,
        w: layout.viewport.width as f32,
        h: layout.viewport.height as f32,
    })
}

fn resolve_axis_align(
    align: formatting::StaticAxisAlign,
    safety: Option<formatting::StaticAxisAlign>,
    start_edge: f32,
    extent: f32,
    size: f32,
    margin_start: f32,
    margin_end: f32,
) -> f32 {
    let outer = size + margin_start + margin_end;
    let effective = if outer > extent {
        safety.unwrap_or(align)
    } else {
        align
    };
    match effective {
        formatting::StaticAxisAlign::Start => start_edge + margin_start,
        formatting::StaticAxisAlign::Center => start_edge + (extent - outer) / 2.0 + margin_start,
        formatting::StaticAxisAlign::End => start_edge + extent - size - margin_end,
    }
}

fn paint(layout: &mut Layout<'_, '_>, border_boxes: &mut [Option<Rect>], entry: &Entry) -> bool {
    let id = entry.id;
    let style = layout.styles[id].clone();
    let fixed = style.layout.position == Position::Fixed;
    let containing = if !fixed && let Some(ancestor) = entry.containing {
        let Some(c) = resolve_containing_box(layout, border_boxes, ancestor) else {
            return false;
        };
        c
    } else {
        Rect {
            x: 0.0,
            y: if fixed {
                layout.viewport.scroll as f32
            } else {
                0.0
            },
            w: layout.viewport.width as f32,
            h: layout.viewport.height as f32,
        }
    };
    let (mut margin, padding) = layout.edges(id, containing.w);
    let horizontal = padding[1] + padding[3];
    let vertical = padding[0] + padding[2];
    let [top, right, bottom, left] = std::array::from_fn(|side| {
        style.layout.inset[side].resolve(if side % 2 == 0 {
            containing.h
        } else {
            containing.w
        })
    });
    if [top, right, bottom, left]
        .into_iter()
        .flatten()
        .any(|n| !n.is_finite())
    {
        layout.fail(id, "positioned inset is not finite");
        return false;
    }
    let sizing = |value: f32, edges: f32| {
        if style.layout.box_sizing == BoxSizing::ContentBox {
            value + edges
        } else {
            value.max(edges)
        }
    };
    let constrain = |value: f32, min: Length, max: Length, basis: f32, edges: f32| {
        let maximum = max
            .resolve(basis)
            .map(|n| sizing(n, edges))
            .unwrap_or(MAX_EXTENT);
        let minimum = min
            .resolve(basis)
            .map(|n| sizing(n, edges))
            .unwrap_or(edges);
        value.min(maximum).max(minimum).clamp(0.0, MAX_EXTENT)
    };
    let explicit_h = style.height.resolve(containing.h);
    let stretch_h = top.zip(bottom);
    let height_is_definite = explicit_h.is_some() || stretch_h.is_some();
    let definite_border_h = height_is_definite.then(|| {
        let raw_h = if let Some(h) = explicit_h {
            sizing(h, vertical)
        } else {
            let (t, b) = stretch_h.unwrap();
            (containing.h - t - b - margin[0] - margin[2]).max(vertical)
        };
        constrain(
            raw_h,
            style.min_height,
            style.max_height,
            containing.h,
            vertical,
        )
    });
    let is_replaced = layout.is_replaced(id);
    let is_image_or_svg = matches!(layout.document.nodes[id].tag.as_str(), "img" | "svg");

    let width = if let Some(width) = style.width.resolve(containing.w) {
        sizing(width, horizontal)
    } else if !style.layout.width_fit_content
        && !style.layout.width_max_content
        && let (Some(left), Some(right)) = (left, right)
    {
        (containing.w - left - right - margin[1] - margin[3]).max(horizontal)
    } else if !style.layout.width_fit_content
        && !style.layout.width_max_content
        && let Some(used_h) = definite_border_h
        && (style.layout.aspect_ratio.is_some() || (is_replaced && is_image_or_svg))
    {
        if is_replaced {
            let Some((nat_w, nat_h)) = layout.measure_natural_replaced(id, entry.depth + 1) else {
                layout.fail(id, "positioned replaced content measurement rejected");
                return false;
            };
            let nat_ratio = (nat_w > 0.0 && nat_h > 0.0).then_some(nat_w / nat_h);
            let ratio = if style.layout.aspect_ratio_auto {
                nat_ratio.or(style.layout.aspect_ratio)
            } else {
                style.layout.aspect_ratio.or(nat_ratio)
            };
            if let Some(r) = ratio {
                if !style.layout.aspect_ratio_auto
                    && style.layout.aspect_ratio.is_some()
                    && style.layout.box_sizing == BoxSizing::BorderBox
                {
                    (used_h * r).max(horizontal)
                } else {
                    (used_h - vertical).max(0.0) * r + horizontal
                }
            } else {
                nat_w + horizontal
            }
        } else {
            let r = style.layout.aspect_ratio.unwrap();
            if style.layout.box_sizing == BoxSizing::BorderBox {
                (used_h * r).max(horizontal)
            } else {
                (used_h - vertical).max(0.0) * r + horizontal
            }
        }
    } else {
        let intrinsic = if style.layout.width_fit_content || style.layout.width_max_content {
            layout.measure_fit_content_intrinsic(
                id,
                containing.w,
                Some(containing.h),
                entry.depth + 1,
            )
        } else {
            layout.measure_content_intrinsic(id, entry.depth + 1)
        };
        let Some(intrinsic) = intrinsic else {
            layout.fail(id, "positioned shrink-to-fit measurement rejected");
            return false;
        };
        let available = (containing.w
            - left.unwrap_or(0.0)
            - right.unwrap_or(0.0)
            - margin[1]
            - margin[3]
            - horizontal)
            .max(0.0);
        if style.layout.width_max_content {
            intrinsic.max + horizontal
        } else {
            intrinsic.max.min(available).max(intrinsic.min) + horizontal
        }
    };
    let width = if style.layout.max_width_fit_content {
        let Some(intrinsic) = layout.measure_fit_content_intrinsic(
            id,
            containing.w,
            Some(containing.h),
            entry.depth + 1,
        ) else {
            layout.fail(id, "positioned fit-content maximum measurement rejected");
            return false;
        };
        let stretch = (containing.w
            - left.unwrap_or(0.0)
            - right.unwrap_or(0.0)
            - margin[1]
            - margin[3]
            - horizontal)
            .max(0.0);
        width.min(intrinsic.max.min(intrinsic.min.max(stretch)) + horizontal)
    } else {
        width
    };
    let mut width = constrain(
        width,
        style.min_width,
        style.max_width,
        containing.w,
        horizontal,
    );
    if let Some(minimum) = style.layout.min_width_intrinsic {
        let intrinsic = if minimum == IntrinsicSize::FitContent {
            layout.measure_fit_content_intrinsic(
                id,
                containing.w,
                Some(containing.h),
                entry.depth + 1,
            )
        } else {
            layout.measure_content_intrinsic(id, entry.depth + 1)
        };
        let Some(intrinsic) = intrinsic else {
            layout.fail(id, "positioned intrinsic minimum measurement rejected");
            return false;
        };
        let content_minimum = match minimum {
            IntrinsicSize::MinContent => intrinsic.min,
            IntrinsicSize::MaxContent => intrinsic.max,
            IntrinsicSize::FitContent => intrinsic.max.min(
                intrinsic.min.max(
                    (containing.w
                        - left.unwrap_or(0.0)
                        - right.unwrap_or(0.0)
                        - margin[1]
                        - margin[3]
                        - horizontal)
                        .max(0.0),
                ),
            ),
        };
        width = width.max(content_minimum + horizontal).min(MAX_EXTENT);
    }
    let content_width = (width - horizontal).max(0.0);
    let height = if let Some(used_h) = definite_border_h {
        used_h
    } else if is_replaced {
        let Some((natural_width, natural_height)) =
            layout.measure_natural_replaced(id, entry.depth + 1)
        else {
            layout.fail(id, "positioned replaced content measurement rejected");
            return false;
        };
        let nat_ratio = (is_image_or_svg && natural_width > 0.0 && natural_height > 0.0)
            .then_some(natural_width / natural_height);
        let ratio = if style.layout.aspect_ratio_auto {
            nat_ratio.or(style.layout.aspect_ratio)
        } else {
            style.layout.aspect_ratio.or(nat_ratio)
        };
        let raw_h = if let Some(r) = ratio {
            if !style.layout.aspect_ratio_auto
                && style.layout.aspect_ratio.is_some()
                && style.layout.box_sizing == BoxSizing::BorderBox
            {
                (width / r).max(vertical)
            } else {
                content_width / r + vertical
            }
        } else if layout.document.nodes[id].tag == "img" && natural_width > 0.0 {
            natural_height * content_width / natural_width + vertical
        } else {
            natural_height + vertical
        };
        constrain(
            raw_h,
            style.min_height,
            style.max_height,
            containing.h,
            vertical,
        )
    } else if let Some(ar) = style.layout.aspect_ratio {
        let ar_border_h = if style.layout.box_sizing == BoxSizing::BorderBox {
            (width / ar).max(vertical)
        } else {
            content_width / ar + vertical
        };
        let auto_min_border_h = if style.min_height == Length::Auto
            && style.layout.overflow == [crate::style::Overflow::Visible; 2]
        {
            let Some(ch) = layout.measure_content_height(id, content_width, None, entry.depth + 1)
            else {
                layout.fail(id, "positioned content height measurement rejected");
                return false;
            };
            let max_b = style
                .max_height
                .resolve(containing.h)
                .map(|n| sizing(n, vertical))
                .unwrap_or(MAX_EXTENT);
            (ch + vertical).min(max_b)
        } else {
            vertical
        };
        constrain(
            ar_border_h.max(auto_min_border_h),
            style.min_height,
            style.max_height,
            containing.h,
            vertical,
        )
    } else {
        let Some(height) = layout.measure_content_height(id, content_width, None, entry.depth + 1)
        else {
            layout.fail(id, "positioned content height measurement rejected");
            return false;
        };
        constrain(
            height + vertical,
            style.min_height,
            style.max_height,
            containing.h,
            vertical,
        )
    };
    // Auto margins only consume free space when both insets are definite.
    for (start, end, before, after, extent, size) in [
        (3, 1, left, right, containing.w, width),
        (0, 2, top, bottom, containing.h, height),
    ] {
        if let (Some(before), Some(after)) = (before, after) {
            let extra = (extent - before - after - size - margin[start] - margin[end]).max(0.0);
            match (style.margin[start], style.margin[end]) {
                (Length::Auto, Length::Auto) => {
                    margin[start] = extra / 2.0;
                    margin[end] = extra / 2.0;
                }
                (Length::Auto, _) => margin[start] = extra,
                (_, Length::Auto) => margin[end] = extra,
                _ => {}
            }
        }
    }
    let fixed_scroll = if fixed {
        layout.viewport.scroll as f32
    } else {
        0.0
    };
    let abspos_align = layout
        .scene
        .abspos_alignments
        .get(&id)
        .copied()
        .map(|mut a| {
            a.rect.y += fixed_scroll;
            a
        });
    let static_pos = layout
        .scene
        .static_positions
        .get(&id)
        .copied()
        .map(|(sx, sy, rtl)| (sx, sy + fixed_scroll, rtl));
    let x =
        left.map(|n| containing.x + n + margin[3])
            .or_else(|| right.map(|n| containing.x + containing.w - n - width - margin[1]))
            .unwrap_or_else(|| {
                if let Some(align) = abspos_align {
                    resolve_axis_align(
                        align.h_align,
                        align.h_safe_fallback,
                        align.rect.x,
                        align.rect.w,
                        width,
                        margin[3],
                        margin[1],
                    )
                } else if let Some((sx, _, is_rtl)) = static_pos {
                    if is_rtl {
                        sx - width - margin[1]
                    } else {
                        sx + margin[3]
                    }
                } else if entry.containing.is_some_and(|cb| {
                    layout.styles[cb].direction == crate::style::TextDirection::Rtl
                }) {
                    containing.x + containing.w - width - margin[1]
                } else {
                    containing.x + margin[3]
                }
            });
    let y = top
        .map(|n| containing.y + n + margin[0])
        .or_else(|| bottom.map(|n| containing.y + containing.h - n - height - margin[2]))
        .unwrap_or_else(|| {
            if let Some(align) = abspos_align {
                resolve_axis_align(
                    align.v_align,
                    align.v_safe_fallback,
                    align.rect.y,
                    align.rect.h,
                    height,
                    margin[0],
                    margin[2],
                )
            } else if let Some((_, sy, _)) = static_pos {
                sy + margin[0]
            } else {
                containing.y + margin[0]
            }
        });
    if ![x, y, width, height]
        .into_iter()
        .all(|n| n.is_finite() && n.abs() <= MAX_EXTENT)
    {
        layout.fail(id, "positioned box exceeds finite extent limit");
        return false;
    }
    let rect = Rect {
        x,
        y,
        w: width,
        h: height,
    };
    let mut clip: Option<Rect> = None;
    let mut rounded_clips: Vec<crate::paint::RoundedRect> = Vec::new();
    if !fixed {
        let mut ancestor = layout.document.nodes[id].parent;
        for depth in 0..=MAX_DEPTH {
            if !layout.spend(depth) {
                layout.fail(id, "positioned clipping ancestor depth or work limit");
                return false;
            }
            let ancestor_style = &layout.styles[ancestor];
            let ends_clipping = ancestor_style.layout.position == Position::Fixed;
            if ancestor_style
                .layout
                .overflow
                .iter()
                .any(|overflow| !matches!(overflow, crate::style::Overflow::Visible))
            {
                if !layout.spend(0) {
                    layout.fail(ancestor, "positioned border lookup work limit");
                    return false;
                }
                if let Some(r) = border_boxes.get(ancestor).copied().flatten() {
                    if let Some(next) = layout.clip_for(ancestor, r) {
                        clip = Some(clip.map_or(next, |r| r.intersect(next)));
                    }
                    if let Some(rr) = layout.rounded_clip_for(ancestor, r) {
                        rounded_clips.push(rr);
                    }
                }
            }
            if ancestor == 0 || ends_clipping {
                break;
            }
            ancestor = layout.document.nodes[ancestor].parent;
        }
    }
    let mark = layout.scene.mark();
    let definite_height = (height_is_definite || style.layout.aspect_ratio.is_some())
        .then_some((height - vertical).max(0.0));
    layout.paint_allocated(id, rect, padding, definite_height, entry.depth + 1);
    for &(node, r) in &layout.scene.borders[mark.3..] {
        if let Some(slot) = border_boxes.get_mut(node) {
            *slot = Some(r);
        }
    }
    if fixed || entry.fixed_ancestor {
        layout.scene.box_scroll[mark.1..].fill(false);
    }
    if let Some(clip) = clip {
        layout.scene.clip(mark, clip);
    }
    for rr in rounded_clips {
        layout.scene.clip_rounded(mark, rr);
    }
    if layout.exhausted {
        layout.fail(id, "positioned paint or descendant layout rejected");
    }
    !layout.exhausted
}
