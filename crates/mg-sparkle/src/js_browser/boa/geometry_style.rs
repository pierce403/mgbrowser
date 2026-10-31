//! Computed styles (`window.getComputedStyle`), layout box geometry
//! (`getBoundingClientRect`, `getClientRects`, `offset*`, `client*`, `scroll*`),
//! and viewport/scroll synchronization (`innerWidth`, `scrollY`, `scrollTo`, `scrollIntoView`).
use super::{
    State, dom_exception, dom_ops, failure, key, modern, property_name, receiver, state, utf8, wrap,
};
use crate::document::StylesheetSource;
use crate::page_session::platform::ScrollRequest;
use crate::render::{LayoutBox, Viewport};
use crate::style::{
    AlignKeyword, BoxSizing, Color, ComputedStyle, Display, FlexBasis, FlexDirection, FlexWrap,
    Length, Overflow, Position, TextAlign, VerticalAlign, WhiteSpace,
};
use mg_butane::runtime::Value;
use modern::boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsSymbol, JsValue, NativeFunction, js_string,
    object::{FunctionObjectBuilder, JsObject, ObjectInitializer, builtins::JsArray},
    property::{Attribute, PropertyDescriptor},
};
use std::sync::Arc;

pub(super) const GET_COMPUTED_STYLE: &str = "host.window.getComputedStyle";
pub(super) const WINDOW_SCROLL_TO: &str = "host.window.scrollTo";
pub(super) const WINDOW_SCROLL: &str = "host.window.scroll";
pub(super) const WINDOW_SCROLL_BY: &str = "host.window.scrollBy";

pub(super) const EL_GET_BCR: &str = "host.m2.getBoundingClientRect";
pub(super) const EL_GET_CLIENT_RECTS: &str = "host.m2.getClientRects";
pub(super) const EL_SCROLL_INTO_VIEW: &str = "host.m2.scrollIntoView";

pub(super) const CS_GET_PROPERTY_VALUE: &str = "host.m2.cs.getPropertyValue";
pub(super) const CS_GET_PROPERTY_PRIORITY: &str = "host.m2.cs.getPropertyPriority";
pub(super) const CS_SET_PROPERTY: &str = "host.m2.cs.setProperty";
pub(super) const CS_REMOVE_PROPERTY: &str = "host.m2.cs.removeProperty";
pub(super) const CS_ITEM: &str = "host.m2.cs.item";

const COMPUTED_PROP_NAMES: &[&str] = &[
    "display",
    "position",
    "box-sizing",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "top",
    "right",
    "bottom",
    "left",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "color",
    "background-color",
    "opacity",
    "visibility",
    "overflow-x",
    "overflow-y",
    "z-index",
    "font-size",
    "font-weight",
    "font-style",
    "font-family",
    "line-height",
    "text-align",
    "text-decoration-line",
    "vertical-align",
    "white-space",
    "pointer-events",
    "flex-direction",
    "flex-wrap",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "order",
    "row-gap",
    "column-gap",
    "align-items",
    "justify-content",
    "align-self",
    "justify-self",
    "align-content",
    "justify-items",
    "border-collapse",
];

pub(super) struct CachedLayout {
    pub epoch: u64,
    pub width: u32,
    pub height: u32,
    pub styles: Arc<[ComputedStyle]>,
    pub boxes: Vec<LayoutBox>,
    pub borders: Vec<(usize, (f32, f32, f32, f32))>,
    pub scroll_height: f32,
}

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        GET_COMPUTED_STYLE => Some(("getComputedStyle", 1)),
        WINDOW_SCROLL_TO => Some(("scrollTo", 0)),
        WINDOW_SCROLL => Some(("scroll", 0)),
        WINDOW_SCROLL_BY => Some(("scrollBy", 0)),
        EL_GET_BCR => Some(("getBoundingClientRect", 0)),
        EL_GET_CLIENT_RECTS => Some(("getClientRects", 0)),
        EL_SCROLL_INTO_VIEW => Some(("scrollIntoView", 0)),
        CS_GET_PROPERTY_VALUE => Some(("getPropertyValue", 1)),
        CS_GET_PROPERTY_PRIORITY => Some(("getPropertyPriority", 1)),
        CS_SET_PROPERTY => Some(("setProperty", 2)),
        CS_REMOVE_PROPERTY => Some(("removeProperty", 1)),
        CS_ITEM => Some(("item", 1)),
        _ => None,
    }
}

pub(super) fn register(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    for (prop, internal) in [
        ("getComputedStyle", GET_COMPUTED_STYLE),
        ("scrollTo", WINDOW_SCROLL_TO),
        ("scroll", WINDOW_SCROLL),
        ("scrollBy", WINDOW_SCROLL_BY),
    ] {
        let fn_obj = super::method(internal, context).map_err(|_| "Geometry method bind failed")?;
        global
            .define_property_or_throw(
                JsString::from(prop),
                PropertyDescriptor::builder()
                    .value(fn_obj)
                    .writable(true)
                    .enumerable(true)
                    .configurable(true),
                context,
            )
            .map_err(|_| "Global geometry method failed")?;
    }

    for prop in [
        "innerWidth",
        "innerHeight",
        "outerWidth",
        "outerHeight",
        "scrollX",
        "pageXOffset",
        "scrollY",
        "pageYOffset",
        "devicePixelRatio",
    ] {
        let getter = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |_, _, prop, context| {
                    modern::check_context(context)?;
                    let vp = state(context).borrow().viewport;
                    let val: f64 = match *prop {
                        "innerWidth" | "outerWidth" => vp.inner_width as f64,
                        "innerHeight" | "outerHeight" => vp.inner_height as f64,
                        "scrollX" | "pageXOffset" => vp.scroll_x as f64,
                        "scrollY" | "pageYOffset" => vp.scroll_y as f64,
                        "devicePixelRatio" => vp.device_pixel_ratio as f64,
                        _ => 0.0,
                    };
                    Ok(val.into())
                },
                prop,
            ),
        )
        .name(JsString::from(format!("get {prop}")))
        .build();
        global
            .define_property_or_throw(
                JsString::from(prop),
                PropertyDescriptor::builder()
                    .get(getter)
                    .enumerable(true)
                    .configurable(true),
                context,
            )
            .map_err(|_| "Viewport getter failed")?;
    }
    Ok(())
}

