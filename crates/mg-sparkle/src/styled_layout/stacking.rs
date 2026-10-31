//! Bounded CSS 2.1 Appendix E stacking contexts and painting order.
use super::*;
use crate::style::Position;

const MAX_CONTEXTS: usize = 512;

struct Scope {
    node: usize,
    is_real: bool,
    z: i32,
    order: usize,
    real_children: Vec<usize>,
    step2_float_children: Vec<usize>,
    step3_children: Vec<usize>,
    step4_pseudo_children: Vec<usize>,
    bg_rank: usize,
    block_bg_rank: usize,
    step3_gaps: Vec<(usize, usize)>,
    step3_tail_rank: usize,
}

impl Scope {
    fn new(node: usize, is_real: bool, z: i32, order: usize) -> Self {
        Self {
            node,
            is_real,
            z,
            order,
            real_children: Vec::new(),
            step2_float_children: Vec::new(),
            step3_children: Vec::new(),
            step4_pseudo_children: Vec::new(),
            bg_rank: 0,
            block_bg_rank: 0,
            step3_gaps: Vec::new(),
            step3_tail_rank: 0,
        }
    }

    fn step3_rank(&self, node_preorder: usize) -> usize {
        if self.step3_gaps.is_empty() {
            return self.step3_tail_rank;
        }
        let idx = self
            .step3_gaps
            .partition_point(|&(boundary, _)| boundary <= node_preorder);
        if idx < self.step3_gaps.len() {
            self.step3_gaps[idx].1
        } else {
            self.step3_tail_rank
        }
    }
}

fn charge(layout: &mut Layout<'_, '_>, amount: usize, reason: &'static str) -> bool {
    if amount > layout.budget {
        layout.fail(0, reason);
        false
    } else {
        layout.budget -= amount;
        true
    }
}

fn sort_work(count: usize) -> usize {
    count.saturating_mul((usize::BITS - count.max(1).leading_zeros()) as usize + 1)
}

fn effective_parent_is_format(layout: &Layout<'_, '_>, id: usize) -> bool {
    if id == 0 {
        return false;
    }
    let mut p = layout.document.nodes[id].parent;
    for _ in 0..MAX_DEPTH {
        if p == 0
            || layout.document.nodes[p].tag == "#text"
            || layout.styles[p].display != Display::Contents
        {
            break;
        }
        p = layout.document.nodes[p].parent;
    }
    is_format(layout.styles[p].display)
}

