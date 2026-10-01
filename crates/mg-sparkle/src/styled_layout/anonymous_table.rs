//! Pure-cell anonymous table wrappers, without changing the authored DOM.
//! https://www.w3.org/TR/CSS22/tables.html#anonymous-boxes
//!
//! Mixed ordinary/table content remains an explicit unsupported boundary. Real
//! table rows and this one anonymous row share columns, measurement and paint.
use super::*;
use crate::style::Position;

pub(super) fn prepare(layout: &mut Layout<'_, '_>) {
    let count = layout.document.nodes.len();
    if count > MAX_OPS {
        layout.fail(0, "anonymous table node admission limit");
        return;
    }
    layout.anonymous_tables = vec![false; count];
    for id in 0..count {
        if !layout.spend(0) {
            layout.fail(id, "anonymous table discovery work limit");
            return;
        }
        if layout.document.nodes[id].tag == "#text"
            || !matches!(
                layout.styles[id].display,
                Display::Block | Display::InlineBlock | Display::ListItem | Display::TableCell
            )
        {
            continue;
        }
        let mut cells = Vec::new();
        let mut columns = 0usize;
        let mut mixed = false;
        for index in 0..layout.document.nodes[id].children.len() {
            if !layout.spend(0) {
                layout.fail(id, "anonymous table child discovery work limit");
                return;
            }
            let child = layout.document.nodes[id].children[index];
            let node = &layout.document.nodes[child];
            let style = &layout.styles[child];
            if style.display == Display::None
                || matches!(style.layout.position, Position::Absolute | Position::Fixed)
                || (node.tag == "#text" && node.text.chars().all(is_css_space))
            {
                continue;
            }
            if node.tag == "#text" || style.display != Display::TableCell {
                mixed = true;
                continue;
            }
            let span = node
                .attr("colspan")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, MAX_COLUMNS);
            if columns.saturating_add(span) <= MAX_COLUMNS {
                cells.push(Cell {
                    node: child,
                    column: columns,
                    span,
                });
            }
            columns = columns.saturating_add(span);
        }
        if columns == 0 {
            continue;
        }
        // Discovery inspects the flat DOM, whereas normal layout never enters
        // display:none subtrees. Do not admit or diagnose their orphan cells.
        let mut ancestor = id;
        let mut hidden = false;
        for depth in 0..=MAX_DEPTH {
            if !layout.spend(depth) {
                layout.fail(id, "anonymous table ancestor work limit");
                return;
            }
            if layout.styles[ancestor].display == Display::None {
                hidden = true;
                break;
            }
            if ancestor == 0 {
                break;
            }
            if depth == MAX_DEPTH {
                layout.fail(id, "anonymous table ancestor depth limit");
                return;
            }
            ancestor = layout.document.nodes[ancestor].parent;
        }
        if hidden {
            continue;
        }
        if mixed {
            layout.diagnostics.record(
                "css-unsupported",
                format_args!("Anonymous tables mixed with ordinary content are not implemented"),
                format_args!("{}", layout.document.base_url),
                Some(id),
                None,
            );
            continue;
        }
        if columns > MAX_COLUMNS {
            layout.fail(id, "anonymous table column limit (256)");
            return;
        }
        layout.anonymous_tables[id] = true;
        layout.table_cache.insert(
            id,
            Table {
                rows: vec![(None, cells)],
                columns,
            },
        );
    }
}

pub(super) fn contains(layout: &Layout<'_, '_>, id: usize) -> bool {
    layout.anonymous_tables.get(id).copied().unwrap_or(false)
}

pub(super) fn run<const EMIT: bool>(
    layout: &mut Layout<'_, '_>,
    id: usize,
    x: f32,
    y: f32,
    available: f32,
    containing_height: Option<f32>,
    depth: usize,
) -> f32 {
    let (min, max) = layout.table_columns(id, depth + 1);
    let spacing = layout.table_spacing(id)[0] * (min.len() + 1) as f32;
    let minimum = min.iter().sum::<f32>() + spacing;
    let maximum = max.iter().sum::<f32>() + spacing;
    // The wrapper is an auto-width table inside an ordinary block, not a new
    // display value for the parent. Do not stretch cells to the parent's width.
    let width = maximum.min(available).max(minimum);
    if !width.is_finite() || !(0.0..=MAX_EXTENT).contains(&width) {
        layout.fail(id, "anonymous table width exceeds finite extent limit");
        return 0.0;
    }
    layout.layout_table::<EMIT>(id, x, y, width, containing_height, depth + 1)
}