// --- Style & Layout Cache ---

fn collect_live_stylesheets(data: &State) -> Vec<StylesheetSource> {
    let mut sheets = data.host.document.stylesheets.clone();
    let base_url = if data.host.document.base_url.is_empty() {
        data.host.url.as_str().to_owned()
    } else {
        data.host.document.base_url.clone()
    };
    // Collect connected <style> elements in document order.
    let nodes = &data.host.document.nodes;
    let mut inline_sheets = Vec::new();
    let mut stack: Vec<usize> = nodes[0].children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        if nodes[id].tag == "style" {
            let css = super::text_content(nodes, id);
            if !css.trim().is_empty() {
                let media = nodes[id].attr("media").unwrap_or("").to_owned();
                inline_sheets.push(StylesheetSource {
                    css,
                    base_url: base_url.clone(),
                    media,
                });
            }
        } else if nodes[id].tag != "template" && !nodes[id].tag.starts_with('#') {
            stack.extend(nodes[id].children.iter().rev().copied());
        }
    }
    if sheets.is_empty() {
        inline_sheets
    } else {
        // Keep external stylesheets + any new inline <style> blocks not already present
        for inl in inline_sheets {
            if !sheets
                .iter()
                .any(|s| s.css == inl.css && s.media == inl.media)
                && sheets.len() < 64
            {
                sheets.push(inl);
            }
        }
        sheets
    }
}

fn ensure_layout_cached(data: &mut State) {
    let epoch = data.dom_epoch;
    let w = data.viewport.inner_width.max(1);
    let h = data.viewport.inner_height.max(1);
    if let Some(cached) = &data.cached_layout
        && cached.epoch == epoch
        && cached.width == w
        && cached.height == h
        && cached.styles.len() == data.host.document.nodes.len()
    {
        return;
    }

    let sheets = collect_live_stylesheets(data);
    let styles_vec =
        crate::style::compute_styles(&data.host.document, &sheets, (w as f32, h as f32))
            .unwrap_or_else(|_| {
                data.host
                    .document
                    .nodes
                    .iter()
                    .map(|_| ComputedStyle::default())
                    .collect()
            });
    let styles: Arc<[ComputedStyle]> = styles_vec.into();
    let mut fonts = crate::paint::Fonts::fallback_metrics();
    let vp = Viewport {
        width: w,
        height: h,
        scroll: data.viewport.scroll_y.round() as i32,
    };
    let (boxes, borders, scroll_height) =
        crate::styled_layout::compute_boxes(&data.host.document, &mut fonts, vp, styles.clone())
            .unwrap_or_else(|| (Vec::new(), Vec::new(), h as f32));

    data.cached_layout = Some(CachedLayout {
        epoch,
        width: w,
        height: h,
        styles,
        boxes,
        borders,
        scroll_height,
    });
}

fn node_rect(data: &mut State, id: usize) -> Option<(f32, f32, f32, f32)> {
    if !dom_ops::is_connected_node(&data.host.document.nodes, id) {
        return None;
    }
    ensure_layout_cached(data);
    let cached = data.cached_layout.as_ref()?;
    if cached.styles.get(id).map(|s| s.display) == Some(Display::None) {
        return None;
    }
    if let Some((_, rect)) = cached.borders.iter().rev().find(|(nid, _)| *nid == id) {
        return Some(*rect);
    }
    if let Some(b) = cached.boxes.iter().rev().find(|b| b.node == id) {
        return Some((b.x as f32, b.y as f32, b.width as f32, b.height as f32));
    }
    None
}

fn build_dom_rect(x: f64, y: f64, width: f64, height: f64, context: &mut Context) -> JsObject {
    let top = y.min(y + height);
    let bottom = y.max(y + height);
    let left = x.min(x + width);
    let right = x.max(x + width);
    let to_json = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |_, _, &(x, y, width, height, top, right, bottom, left), ctx| {
                Ok(ObjectInitializer::new(ctx)
                    .property(js_string!("x"), x, Attribute::all())
                    .property(js_string!("y"), y, Attribute::all())
                    .property(js_string!("width"), width, Attribute::all())
                    .property(js_string!("height"), height, Attribute::all())
                    .property(js_string!("top"), top, Attribute::all())
                    .property(js_string!("right"), right, Attribute::all())
                    .property(js_string!("bottom"), bottom, Attribute::all())
                    .property(js_string!("left"), left, Attribute::all())
                    .build()
                    .into())
            },
            (x, y, width, height, top, right, bottom, left),
        ),
    )
    .name(js_string!("toJSON"))
    .length(0)
    .build();

    ObjectInitializer::new(context)
        .property(
            js_string!("x"),
            x,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("y"),
            y,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("width"),
            width,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("height"),
            height,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("top"),
            top,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("right"),
            right,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("bottom"),
            bottom,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("left"),
            left,
            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("toJSON"),
            to_json,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        )
        .build()
}

// --- Element Geometry Interception (`try_get`, `try_set`, `try_has`) ---