fn assign_ranks(
    layout: &mut Layout<'_, '_>,
    scopes: &mut [Scope],
    index: usize,
    depth: usize,
    next: &mut usize,
) -> bool {
    if !layout.spend(depth) {
        layout.fail(scopes[index].node, "stacking context depth or work limit");
        return false;
    }
    let mut real_children = std::mem::take(&mut scopes[index].real_children);
    let mut step2_floats = std::mem::take(&mut scopes[index].step2_float_children);
    let mut step3_children = std::mem::take(&mut scopes[index].step3_children);
    let mut step4_pseudo = std::mem::take(&mut scopes[index].step4_pseudo_children);

    let work = sort_work(real_children.len())
        .saturating_add(sort_work(step2_floats.len()))
        .saturating_add(sort_work(step3_children.len()))
        .saturating_add(sort_work(step4_pseudo.len()));
    if !charge(layout, work, "stacking child ordering work limit") {
        return false;
    }

    real_children.sort_unstable_by_key(|&c| (scopes[c].z, scopes[c].order));
    step2_floats.sort_unstable_by_key(|&c| scopes[c].order);
    step3_children.sort_unstable_by_key(|&c| scopes[c].order);

    // Step 0: context/scope own background and borders.
    scopes[index].bg_rank = *next;
    *next += 1;

    // Step 1: negative z-index child stacking contexts (real contexts only).
    let neg_end = real_children.partition_point(|&c| scopes[c].z < 0);
    let zero_end = real_children.partition_point(|&c| scopes[c].z <= 0);
    if scopes[index].is_real {
        for &child in &real_children[..neg_end] {
            if !assign_ranks(layout, scopes, child, depth + 1, next) {
                return false;
            }
        }
    }

    // Step 2a: in-flow non-positioned block-level backgrounds and borders.
    scopes[index].block_bg_rank = *next;
    *next += 1;

    // Step 2b: non-positioned floats painted as pseudo-stacking contexts in tree order.
    for &fl in &step2_floats {
        if !assign_ranks(layout, scopes, fl, depth + 1, next) {
            return false;
        }
    }

    // Step 3: inline-level content interleaved with atomic inline / flex / grid item pseudo-scopes.
    let mut gaps = Vec::with_capacity(step3_children.len());
    for &ps in &step3_children {
        let gap_rank = *next;
        *next += 1;
        gaps.push((scopes[ps].order, gap_rank));
        if !assign_ranks(layout, scopes, ps, depth + 1, next) {
            return false;
        }
    }
    scopes[index].step3_gaps = gaps;
    scopes[index].step3_tail_rank = *next;
    *next += 1;

    // Step 4: positioned z-index:auto pseudo-scopes interleaved in DOM order with z == 0 real contexts.
    if scopes[index].is_real && neg_end < zero_end {
        step4_pseudo.extend_from_slice(&real_children[neg_end..zero_end]);
    }
    if !charge(
        layout,
        sort_work(step4_pseudo.len()),
        "stacking step-4 ordering work limit",
    ) {
        return false;
    }
    step4_pseudo.sort_unstable_by_key(|&c| scopes[c].order);
    for &child in &step4_pseudo {
        if !assign_ranks(layout, scopes, child, depth + 1, next) {
            return false;
        }
    }

    // Step 5: positive z-index child stacking contexts (real contexts only).
    if scopes[index].is_real {
        for &child in &real_children[zero_end..] {
            if !assign_ranks(layout, scopes, child, depth + 1, next) {
                return false;
            }
        }
    }
    true
}

/// Apply new-position -> old-position order without cloning any scene payload.
fn permute(order: &mut [usize], mut swap: impl FnMut(usize, usize)) {
    for start in 0..order.len() {
        let mut cursor = start;
        while order[cursor] != start {
            let next = order[cursor];
            swap(cursor, next);
            order[cursor] = cursor;
            cursor = next;
        }
        order[cursor] = cursor;
    }
}

