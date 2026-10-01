//! Bounded out-of-flow boxes after normal layout. This is not the complete CSS
//! stacking-context or hypothetical static-position algorithm.

use super::*;
use crate::style::{BoxSizing, IntrinsicSize, Position};

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
        {
            continue;
        }
        count += 1;
        if count > MAX_POSITIONED {
            layout.fail(id, "positioned node limit (512)");
            return;
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
            hidden |= style.display == Display::None || !style.visible;
            if cursor != id {
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
        if layout.styles[id].layout.unsupported {
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
    // Ancestors must provide final geometry before descendants. Among ready
    // nodes, numeric z-index and then DOM order decide paint and hit order.
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
        if !paint(layout, &entries[index]) {
            layout.fail(entries[index].id, "positioned box rejected");
            return;
        }
        entries[index].done = true;
    }
}

fn border(layout: &mut Layout<'_, '_>, id: usize, missing: &'static str) -> Option<Rect> {
    // The existing structural geometry vector is bounded but its reverse lookup
    // is linear. Charge the whole scan conservatively, not just its caller.
    let Some(budget) = layout.budget.checked_sub(layout.scene.borders.len()) else {
        layout.fail(id, "positioned border lookup work limit");
        return None;
    };
    layout.budget = budget;
    let rect = layout.border_box(id);
    if rect.is_none() {
        layout.fail(id, missing);
    }
    rect
}

fn paint(layout: &mut Layout<'_, '_>, entry: &Entry) -> bool {
    let id = entry.id;
    let style = layout.styles[id].clone();
    let fixed = style.layout.position == Position::Fixed;
    let containing = if !fixed && let Some(ancestor) = entry.containing {
        let Some(r) = border(layout, ancestor, "positioned containing border box missing") else {
            return false;
        };
        let b = layout.styles[ancestor].border_width;
        Rect {
            x: r.x + b[3],
            y: r.y + b[0],
            w: (r.w - b[1] - b[3]).max(0.0),
            h: (r.h - b[0] - b[2]).max(0.0),
        }
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
    let width = if let Some(width) = style.width.resolve(containing.w) {
        sizing(width, horizontal)
    } else if !style.layout.width_fit_content
        && !style.layout.width_max_content
        && let (Some(left), Some(right)) = (left, right)
    {
        (containing.w - left - right - margin[1] - margin[3]).max(horizontal)
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
    let height = if let Some(height) = style.height.resolve(containing.h) {
        sizing(height, vertical)
    } else if let (Some(top), Some(bottom)) = (top, bottom) {
        (containing.h - top - bottom - margin[0] - margin[2]).max(vertical)
    } else if layout.is_replaced(id) {
        let Some((natural_width, natural_height)) =
            layout.measure_natural_replaced(id, entry.depth + 1)
        else {
            layout.fail(id, "positioned replaced content measurement rejected");
            return false;
        };
        let height = if layout.document.nodes[id].tag == "img" && natural_width > 0.0 {
            natural_height * content_width / natural_width
        } else {
            natural_height
        };
        height + vertical
    } else {
        let Some(height) = layout.measure_content_height(id, content_width, None, entry.depth + 1)
        else {
            layout.fail(id, "positioned content height measurement rejected");
            return false;
        };
        height + vertical
    };
    let height = constrain(
        height,
        style.min_height,
        style.max_height,
        containing.h,
        vertical,
    );
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
    let x = containing.x
        + left.map(|n| n + margin[3]).unwrap_or_else(|| {
            right
                .map(|n| containing.w - n - width - margin[1])
                .unwrap_or(margin[3])
        });
    let y = containing.y
        + top.map(|n| n + margin[0]).unwrap_or_else(|| {
            bottom
                .map(|n| containing.h - n - height - margin[2])
                .unwrap_or(margin[0])
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
                let Some(r) = border(layout, ancestor, "positioned overflow ancestor box missing")
                else {
                    return false;
                };
                if let Some(next) = layout.clip_for(ancestor, r) {
                    clip = Some(clip.map_or(next, |r| r.intersect(next)));
                }
            }
            if ancestor == 0 || ends_clipping {
                break;
            }
            ancestor = layout.document.nodes[ancestor].parent;
        }
    }
    let mark = layout.scene.mark();
    let definite_height = (style.height.resolve(containing.h).is_some()
        || (top.is_some() && bottom.is_some()))
    .then_some((height - vertical).max(0.0));
    layout.paint_allocated(id, rect, padding, definite_height, entry.depth + 1);
    if fixed || entry.fixed_ancestor {
        layout.scene.box_scroll[mark.1..].fill(false);
    }
    if let Some(clip) = clip {
        layout.scene.clip(mark, clip);
    }
    if layout.exhausted {
        layout.fail(id, "positioned paint or descendant layout rejected");
    }
    !layout.exhausted
}