pub(super) fn try_get(
    handle: &str,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    if !handle.starts_with("node:") {
        return Ok(None);
    }
    let id = {
        let shared = state(context);
        let data = shared.borrow();
        let id = data.host.node(handle).map_err(failure)?;
        if data.host.document.nodes[id].tag.starts_with('#') {
            return Ok(None);
        }
        id
    };

    match name {
        "getBoundingClientRect" => super::method(EL_GET_BCR, context).map(|o| Some(o.into())),
        "getClientRects" => super::method(EL_GET_CLIENT_RECTS, context).map(|o| Some(o.into())),
        "scrollIntoView" => super::method(EL_SCROLL_INTO_VIEW, context).map(|o| Some(o.into())),
        "offsetWidth" | "offsetHeight" | "offsetLeft" | "offsetTop" => {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let val = match node_rect(&mut data, id) {
                Some((x, y, w, h)) => match name {
                    "offsetWidth" => w.round() as i32,
                    "offsetHeight" => h.round() as i32,
                    "offsetLeft" => x.round() as i32,
                    "offsetTop" => y.round() as i32,
                    _ => 0,
                },
                None => 0,
            };
            Ok(Some(val.into()))
        }
        "offsetParent" => {
            let parent_id = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let tag = data.host.document.nodes[id].tag.as_str();
                if matches!(tag, "html" | "body")
                    || !dom_ops::is_connected_node(&data.host.document.nodes, id)
                {
                    None
                } else {
                    ensure_layout_cached(&mut data);
                    let cached = data.cached_layout.as_ref().unwrap();
                    if cached.styles.get(id).map(|s| s.display) == Some(Display::None)
                        || cached.styles.get(id).map(|s| s.layout.position) == Some(Position::Fixed)
                    {
                        None
                    } else {
                        let mut cur = data.host.document.nodes[id].parent;
                        let mut found = None;
                        while cur != 0 && data.host.document.nodes[cur].parent != cur {
                            if data.host.document.nodes[cur].tag == "body"
                                || cached
                                    .styles
                                    .get(cur)
                                    .is_some_and(|s| s.layout.position != Position::Static)
                            {
                                found = Some(cur);
                                break;
                            }
                            cur = data.host.document.nodes[cur].parent;
                        }
                        found
                    }
                }
            };
            match parent_id {
                Some(pid) => wrap(&format!("node:{pid}"), context).map(Some),
                None => Ok(Some(JsValue::null())),
            }
        }
        "clientWidth" | "clientHeight" | "clientLeft" | "clientTop" => {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let tag = data.host.document.nodes[id].tag.clone();
            if tag == "html" {
                let vp = data.viewport;
                let v = match name {
                    "clientWidth" => vp.inner_width as i32,
                    "clientHeight" => vp.inner_height as i32,
                    _ => 0,
                };
                return Ok(Some(v.into()));
            }
            let rect = node_rect(&mut data, id);
            let Some((_, _, w, h)) = rect else {
                return Ok(Some(0.into()));
            };
            let bw = data
                .cached_layout
                .as_ref()
                .and_then(|c| c.styles.get(id))
                .map(|s| s.border_width)
                .unwrap_or([0.0; 4]);
            let v = match name {
                "clientLeft" => bw[3].round() as i32,
                "clientTop" => bw[0].round() as i32,
                "clientWidth" => (w - bw[1] - bw[3]).max(0.0).round() as i32,
                "clientHeight" => (h - bw[0] - bw[2]).max(0.0).round() as i32,
                _ => 0,
            };
            Ok(Some(v.into()))
        }
        "scrollWidth" | "scrollHeight" => {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let tag = data.host.document.nodes[id].tag.clone();
            if matches!(tag.as_str(), "html" | "body") {
                ensure_layout_cached(&mut data);
                let vp = data.viewport;
                let sh = data
                    .cached_layout
                    .as_ref()
                    .map(|c| c.scroll_height)
                    .unwrap_or(vp.inner_height as f32);
                let v = if name == "scrollWidth" {
                    vp.inner_width as i32
                } else {
                    sh.max(vp.inner_height as f32).round() as i32
                };
                return Ok(Some(v.into()));
            }
            let v = match node_rect(&mut data, id) {
                Some((_, _, w, h)) => {
                    if name == "scrollWidth" {
                        w.round() as i32
                    } else {
                        h.round() as i32
                    }
                }
                None => 0,
            };
            Ok(Some(v.into()))
        }
        "scrollTop" | "scrollLeft" => {
            let shared = state(context);
            let data = shared.borrow();
            let tag = data.host.document.nodes[id].tag.as_str();
            if tag == "html" {
                let v = if name == "scrollTop" {
                    data.viewport.scroll_y as f64
                } else {
                    data.viewport.scroll_x as f64
                };
                return Ok(Some(v.into()));
            }
            let (sx, sy) = data
                .element_scroll_offsets
                .get(&id)
                .copied()
                .unwrap_or((0.0, 0.0));
            Ok(Some(
                (if name == "scrollTop" {
                    sy as f64
                } else {
                    sx as f64
                })
                .into(),
            ))
        }
        _ => Ok(None),
    }
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<bool> {
    if !handle.starts_with("node:") || !matches!(name, "scrollTop" | "scrollLeft") {
        return Ok(false);
    }
    let id = {
        let shared = state(context);
        let data = shared.borrow();
        let id = data.host.node(handle).map_err(failure)?;
        if data.host.document.nodes[id].tag.starts_with('#') {
            return Ok(false);
        }
        id
    };
    let num = value.to_number(context)?;
    let clamped = if num.is_finite() {
        (num as f32).max(0.0)
    } else {
        0.0
    };
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.host.document.nodes[id].tag == "html" {
        if name == "scrollTop" {
            data.viewport.scroll_y = clamped;
        } else {
            data.viewport.scroll_x = clamped;
        }
        data.scroll_request = Some(ScrollRequest::Absolute {
            x: data.viewport.scroll_x,
            y: data.viewport.scroll_y,
        });
    } else {
        let entry = data.element_scroll_offsets.entry(id).or_insert((0.0, 0.0));
        if name == "scrollTop" {
            entry.1 = clamped;
        } else {
            entry.0 = clamped;
        }
    }
    Ok(true)
}

