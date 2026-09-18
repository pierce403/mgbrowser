//! Bounded numeric positioned stacking contexts after geometry is complete.
//!
//! A numeric relative ancestor contains its positioned descendants just like a
//! numeric absolute ancestor. This pass is not complete CSS painting order:
//! auto-z positioned/floating order inside one context remains the admitted
//! scene order, and opacity, transform and isolation contexts are not modeled.
use super::*;
use crate::style::Position;

const MAX_CONTEXTS: usize = 512;

struct Context {
    node: usize,
    z: i32,
    order: usize,
    children: Vec<usize>,
    background: usize,
    contents: usize,
}

impl Context {
    fn new(node: usize, z: i32, order: usize) -> Self {
        Self {
            node,
            z,
            order,
            children: Vec::new(),
            background: 0,
            contents: 0,
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

fn ranks(
    layout: &mut Layout<'_, '_>,
    contexts: &mut [Context],
    index: usize,
    depth: usize,
    next: &mut usize,
) -> bool {
    if !layout.spend(depth) {
        layout.fail(contexts[index].node, "stacking context depth or work limit");
        return false;
    }
    let mut children = std::mem::take(&mut contexts[index].children);
    if !charge(
        layout,
        sort_work(children.len()),
        "stacking child ordering work limit",
    ) {
        return false;
    }
    children.sort_unstable_by_key(|&child| (contexts[child].z, contexts[child].order));
    contexts[index].background = *next;
    *next += 1;
    let split = children.partition_point(|&child| contexts[child].z < 0);
    for &child in &children[..split] {
        if !ranks(layout, contexts, child, depth + 1, next) {
            return false;
        }
    }
    contexts[index].contents = *next;
    *next += 1;
    for &child in &children[split..] {
        if !ranks(layout, contexts, child, depth + 1, next) {
            return false;
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
        || layout.scene.paint_nodes.len() != paints
        || layout.scene.hit_nodes.len() != hits
    {
        layout.fail(0, "stacking scene or owner metadata admission limit");
        return;
    }
    let mut owners = vec![usize::MAX; nodes];
    let mut contexts = vec![Context::new(0, 0, 0)];
    let mut pending = vec![(0, 0, 0)];
    let mut preorder = 0;
    while let Some((id, parent_context, depth)) = pending.pop() {
        if id >= nodes || owners[id] != usize::MAX || !layout.spend(depth) {
            layout.fail(id, "stacking DOM tree depth, identity or work limit");
            return;
        }
        let style = &layout.styles[id];
        let numeric_positioned = matches!(
            style.layout.position,
            Position::Relative | Position::Absolute | Position::Fixed
        ) && style.layout.z_index.is_some();
        let context = if id != 0
            && layout.document.nodes[id].tag != "#text"
            && (numeric_positioned || style.layout.position == Position::Fixed)
        {
            if contexts.len() > MAX_CONTEXTS {
                layout.fail(id, "stacking context limit (512)");
                return;
            }
            let index = contexts.len();
            contexts.push(Context::new(
                id,
                style.layout.z_index.unwrap_or(0),
                preorder,
            ));
            contexts[parent_context].children.push(index);
            index
        } else {
            parent_context
        };
        owners[id] = context;
        preorder += 1;
        for &child in layout.document.nodes[id].children.iter().rev() {
            if pending.len() >= MAX_OPS {
                layout.fail(id, "stacking tree scratch limit");
                return;
            }
            pending.push((child, context, depth + 1));
        }
    }
    // Most legacy pages have no positioned stacking contexts. Retain their
    // exact historical scene order and avoid an unnecessary sorting pass.
    if contexts.len() == 1 {
        return;
    }
    if !ranks(layout, &mut contexts, 0, 0, &mut 0)
        || !charge(
            layout,
            sort_work(paints).saturating_add(sort_work(hits)),
            "stacking scene ordering work limit",
        )
    {
        return;
    }
    if layout
        .scene
        .paint_nodes
        .iter()
        .any(|&id| id >= nodes || owners[id] == usize::MAX)
        || layout
            .scene
            .hit_nodes
            .iter()
            .any(|&id| id >= nodes || owners[id] == usize::MAX)
    {
        layout.fail(0, "stacking scene refers to an unreachable owner");
        return;
    }
    let mut order: Vec<_> = (0..paints).collect();
    order.sort_unstable_by_key(|&index| {
        let node = layout.scene.paint_nodes[index];
        let context = &contexts[owners[node]];
        // All box rect/border/background image operations precede a context's
        // negative children. A replaced image has no separately painted child
        // content here. Root HTML/body backgrounds remain canvas backgrounds.
        let own_background = (node == context.node
            || (owners[node] == 0
                && matches!(layout.document.nodes[node].tag.as_str(), "html" | "body")))
            && matches!(
                layout.scene.paints[index],
                Paint::Background(..) | Paint::Rect(..) | Paint::Image(..)
            );
        (
            if own_background {
                context.background
            } else {
                context.contents
            },
            index,
        )
    });
    permute(&mut order, |a, b| {
        layout.scene.paints.swap(a, b);
        layout.scene.clips.swap(a, b);
        layout.scene.paint_nodes.swap(a, b);
    });
    order.clear();
    order.extend(0..hits);
    order.sort_unstable_by_key(|&index| {
        let node = layout.scene.hit_nodes[index];
        (contexts[owners[node]].contents, index)
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