pub(super) fn run(layout: &mut Layout<'_, '_>) {
    if layout.exhausted {
        return;
    }
    let nodes = layout.document.nodes.len();
    let paints = layout.scene.paints.len();
    let hits = layout.scene.hits.len();
    if nodes == 0
        || nodes > MAX_OPS
        || paints >= MAX_OPS
        || hits >= MAX_OPS
        || layout.scene.clips.len() != paints
        || layout.scene.rounded_clips.len() != paints
        || layout.scene.paint_nodes.len() != paints
        || layout.scene.hit_nodes.len() != hits
    {
        layout.fail(0, "stacking scene or owner metadata admission limit");
        return;
    }
    let mut scope_owners = vec![usize::MAX; nodes];
    let mut preorders = vec![0usize; nodes];
    let mut scopes = vec![Scope::new(0, true, 0, 0)];
    let mut real_count = 0usize;
    let mut pending = vec![(0usize, 0usize, 0usize, 0usize)];
    let mut preorder = 0usize;

    while let Some((id, parent_real, parent_scope, depth)) = pending.pop() {
        if id >= nodes || scope_owners[id] != usize::MAX || !layout.spend(depth) {
            layout.fail(id, "stacking DOM tree depth, identity or work limit");
            return;
        }
        let style = &layout.styles[id];
        let is_element = id != 0
            && layout.document.nodes[id].tag != "#text"
            && style.display != Display::None
            && style.opacity > 0.0
            && !positioned::is_inset_zero_passthrough(layout, id);

        let parent_is_fmt = is_element && effective_parent_is_format(layout, id);
        let is_flex_or_grid_item = parent_is_fmt
            && !matches!(style.layout.position, Position::Absolute | Position::Fixed)
            && style.display != Display::Contents;
        let z_applies = style.layout.position != Position::Static || is_flex_or_grid_item;

        let creates_real = is_element
            && (matches!(style.layout.position, Position::Fixed | Position::Sticky)
                || style.own_opacity < 1.0
                || (z_applies && style.layout.z_index.is_some()));

        let (next_real, next_scope) = if creates_real {
            real_count += 1;
            if real_count > MAX_CONTEXTS {
                layout.fail(id, "stacking context limit (512)");
                return;
            }
            if scopes.len() >= MAX_OPS {
                layout.fail(id, "stacking scope scratch limit");
                return;
            }
            let z = if z_applies {
                style.layout.z_index.unwrap_or(0)
            } else {
                0
            };
            let idx = scopes.len();
            scopes.push(Scope::new(id, true, z, preorder));
            scopes[parent_real].real_children.push(idx);
            (idx, idx)
        } else if is_element
            && matches!(
                style.layout.position,
                Position::Relative | Position::Absolute
            )
        {
            if scopes.len() >= MAX_OPS {
                layout.fail(id, "stacking scope scratch limit");
                return;
            }
            let idx = scopes.len();
            scopes.push(Scope::new(id, false, 0, preorder));
            scopes[parent_scope].step4_pseudo_children.push(idx);
            (parent_real, idx)
        } else if is_element
            && !is_flex_or_grid_item
            && style.layout.position == Position::Static
            && style.layout.float != crate::style::FloatSide::None
        {
            if scopes.len() >= MAX_OPS {
                layout.fail(id, "stacking scope scratch limit");
                return;
            }
            let idx = scopes.len();
            scopes.push(Scope::new(id, false, 0, preorder));
            scopes[parent_scope].step2_float_children.push(idx);
            (parent_real, idx)
        } else if is_element
            && style.layout.position == Position::Static
            && (is_flex_or_grid_item
                || matches!(
                    style.display,
                    Display::InlineBlock
                        | Display::InlineTable
                        | Display::InlineFlex
                        | Display::InlineGrid
                )
                || (layout.is_replaced(id)
                    && !(layout.document.nodes[id].tag == "svg" && is_block(style.display))))
        {
            if scopes.len() >= MAX_OPS {
                layout.fail(id, "stacking scope scratch limit");
                return;
            }
            let idx = scopes.len();
            scopes.push(Scope::new(id, false, 0, preorder));
            scopes[parent_scope].step3_children.push(idx);
            (parent_real, idx)
        } else {
            (parent_real, parent_scope)
        };

        scope_owners[id] = next_scope;
        preorders[id] = preorder;
        preorder += 1;

        let children = &layout.document.nodes[id].children;
        let sort_by_order = id != 0
            && layout.document.nodes[id].tag != "#text"
            && (is_format(style.display) || (style.display == Display::Contents && parent_is_fmt))
            && children.iter().any(|&c| {
                !matches!(
                    layout.styles[c].layout.position,
                    Position::Absolute | Position::Fixed
                ) && layout.styles[c].layout.order != 0
            });
        if sort_by_order {
            let mut ordered = children.clone();
            ordered.sort_by_key(|&c| {
                if matches!(
                    layout.styles[c].layout.position,
                    Position::Absolute | Position::Fixed
                ) {
                    0
                } else {
                    layout.styles[c].layout.order
                }
            });
            for &child in ordered.iter().rev() {
                if pending.len() >= MAX_OPS {
                    layout.fail(id, "stacking tree scratch limit");
                    return;
                }
                pending.push((child, next_real, next_scope, depth + 1));
            }
        } else {
            for &child in children.iter().rev() {
                if pending.len() >= MAX_OPS {
                    layout.fail(id, "stacking tree scratch limit");
                    return;
                }
                pending.push((child, next_real, next_scope, depth + 1));
            }
        }
    }

    if !assign_ranks(layout, &mut scopes, 0, 0, &mut 0)
        || !charge(
            layout,
            sort_work(paints).saturating_add(sort_work(hits)),
            "stacking scene ordering work limit",
        )
    {
        return;
    }

    let mut order: Vec<_> = (0..paints).collect();
    if layout.scene.paint_nodes.iter().any(|&id| id >= nodes)
        || layout.scene.hit_nodes.iter().any(|&id| id >= nodes)
    {
        layout.fail(0, "stacking scene refers to an unreachable owner");
        return;
    }
    // Note: nodes inside display:none or passthrough wrappers never emit paints,
    // except if an unreachable owner somehow emitted one, guard `scope_owners[id] == usize::MAX`.
    if layout
        .scene
        .paint_nodes
        .iter()
        .any(|&id| scope_owners[id] == usize::MAX)
        || layout
            .scene
            .hit_nodes
            .iter()
            .any(|&id| scope_owners[id] == usize::MAX)
    {
        layout.fail(0, "stacking scene refers to an unreachable owner");
        return;
    }

    order.sort_unstable_by_key(|&index| {
        let node = layout.scene.paint_nodes[index];
        let scope_idx = scope_owners[node];
        let scope = &scopes[scope_idx];
        let is_decoration_op = matches!(
            layout.scene.paints[index],
            Paint::Background(..)
                | Paint::RoundedDecoration(..)
                | Paint::Rect(..)
                | Paint::Image(..)
        );
        let own_bg = (node == scope.node
            || (scope_idx == 0
                && layout.styles[node].layout.position == Position::Static
                && matches!(layout.document.nodes[node].tag.as_str(), "html" | "body")))
            && is_decoration_op;
        let rank = if own_bg {
            scope.bg_rank
        } else if node != scope.node
            && is_decoration_op
            && is_block(layout.styles[node].display)
            && !layout.is_replaced(node)
        {
            scope.block_bg_rank
        } else {
            scope.step3_rank(preorders[node])
        };
        (rank, preorders[node], index)
    });
    permute(&mut order, |a, b| {
        layout.scene.paints.swap(a, b);
        layout.scene.clips.swap(a, b);
        layout.scene.rounded_clips.swap(a, b);
        layout.scene.paint_nodes.swap(a, b);
    });

    order.clear();
    order.extend(0..hits);
    order.sort_unstable_by_key(|&index| {
        let node = layout.scene.hit_nodes[index];
        let scope_idx = scope_owners[node];
        let scope = &scopes[scope_idx];
        let rank = if node == scope.node {
            scope.bg_rank
        } else if is_block(layout.styles[node].display) && !layout.is_replaced(node) {
            scope.block_bg_rank
        } else {
            scope.step3_rank(preorders[node])
        };
        (rank, preorders[node], index)
    });
    permute(&mut order, |a, b| {
        layout.scene.hits.swap(a, b);
        layout.scene.hit_nodes.swap(a, b);
    });
}

#[cfg(test)]
mod tests {
    use super::permute;

    #[test]
    fn cycles_preserve_parallel_scene_metadata() {
        for order in [vec![0, 1, 2, 3], vec![2, 0, 3, 1], vec![3, 2, 1, 0]] {
            let mut mutable = order.clone();
            let mut values = vec![0, 1, 2, 3];
            let mut metadata = vec![10, 11, 12, 13];
            permute(&mut mutable, |a, b| {
                values.swap(a, b);
                metadata.swap(a, b);
            });
            assert_eq!(values, order);
            assert_eq!(
                metadata,
                order.iter().map(|value| value + 10).collect::<Vec<_>>()
            );
            assert_eq!(mutable, vec![0, 1, 2, 3]);
        }
    }
}