pub(super) fn try_has(handle: &str, name: &str, context: &mut Context) -> JsResult<Option<bool>> {
    if !handle.starts_with("node:") {
        return Ok(None);
    }
    if matches!(
        name,
        "getBoundingClientRect"
            | "getClientRects"
            | "scrollIntoView"
            | "offsetWidth"
            | "offsetHeight"
            | "offsetLeft"
            | "offsetTop"
            | "offsetParent"
            | "clientWidth"
            | "clientHeight"
            | "clientLeft"
            | "clientTop"
            | "scrollWidth"
            | "scrollHeight"
            | "scrollTop"
            | "scrollLeft"
    ) {
        let shared = state(context);
        let data = shared.borrow();
        let id = data.host.node(handle).map_err(failure)?;
        if !data.host.document.nodes[id].tag.starts_with('#') {
            return Ok(Some(true));
        }
    }
    Ok(None)
}

// --- ComputedStyle Proxy (`computedstyle:{id}` & `computedstyle:pseudo:{id}`) ---

fn parse_cs_handle(handle: &str) -> Option<(usize, bool)> {
    if let Some(rest) = handle.strip_prefix("computedstyle:pseudo:") {
        rest.parse::<usize>().ok().map(|id| (id, true))
    } else if let Some(rest) = handle.strip_prefix("computedstyle:") {
        rest.parse::<usize>().ok().map(|id| (id, false))
    } else {
        None
    }
}

fn camel_to_kebab(name: &str) -> String {
    if name == "cssFloat" {
        return "float".into();
    }
    let mut out = String::with_capacity(name.len() + 4);
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn fmt_px(v: f32) -> String {
    let rounded = (v * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 1e-4 {
        format!("{}px", rounded.round() as i64)
    } else {
        format!("{rounded}px")
    }
}

fn fmt_length(len: Length, resolved_px: Option<f32>) -> String {
    if let Some(px) = resolved_px {
        return fmt_px(px);
    }
    match len {
        Length::Auto => "auto".into(),
        Length::Px(px) => fmt_px(px),
        Length::Percent(p) => {
            let pct = (p * 100.0 * 1000.0).round() / 1000.0;
            if (pct - pct.round()).abs() < 1e-4 {
                format!("{}%", pct.round() as i64)
            } else {
                format!("{pct}%")
            }
        }
        Length::Calc { px, percent, .. } => {
            if percent == 0.0 {
                fmt_px(px)
            } else {
                format!("calc({}% + {})", percent * 100.0, fmt_px(px))
            }
        }
    }
}

fn fmt_color(c: Color) -> String {
    let r = (c.rgb >> 16) & 0xff;
    let g = (c.rgb >> 8) & 0xff;
    let b = c.rgb & 0xff;
    if c.alpha <= 0.0 {
        format!("rgba({r}, {g}, {b}, 0)")
    } else if c.alpha >= 1.0 - 1e-4 {
        format!("rgb({r}, {g}, {b})")
    } else {
        let a = (c.alpha * 1000.0).round() / 1000.0;
        format!("rgba({r}, {g}, {b}, {a})")
    }
}

fn fmt_overflow(o: Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
        Overflow::Clip => "clip",
        Overflow::Auto => "auto",
        Overflow::Scroll => "scroll",
    }
}

fn fmt_align(k: AlignKeyword) -> &'static str {
    match k {
        AlignKeyword::Auto => "auto",
        AlignKeyword::Normal => "normal",
        AlignKeyword::Start => "start",
        AlignKeyword::End => "end",
        AlignKeyword::FlexStart => "flex-start",
        AlignKeyword::FlexEnd => "flex-end",
        AlignKeyword::Center => "center",
        AlignKeyword::Left => "left",
        AlignKeyword::Right => "right",
        AlignKeyword::Baseline => "baseline",
        AlignKeyword::LastBaseline => "last baseline",
        AlignKeyword::Stretch => "stretch",
        AlignKeyword::SelfStart => "self-start",
        AlignKeyword::SelfEnd => "self-end",
        AlignKeyword::SpaceBetween => "space-between",
        AlignKeyword::SpaceAround => "space-around",
        AlignKeyword::SpaceEvenly => "space-evenly",
        AlignKeyword::AnchorCenter => "anchor-center",
    }
}

fn lookup_custom_property(data: &State, mut id: usize, custom_name: &str) -> String {
    // Walk element -> ancestors for inherited CSS custom property `--*` in inline `style`
    let nodes = &data.host.document.nodes;
    for _ in 0..=crate::document::MAX_DEPTH {
        if id == 0 || id >= nodes.len() {
            break;
        }
        if let Some(inline) = nodes[id].attr("style")
            && let Ok(val) =
                crate::style::declarations::declared_value(inline, custom_name, &data.host.url)
            && !val.is_empty()
        {
            return val;
        }
        let p = nodes[id].parent;
        if p == id {
            break;
        }
        id = p;
    }
    String::new()
}

fn computed_property_value(
    data: &mut State,
    id: usize,
    is_pseudo: bool,
    prop: &str,
) -> Option<String> {
    if is_pseudo {
        return if prop == "content" {
            Some("none".into())
        } else if COMPUTED_PROP_NAMES.contains(&prop) {
            Some(String::new())
        } else {
            None
        };
    }
    if prop.starts_with("--") {
        return Some(lookup_custom_property(data, id, prop));
    }

    let rect = node_rect(data, id);
    ensure_layout_cached(data);
    let s = data
        .cached_layout
        .as_ref()
        .and_then(|c| c.styles.get(id))
        .cloned()
        .unwrap_or_default();

    let (box_w, box_h) = match rect {
        Some((_, _, w, h)) => {
            if s.layout.box_sizing == BoxSizing::BorderBox {
                (Some(w), Some(h))
            } else {
                let pad_l = s.padding[3].resolve(w).unwrap_or(0.0);
                let pad_r = s.padding[1].resolve(w).unwrap_or(0.0);
                let pad_t = s.padding[0].resolve(w).unwrap_or(0.0);
                let pad_b = s.padding[2].resolve(w).unwrap_or(0.0);
                let cw = (w - s.border_width[1] - s.border_width[3] - pad_l - pad_r).max(0.0);
                let ch = (h - s.border_width[0] - s.border_width[2] - pad_t - pad_b).max(0.0);
                (Some(cw), Some(ch))
            }
        }
        None => (None, None),
    };

    let val: String = match prop {
        "display" => match s.display {
            Display::None => "none",
            Display::Block => "block",
            Display::Inline => "inline",
            Display::InlineBlock => "inline-block",
            Display::Table => "table",
            Display::InlineTable => "inline-table",
            Display::TableRowGroup => "table-row-group",
            Display::TableHeaderGroup => "table-header-group",
            Display::TableFooterGroup => "table-footer-group",
            Display::TableRow => "table-row",
            Display::TableCell => "table-cell",
            Display::TableCaption => "table-caption",
            Display::TableColumn => "table-column",
            Display::TableColumnGroup => "table-column-group",
            Display::ListItem => "list-item",
            Display::Contents => "contents",
            Display::Flex => "flex",
            Display::InlineFlex => "inline-flex",
            Display::Grid => "grid",
            Display::InlineGrid => "inline-grid",
        }
        .into(),
        "position" => match s.layout.position {
            Position::Static => "static",
            Position::Relative => "relative",
            Position::Absolute => "absolute",
            Position::Fixed => "fixed",
            Position::Sticky => "sticky",
        }
        .into(),
        "box-sizing" => match s.layout.box_sizing {
            BoxSizing::ContentBox => "content-box",
            BoxSizing::BorderBox => "border-box",
        }
        .into(),
        "width" => fmt_length(s.width, box_w),
        "height" => fmt_length(s.height, box_h),
        "min-width" => fmt_length(s.min_width, None),
        "min-height" => fmt_length(s.min_height, None),
        "max-width" => {
            if s.max_width == Length::Auto {
                "none".into()
            } else {
                fmt_length(s.max_width, None)
            }
        }
        "max-height" => {
            if s.max_height == Length::Auto {
                "none".into()
            } else {
                fmt_length(s.max_height, None)
            }
        }
        "top" => fmt_length(s.layout.inset[0], None),
        "right" => fmt_length(s.layout.inset[1], None),
        "bottom" => fmt_length(s.layout.inset[2], None),
        "left" => fmt_length(s.layout.inset[3], None),
        "margin-top" => fmt_length(s.margin[0], s.margin[0].resolve(0.0)),
        "margin-right" => fmt_length(s.margin[1], s.margin[1].resolve(0.0)),
        "margin-bottom" => fmt_length(s.margin[2], s.margin[2].resolve(0.0)),
        "margin-left" => fmt_length(s.margin[3], s.margin[3].resolve(0.0)),
        "padding-top" => fmt_length(s.padding[0], s.padding[0].resolve(0.0)),
        "padding-right" => fmt_length(s.padding[1], s.padding[1].resolve(0.0)),
        "padding-bottom" => fmt_length(s.padding[2], s.padding[2].resolve(0.0)),
        "padding-left" => fmt_length(s.padding[3], s.padding[3].resolve(0.0)),
        "border-top-width" => fmt_px(s.border_width[0]),
        "border-right-width" => fmt_px(s.border_width[1]),
        "border-bottom-width" => fmt_px(s.border_width[2]),
        "border-left-width" => fmt_px(s.border_width[3]),
        "border-top-color" => fmt_color(s.border_color[0]),
        "border-right-color" => fmt_color(s.border_color[1]),
        "border-bottom-color" => fmt_color(s.border_color[2]),
        "border-left-color" => fmt_color(s.border_color[3]),
        "color" => fmt_color(s.color),
        "background-color" => fmt_color(s.background_color),
        "opacity" => format!("{}", (s.opacity * 1000.0).round() / 1000.0),
        "visibility" => if s.visible { "visible" } else { "hidden" }.into(),
        "overflow" => {
            let ox = fmt_overflow(s.layout.overflow[0]);
            let oy = fmt_overflow(s.layout.overflow[1]);
            if ox == oy {
                ox.into()
            } else {
                format!("{ox} {oy}")
            }
        }
        "overflow-x" => fmt_overflow(s.layout.overflow[0]).into(),
        "overflow-y" => fmt_overflow(s.layout.overflow[1]).into(),
        "z-index" => match s.layout.z_index {
            Some(z) => z.to_string(),
            None => "auto".into(),
        },
        "font-size" => fmt_px(s.font_size),
        "font-weight" => s.font_weight.to_string(),
        "font-style" => if s.italic { "italic" } else { "normal" }.into(),
        "font-family" => s.font_families.join(", "),
        "line-height" => match s.line_height {
            Some(lh) => fmt_px(lh),
            None => "normal".into(),
        },
        "text-align" => match s.text_align {
            TextAlign::Start => "start",
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
            TextAlign::Justify => "justify",
            TextAlign::End => "end",
        }
        .into(),
        "text-decoration" | "text-decoration-line" => {
            if s.underline { "underline" } else { "none" }.into()
        }
        "vertical-align" => match s.vertical_align {
            VerticalAlign::Baseline => "baseline".into(),
            VerticalAlign::Top => "top".into(),
            VerticalAlign::Middle => "middle".into(),
            VerticalAlign::Bottom => "bottom".into(),
            VerticalAlign::TextTop => "text-top".into(),
            VerticalAlign::TextBottom => "text-bottom".into(),
            VerticalAlign::Sub => "sub".into(),
            VerticalAlign::Super => "super".into(),
            VerticalAlign::Length(l) => fmt_length(l, None),
        },
        "white-space" => match s.white_space {
            WhiteSpace::Normal => "normal",
            WhiteSpace::NoWrap => "nowrap",
            WhiteSpace::Pre => "pre",
            WhiteSpace::PreWrap => "pre-wrap",
            WhiteSpace::PreLine => "pre-line",
            WhiteSpace::BreakSpaces => "break-spaces",
        }
        .into(),
        "pointer-events" => if s.pointer_events { "auto" } else { "none" }.into(),
        "flex-direction" => match s.layout.flex_direction {
            FlexDirection::Row => "row",
            FlexDirection::RowReverse => "row-reverse",
            FlexDirection::Column => "column",
            FlexDirection::ColumnReverse => "column-reverse",
        }
        .into(),
        "flex-wrap" => match s.layout.flex_wrap {
            FlexWrap::NoWrap => "nowrap",
            FlexWrap::Wrap => "wrap",
            FlexWrap::WrapReverse => "wrap-reverse",
        }
        .into(),
        "flex-grow" => format!("{}", s.layout.flex_grow),
        "flex-shrink" => format!("{}", s.layout.flex_shrink),
        "flex-basis" => match s.layout.flex_basis {
            FlexBasis::Auto => "auto".into(),
            FlexBasis::Content => "content".into(),
            FlexBasis::Length(l) => fmt_length(l, None),
        },
        "order" => s.layout.order.to_string(),
        "row-gap" => fmt_length(s.layout.gap[0], None),
        "column-gap" => fmt_length(s.layout.gap[1], None),
        "gap" => {
            let r = fmt_length(s.layout.gap[0], None);
            let c = fmt_length(s.layout.gap[1], None);
            if r == c { r } else { format!("{r} {c}") }
        }
        "align-items" => fmt_align(s.layout.align_items.keyword).into(),
        "justify-content" => fmt_align(s.layout.justify_content.keyword).into(),
        "align-self" => fmt_align(s.layout.align_self.keyword).into(),
        "justify-self" => fmt_align(s.layout.justify_self.keyword).into(),
        "align-content" => fmt_align(s.layout.align_content.keyword).into(),
        "justify-items" => fmt_align(s.layout.justify_items.keyword).into(),
        "border-collapse" => if s.border_collapse {
            "collapse"
        } else {
            "separate"
        }
        .into(),
        "float" => "none".into(),
        _ => {
            // Check Stylo supported property table; if known CSS property, check inline style or return ""
            if crate::style::declarations::enabled_non_custom_css_name(prop)
                .ok()
                .flatten()
                .is_some()
            {
                if id < data.host.document.nodes.len()
                    && let Some(inline) = data.host.document.nodes[id].attr("style")
                    && let Ok(v) =
                        crate::style::declarations::declared_value(inline, prop, &data.host.url)
                    && !v.is_empty()
                {
                    return Some(v);
                }
                return Some(String::new());
            }
            return None;
        }
    };
    Some(val)
}

pub(super) fn computedstyle_get(
    handle: &str,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let Some((id, is_pseudo)) = parse_cs_handle(handle) else {
        return Err(failure("Invalid computed style handle"));
    };
    let property = key(&args[1], context)?;
    let Some(name) = property_name(&property) else {
        if property == modern::boa_engine::property::PropertyKey::Symbol(JsSymbol::iterator()) {
            return context
                .intrinsics()
                .constructors()
                .array()
                .prototype()
                .get(JsSymbol::iterator(), context);
        }
        return super::fallback_get(args, context);
    };
    match name.as_str() {
        "length" => Ok((if is_pseudo {
            0
        } else {
            COMPUTED_PROP_NAMES.len()
        } as f64)
            .into()),
        "cssText" => Ok(js_string!("").into()),
        "parentRule" => Ok(JsValue::null()),
        "getPropertyValue" => super::method(CS_GET_PROPERTY_VALUE, context).map(Into::into),
        "getPropertyPriority" => super::method(CS_GET_PROPERTY_PRIORITY, context).map(Into::into),
        "setProperty" => super::method(CS_SET_PROPERTY, context).map(Into::into),
        "removeProperty" => super::method(CS_REMOVE_PROPERTY, context).map(Into::into),
        "item" => super::method(CS_ITEM, context).map(Into::into),
        _ => {
            if let Ok(idx) = name.parse::<usize>() {
                if !is_pseudo && let Some(&pname) = COMPUTED_PROP_NAMES.get(idx) {
                    return Ok(JsString::from(pname).into());
                }
                return Ok(JsValue::undefined());
            }
            let kebab = if name.starts_with("--") {
                return super::fallback_get(args, context);
            } else if name.contains('-') {
                name.to_ascii_lowercase()
            } else {
                camel_to_kebab(&name)
            };
            let val = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                computed_property_value(&mut data, id, is_pseudo, &kebab)
            };
            if let Some(v) = val {
                Ok(JsString::from(v).into())
            } else {
                super::fallback_get(args, context)
            }
        }
    }
}

pub(super) fn computedstyle_set(
    _: &str,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    Err(dom_exception::throw(
        "NoModificationAllowedError",
        "Computed style declarations are read-only.",
        context,
    ))
}

pub(super) fn computedstyle_has(
    handle: &str,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let Some((_, is_pseudo)) = parse_cs_handle(handle) else {
        return Ok(false.into());
    };
    let property = key(&args[1], context)?;
    if let Some(name) = property_name(&property) {
        if matches!(
            name.as_str(),
            "length"
                | "cssText"
                | "parentRule"
                | "getPropertyValue"
                | "getPropertyPriority"
                | "setProperty"
                | "removeProperty"
                | "item"
        ) {
            return Ok(true.into());
        }
        if let Ok(idx) = name.parse::<usize>() {
            return Ok((!is_pseudo && idx < COMPUTED_PROP_NAMES.len()).into());
        }
        if !name.starts_with("--") {
            let kebab = if name.contains('-') {
                name.to_ascii_lowercase()
            } else {
                camel_to_kebab(&name)
            };
            if COMPUTED_PROP_NAMES.contains(&kebab.as_str())
                || crate::style::declarations::enabled_non_custom_css_name(&kebab)
                    .ok()
                    .flatten()
                    .is_some()
            {
                return Ok(true.into());
            }
        }
    }
    args[0]
        .as_object()
        .unwrap()
        .has_property(property, context)
        .map(Into::into)
}

pub(super) fn computedstyle_own_keys(
    handle: &str,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let is_pseudo = parse_cs_handle(handle).map(|(_, p)| p).unwrap_or(true);
    let mut keys: Vec<JsValue> = Vec::new();
    if !is_pseudo {
        for i in 0..COMPUTED_PROP_NAMES.len() {
            keys.push(JsString::from(i.to_string()).into());
        }
    }
    Ok(JsArray::from_iter(keys, context).into())
}

pub(super) fn computedstyle_own_descriptor(
    handle: &str,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let is_pseudo = parse_cs_handle(handle).map(|(_, p)| p).unwrap_or(true);
    let property = key(&args[1], context)?;
    if !is_pseudo
        && let Some(name) = property_name(&property)
        && let Ok(idx) = name.parse::<usize>()
        && let Some(&pname) = COMPUTED_PROP_NAMES.get(idx)
    {
        let desc = ObjectInitializer::new(context)
            .property(
                js_string!("value"),
                JsString::from(pname),
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("writable"),
                false,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("enumerable"),
                true,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("configurable"),
                true,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .build();
        return Ok(desc.into());
    }
    Ok(JsValue::undefined())
}

// --- Method Calls ---

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match name {
        GET_COMPUTED_STYLE => {
            let Some(el_arg) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("getComputedStyle requires 1 argument")
                    .into());
            };
            let Value::Host(handle) = receiver(el_arg, context)? else {
                return Err(JsNativeError::typ()
                    .with_message("Argument 1 of getComputedStyle is not an Element")
                    .into());
            };
            let id = {
                let shared = state(context);
                let data = shared.borrow();
                let id = data.host.node(&handle).map_err(|_| {
                    JsNativeError::typ()
                        .with_message("Argument 1 of getComputedStyle is not an Element")
                })?;
                if data.host.document.nodes[id].tag.starts_with('#') {
                    return Err(JsNativeError::typ()
                        .with_message("Argument 1 of getComputedStyle is not an Element")
                        .into());
                }
                id
            };
            let is_pseudo = match args.get(1) {
                None => false,
                Some(v) if v.is_undefined() || v.is_null() => false,
                Some(v) => {
                    let s = utf8(v, context)?;
                    !s.trim().is_empty()
                }
            };
            let cs_handle = if is_pseudo {
                format!("computedstyle:pseudo:{id}")
            } else {
                format!("computedstyle:{id}")
            };
            wrap(&cs_handle, context)
        }
        CS_GET_PROPERTY_VALUE => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            let Some((id, is_pseudo)) = parse_cs_handle(&handle) else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            let Some(first) = args.first() else {
                return Err(failure("getPropertyValue requires 1 argument"));
            };
            let raw = utf8(first, context)?;
            let prop_name = if raw.starts_with("--") {
                raw
            } else {
                raw.to_ascii_lowercase()
            };
            let val = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                computed_property_value(&mut data, id, is_pseudo, &prop_name).unwrap_or_default()
            };
            Ok(JsString::from(val).into())
        }
        CS_GET_PROPERTY_PRIORITY => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            if parse_cs_handle(&handle).is_none() {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            let Some(first) = args.first() else {
                return Err(failure("getPropertyPriority requires 1 argument"));
            };
            let _ = utf8(first, context)?;
            Ok(js_string!("").into())
        }
        CS_SET_PROPERTY | CS_REMOVE_PROPERTY => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            if parse_cs_handle(&handle).is_none() {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            Err(dom_exception::throw(
                "NoModificationAllowedError",
                "Computed style declarations are read-only.",
                context,
            ))
        }
        CS_ITEM => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            let Some((_, is_pseudo)) = parse_cs_handle(&handle) else {
                return Err(failure("Invalid CSSStyleDeclaration receiver"));
            };
            let idx = args
                .first()
                .unwrap_or(&JsValue::undefined())
                .to_u32(context)? as usize;
            if !is_pseudo && let Some(&pname) = COMPUTED_PROP_NAMES.get(idx) {
                Ok(JsString::from(pname).into())
            } else {
                Ok(js_string!("").into())
            }
        }
        EL_GET_BCR => {
            let id = element_id_receiver(this, context)?;
            let (x, y, w, h) = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                match node_rect(&mut data, id) {
                    Some((rx, ry, rw, rh)) => {
                        let is_fixed = data
                            .cached_layout
                            .as_ref()
                            .and_then(|c| c.styles.get(id))
                            .is_some_and(|s| s.layout.position == Position::Fixed);
                        let (sx, sy) = if is_fixed {
                            (0.0, 0.0)
                        } else {
                            (data.viewport.scroll_x, data.viewport.scroll_y)
                        };
                        ((rx - sx) as f64, (ry - sy) as f64, rw as f64, rh as f64)
                    }
                    None => (0.0, 0.0, 0.0, 0.0),
                }
            };
            Ok(build_dom_rect(x, y, w, h, context).into())
        }
        EL_GET_CLIENT_RECTS => {
            let id = element_id_receiver(this, context)?;
            let rects: Vec<(f64, f64, f64, f64)> = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if !dom_ops::is_connected_node(&data.host.document.nodes, id) {
                    Vec::new()
                } else {
                    ensure_layout_cached(&mut data);
                    let cached = data.cached_layout.as_ref().unwrap();
                    if cached.styles.get(id).map(|s| s.display) == Some(Display::None) {
                        Vec::new()
                    } else {
                        let is_fixed = cached
                            .styles
                            .get(id)
                            .is_some_and(|s| s.layout.position == Position::Fixed);
                        let (sx, sy) = if is_fixed {
                            (0.0, 0.0)
                        } else {
                            (data.viewport.scroll_x, data.viewport.scroll_y)
                        };
                        cached
                            .boxes
                            .iter()
                            .filter(|b| b.node == id)
                            .map(|b| {
                                (
                                    (b.x as f32 - sx) as f64,
                                    (b.y as f32 - sy) as f64,
                                    b.width as f64,
                                    b.height as f64,
                                )
                            })
                            .collect()
                    }
                }
            };
            let js_rects: Vec<JsValue> = rects
                .into_iter()
                .map(|(x, y, w, h)| build_dom_rect(x, y, w, h, context).into())
                .collect();
            let arr = JsArray::from_iter(js_rects, context);
            let item_fn = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_fn_ptr(|this, args, ctx| {
                    let obj = this
                        .as_object()
                        .ok_or_else(|| failure("Invalid DOMRectList receiver"))?;
                    let idx = args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
                    let v = obj.get(idx, ctx)?;
                    Ok(if v.is_undefined() { JsValue::null() } else { v })
                }),
            )
            .name(js_string!("item"))
            .length(1)
            .build();
            arr.define_property_or_throw(
                js_string!("item"),
                PropertyDescriptor::builder()
                    .value(item_fn)
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )?;
            Ok(arr.into())
        }
        EL_SCROLL_INTO_VIEW => {
            let id = element_id_receiver(this, context)?;
            let mut align_top = true;
            if let Some(arg) = args.first()
                && !arg.is_undefined()
            {
                if let Some(b) = arg.as_boolean() {
                    align_top = b;
                } else if let Some(obj) = arg.as_object() {
                    let block = obj.get(js_string!("block"), context)?;
                    if !block.is_undefined() {
                        let s = utf8(&block, context)?;
                        if s == "end" {
                            align_top = false;
                        }
                    }
                } else {
                    align_top = arg.to_boolean();
                }
            }
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if let Some((_, y, _, h)) = node_rect(&mut data, id) {
                let target_y = if align_top {
                    y.max(0.0)
                } else {
                    (y + h - data.viewport.inner_height as f32).max(0.0)
                };
                data.viewport.scroll_y = target_y;
            }
            data.scroll_request = Some(ScrollRequest::IntoView {
                node: id,
                align_top,
            });
            Ok(JsValue::undefined())
        }
        WINDOW_SCROLL_TO | WINDOW_SCROLL | WINDOW_SCROLL_BY => {
            let is_by = name == WINDOW_SCROLL_BY;
            let (opt_x, opt_y) = parse_scroll_args(args, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if is_by {
                let dx = opt_x.unwrap_or(0.0);
                let dy = opt_y.unwrap_or(0.0);
                data.viewport.scroll_x = (data.viewport.scroll_x + dx).max(0.0);
                data.viewport.scroll_y = (data.viewport.scroll_y + dy).max(0.0);
                data.scroll_request = Some(ScrollRequest::By { dx, dy });
            } else {
                let nx = opt_x.unwrap_or(data.viewport.scroll_x).max(0.0);
                let ny = opt_y.unwrap_or(data.viewport.scroll_y).max(0.0);
                data.viewport.scroll_x = nx;
                data.viewport.scroll_y = ny;
                data.scroll_request = Some(ScrollRequest::Absolute { x: nx, y: ny });
            }
            Ok(JsValue::undefined())
        }
        _ => Err(failure("Unknown geometry/style method")),
    }
}

fn element_id_receiver(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure("Element method requires an Element receiver"));
    };
    let shared = state(context);
    let data = shared.borrow();
    let id = data.host.node(&handle).map_err(failure)?;
    if data.host.document.nodes[id].tag.starts_with('#') {
        return Err(failure("Element method requires an Element receiver"));
    }
    Ok(id)
}

fn parse_scroll_args(
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<(Option<f32>, Option<f32>)> {
    let Some(first) = args.first() else {
        return Ok((Some(0.0), Some(0.0)));
    };
    if args.len() == 1 {
        if first.is_undefined() || first.is_null() {
            return Ok((None, None));
        }
        if let Some(obj) = first.as_object() {
            let left_val = obj.get(js_string!("left"), context)?;
            let top_val = obj.get(js_string!("top"), context)?;
            let lx = if left_val.is_undefined() {
                None
            } else {
                let n = left_val.to_number(context)?;
                Some(if n.is_finite() { n as f32 } else { 0.0 })
            };
            let ty = if top_val.is_undefined() {
                None
            } else {
                let n = top_val.to_number(context)?;
                Some(if n.is_finite() { n as f32 } else { 0.0 })
            };
            return Ok((lx, ty));
        }
    }
    let nx = first.to_number(context)?;
    let ny = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?;
    Ok((
        Some(if nx.is_finite() { nx as f32 } else { 0.0 }),
        Some(if ny.is_finite() { ny as f32 } else { 0.0 }),
    ))
}
