//! Integration tests for Milestone 5 CSS Visuals, Grid/Intrinsic Layout, and Clipping/Transforms:
//! 1. Named grid lines, `fit-content(...)` tracks, and `repeat(auto-fill | auto-fit, ...)`
//! 2. Intrinsic box sizing (`min-content`, `max-content`, `fit-content`) and `<canvas>` 300x150 sizing
//! 3. `box-shadow` (outer, inset, blur, spread, stacking) and `text-shadow`
//! 4. Multi-stop linear, repeating-linear, radial, and repeating-radial gradients
//! 5. 2D `transform` (`translate` / `scale`), `clip-path: inset(...)` / `clip: rect(...)`, and `text-overflow: ellipsis`

use mg_sparkle::{
    document::{Document, parse},
    paint::Fonts,
    render::{Controls, Frame, LayoutBox, Viewport, render_scaled},
};

fn fonts() -> Fonts {
    Fonts::from_bytes(
        std::fs::read(
            std::env::var("MGBROWSER_FONT")
                .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
        )
        .unwrap(),
    )
    .unwrap()
}

fn render_html(body: &str, width: u32, height: u32) -> (Document, Frame) {
    let doc = parse(
        &format!(
            "<!doctype html><html><head><style>\
             html,body{{margin:0;padding:0;background:#ffffff}}\
             *{{margin:0;padding:0;border:0}}\
             </style></head><body>{body}</body></html>"
        ),
        "https://visual.example/",
    );
    let mut f = fonts();
    let frame = render_scaled(
        &doc,
        &mut f,
        Viewport {
            width,
            height,
            scroll: 0,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        frame.diagnostics.entries.is_empty(),
        "unexpected diagnostics: {:?}",
        frame.diagnostics.entries
    );
    (doc, frame)
}

fn box_of<'a>(doc: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let node = doc.query_selector(0, selector).unwrap().unwrap();
    frame
        .boxes
        .iter()
        .rev()
        .find(|b| b.node == node)
        .unwrap_or_else(|| panic!("missing LayoutBox for {selector}"))
}

fn pixel(frame: &Frame, x: u32, y: u32) -> u32 {
    frame.canvas.pixels[(y * frame.canvas.width + x) as usize]
}

#[test]
fn grid_named_lines_span_and_template_area_implicit_lines_resolve_positions() {
    let (doc, frame) = render_html(
        "<div style='display:grid;width:400px;\
            grid-template-columns:[outer-start] 60px [main-start col] 1fr [col] 80px [main-end outer-end];\
            grid-template-rows:[top] 30px [mid] 50px [bot]'>\
            <div id=hdr style='grid-column:outer-start / outer-end;grid-row:top / mid'></div>\
            <div id=body style='grid-column:main-start / main-end;grid-row:mid / bot'></div>\
            <div id=spanned style='grid-column:col 1 / span 1 col;height:20px'></div>\
         </div>\
         <div style='display:grid;width:300px;grid-template-columns:50px 1fr 70px;grid-template-areas:\"nav hero side\"'>\
            <div id=implicit_hero style='grid-column:hero-start / hero-end;height:25px'></div>\
         </div>",
        420,
        240,
    );
    let hdr = box_of(&doc, &frame, "#hdr");
    let body = box_of(&doc, &frame, "#body");
    let spanned = box_of(&doc, &frame, "#spanned");
    let implicit_hero = box_of(&doc, &frame, "#implicit_hero");

    assert_eq!((hdr.x, hdr.y, hdr.width, hdr.height), (0, 0, 400, 30));
    // 400 - 60 - 80 = 260px for the 1fr track, so main-start..main-end = 260 + 80 = 340px at x=60.
    assert_eq!((body.x, body.y, body.width, body.height), (60, 30, 340, 50));
    // First `col` line is at x=60; `span 1 col` reaches second `col` line at x=320 => width 260.
    assert_eq!((spanned.x, spanned.width), (60, 260));
    // Implicit `hero-start` / `hero-end` from `grid-template-areas`: x=50, width = 300 - 50 - 70 = 180.
    assert_eq!(
        (implicit_hero.x, implicit_hero.width, implicit_hero.height),
        (50, 180, 25)
    );
}

#[test]
fn grid_fit_content_clamps_wide_item_and_preserves_narrow_item() {
    let (doc, frame) = render_html(
        "<div style='display:grid;width:300px;grid-template-columns:fit-content(120px) 1fr'>\
            <div id=narrow><span style='display:inline-block;width:60px;height:20px'></span></div>\
            <div id=narrow_rest style='height:20px'></div>\
         </div>\
         <div style='display:grid;width:300px;grid-template-columns:fit-content(120px) 1fr'>\
            <div id=wide><span style='display:inline-block;width:90px;height:20px'></span> <span style='display:inline-block;width:90px;height:20px'></span></div>\
            <div id=wide_rest style='height:20px'></div>\
         </div>",
        320,
        200,
    );
    assert_eq!(box_of(&doc, &frame, "#narrow").width, 60);
    assert_eq!(box_of(&doc, &frame, "#narrow_rest").width, 240);
    assert_eq!(box_of(&doc, &frame, "#wide").width, 120);
    assert_eq!(box_of(&doc, &frame, "#wide_rest").width, 180);
}

#[test]
fn grid_auto_fill_retains_empty_tracks_while_auto_fit_collapses_them() {
    let (doc, frame) = render_html(
        "<div style='display:grid;width:400px;grid-template-columns:repeat(auto-fill,minmax(100px,1fr))'>\
            <div id=fill1 style='height:20px'></div>\
            <div id=fill2 style='height:20px'></div>\
         </div>\
         <div style='display:grid;width:400px;grid-template-columns:repeat(auto-fit,minmax(100px,1fr))'>\
            <div id=fit1 style='height:20px'></div>\
            <div id=fit2 style='height:20px'></div>\
         </div>",
        420,
        160,
    );
    let fill1 = box_of(&doc, &frame, "#fill1");
    let fill2 = box_of(&doc, &frame, "#fill2");
    assert_eq!((fill1.x, fill1.width), (0, 100));
    assert_eq!((fill2.x, fill2.width), (100, 100));

    let fit1 = box_of(&doc, &frame, "#fit1");
    let fit2 = box_of(&doc, &frame, "#fit2");
    assert_eq!((fit1.x, fit1.width), (0, 200));
    assert_eq!((fit2.x, fit2.width), (200, 200));
}

#[test]
fn intrinsic_sizing_keywords_apply_to_width_height_min_max_and_flex_basis() {
    let (doc, frame) = render_html(
        "<div id=minw style='width:min-content;line-height:20px'>\
            <span style='display:inline-block;width:70px;height:20px'></span> \
            <span style='display:inline-block;width:50px;height:20px'></span>\
         </div>\
         <div style='width:300px'>\
            <div id=maxw style='max-width:max-content'>\
                <div style='width:110px;height:18px'></div>\
            </div>\
         </div>\
         <div id=minh style='height:10px;min-height:max-content'>\
            <div style='width:40px;height:46px'></div>\
         </div>\
         <div style='display:flex;width:300px'>\
            <div id=fb_min style='flex:0 0 min-content;line-height:20px'>\
                <span style='display:inline-block;width:45px;height:20px'></span> \
                <span style='display:inline-block;width:35px;height:20px'></span>\
            </div>\
            <div id=fb_max style='flex:0 0 max-content'>\
                <div style='width:85px;height:20px'></div>\
            </div>\
         </div>",
        340,
        260,
    );
    let minw = box_of(&doc, &frame, "#minw");
    assert_eq!(minw.width, 70);
    assert!(minw.height >= 40, "two wrapped lines expected: {minw:?}");

    assert_eq!(box_of(&doc, &frame, "#maxw").width, 110);
    assert_eq!(box_of(&doc, &frame, "#minh").height, 46);
    assert_eq!(box_of(&doc, &frame, "#fb_min").width, 45);
    assert_eq!(box_of(&doc, &frame, "#fb_max").width, 85);
}

#[test]
fn canvas_element_defaults_to_300x150_and_honors_attributes_and_aspect_ratio() {
    let (doc, frame) = render_html(
        "<canvas id=c_default style='display:block'></canvas>\
         <canvas id=c_attrs width=240 height=120 style='display:block'></canvas>\
         <canvas id=c_scaled width=200 height=100 style='display:block;width:80px'></canvas>\
         <canvas id=c_invalid width='-10' height='bogus' style='display:block'></canvas>",
        450,
        550,
    );
    let c_default = box_of(&doc, &frame, "#c_default");
    let c_attrs = box_of(&doc, &frame, "#c_attrs");
    let c_scaled = box_of(&doc, &frame, "#c_scaled");
    let c_invalid = box_of(&doc, &frame, "#c_invalid");

    assert_eq!((c_default.width, c_default.height), (300, 150));
    assert_eq!((c_attrs.width, c_attrs.height), (240, 120));
    assert_eq!((c_scaled.width, c_scaled.height), (80, 40));
    assert_eq!((c_invalid.width, c_invalid.height), (300, 150));
}

#[test]
fn box_shadow_renders_outer_masked_blur_inset_clip_and_list_order() {
    let (doc, frame) = render_html(
        "<div id=b1 style='margin:20px;width:40px;height:30px;box-shadow:10px 8px 0px 2px #ff0000'></div>\
         <div id=b2 style='margin:20px;width:40px;height:30px;border:4px solid #00aa00;box-shadow:inset 8px 8px 0px 0px #0000ff, 0px 0px 0px 4px #ff00ff'></div>",
        140,
        180,
    );
    let b1 = box_of(&doc, &frame, "#b1");
    let b2 = box_of(&doc, &frame, "#b2");

    // First box has transparent background: interior must remain white (outer box-shadow masked inside border-box!).
    assert_eq!(
        pixel(&frame, (b1.x + 15) as u32, (b1.y + 15) as u32),
        0xffffff
    );
    // Outer shadow shifted by (+10, +8) with +2px spread paints red outside the right edge:
    assert_eq!(
        pixel(
            &frame,
            (b1.x + b1.width as i32 + 5) as u32,
            (b1.y + 15) as u32
        ),
        0xff0000
    );

    // Second box has 4px green border and both an inset blue shadow and an outer 4px magenta spread ring:
    assert_eq!(
        pixel(&frame, (b2.x - 2) as u32, (b2.y + 10) as u32),
        0xff00ff
    );
    assert_eq!(
        pixel(&frame, (b2.x + 1) as u32, (b2.y + 10) as u32),
        0x00aa00
    );
    assert_eq!(
        pixel(&frame, (b2.x + 6) as u32, (b2.y + 6) as u32),
        0x0000ff
    );
    assert_eq!(
        pixel(&frame, (b2.x + 35) as u32, (b2.y + 25) as u32),
        0xffffff
    );
}

#[test]
fn text_shadow_renders_sharp_and_blurred_shadows_under_glyphs() {
    let (_, frame) = render_html(
        "<div style='padding:10px;font-size:20px;line-height:24px;color:#000000;text-shadow:30px 0px 0px #ff0000'>H</div>\
         <div style='padding:10px;font-size:20px;line-height:24px;color:#000000;text-shadow:0px 14px 4px #0000ff'>H</div>",
        140,
        120,
    );
    assert!(
        frame.canvas.pixels[..140 * 48].contains(&0xff0000),
        "expected sharp red text-shadow pixels in upper row"
    );
    let blue_tinted = frame.canvas.pixels[140 * 48..]
        .iter()
        .filter(|&&px| {
            let r = (px >> 16) & 0xff;
            let b = px & 0xff;
            b > r && px != 0xffffff && px != 0x000000
        })
        .count();
    assert!(
        blue_tinted > 10,
        "expected blurred blue text-shadow halo pixels, got {blue_tinted}"
    );
}

#[test]
fn gradients_render_multi_stop_midpoint_hint_repeating_and_radial_shapes() {
    let (_, frame) = render_html(
        "<div style='width:100px;height:10px;background:linear-gradient(90deg,#ff0000 0%,10%,#0000ff 100%)'></div>\
         <div style='width:40px;height:40px;background:repeating-radial-gradient(circle at 20px 20px,#ff0000 0px,#ff0000 5px,#00ff00 5px,#00ff00 10px)'></div>",
        120,
        70,
    );
    // With a 10% midpoint hint between red (0%) and blue (100%), at x=50 (50%) blue channel strongly dominates red.
    let mid_px = pixel(&frame, 50, 5);
    let mid_r = (mid_px >> 16) & 0xff;
    let mid_b = mid_px & 0xff;
    assert!(
        mid_b > mid_r + 120,
        "10% color hint should shift midpoint toward blue, got #{mid_px:06x}"
    );

    // Repeating radial rings centered at (20, 10 + 20) = (20, 30):
    // r = 2px -> red, r = 7px -> green, r = 12px -> red, r = 17px -> green.
    assert_eq!(pixel(&frame, 22, 30), 0xff0000);
    assert_eq!(pixel(&frame, 27, 30), 0x00ff00);
    assert_eq!(pixel(&frame, 32, 30), 0xff0000);
    assert_eq!(pixel(&frame, 37, 30), 0x00ff00);
}

#[test]
fn transforms_translate_scale_center_establish_fixed_cb_and_stacking_context() {
    let (doc, frame) = render_html(
        "<div style='position:relative;width:200px;height:120px'>\
            <div id=centered style='position:absolute;left:50%;top:50%;width:80px;height:40px;background:#ff0000;transform:translate(-50%,-50%)'>\
                <div id=fixed_child style='position:fixed;left:4px;top:6px;width:20px;height:10px;background:#00ff00'></div>\
            </div>\
         </div>\
         <div id=scaled style='margin-left:20px;margin-top:10px;width:40px;height:20px;background:#0000ff;transform:scale(2)'></div>",
        260,
        220,
    );
    let centered = box_of(&doc, &frame, "#centered");
    assert_eq!(
        (centered.x, centered.y, centered.width, centered.height),
        (60, 40, 80, 40)
    );
    assert_eq!(pixel(&frame, 100, 60), 0xff0000);

    // `position: fixed` inside a transformed ancestor uses `#centered` as its containing block!
    let fixed_child = box_of(&doc, &frame, "#fixed_child");
    assert_eq!(
        (
            fixed_child.x,
            fixed_child.y,
            fixed_child.width,
            fixed_child.height
        ),
        (64, 46, 20, 10)
    );
    assert_eq!(pixel(&frame, 70, 50), 0x00ff00);

    // `#scaled` untransformed rect is (20, 130, 40, 20) with center (40, 140).
    // `scale(2)` around center produces (0, 120, 80, 40).
    let scaled = box_of(&doc, &frame, "#scaled");
    assert_eq!(
        (scaled.x, scaled.y, scaled.width, scaled.height),
        (0, 120, 80, 40)
    );
    assert_eq!(pixel(&frame, 5, 125), 0x0000ff);
    assert_eq!(pixel(&frame, 75, 155), 0x0000ff);
}

#[test]
fn visually_hidden_clip_rect_and_clip_path_inset_collapse_and_round_clip() {
    let (doc, frame) = render_html(
        "<div style='position:relative;width:120px;height:80px'>\
            <div id=legacy_sr style='position:absolute;left:0;top:0;width:80px;height:40px;background:#ff0000;clip:rect(0,0,0,0)'>Hidden</div>\
            <div id=modern_sr style='width:80px;height:40px;background:#ff0000;clip-path:inset(50%)'>Hidden</div>\
            <div id=inset_box style='width:60px;height:40px;background:#0000ff;clip-path:inset(10px)'></div>\
         </div>",
        160,
        140,
    );
    let legacy_sr = box_of(&doc, &frame, "#legacy_sr");
    let modern_sr = box_of(&doc, &frame, "#modern_sr");
    assert_eq!((legacy_sr.width, legacy_sr.height), (0, 0));
    assert_eq!((modern_sr.width, modern_sr.height), (0, 0));
    assert!(
        !frame.canvas.pixels.contains(&0xff0000),
        "clipped screen-reader-only boxes must paint zero red pixels"
    );

    // `#inset_box` is at y=40..80, x=0..60; `clip-path: inset(10px)` clips it to x=10..50, y=50..70.
    let inset_box = box_of(&doc, &frame, "#inset_box");
    assert_eq!(
        (inset_box.x, inset_box.y, inset_box.width, inset_box.height),
        (10, 50, 40, 20)
    );
    assert_eq!(pixel(&frame, 5, 60), 0xffffff);
    assert_eq!(pixel(&frame, 20, 60), 0x0000ff);
    assert_eq!(pixel(&frame, 55, 60), 0xffffff);
}

#[test]
fn text_overflow_ellipsis_truncates_single_line_hidden_overflow_with_ellipsis_glyph() {
    let (_, frame) = render_html(
        "<div id=ell style='width:90px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-size:14px;line-height:20px;color:#112233'>\
            Supercalifragilisticexpialidocious long single line text\
         </div>",
        240,
        40,
    );
    let joined: String = frame.text_runs.iter().map(|r| r.text.as_str()).collect();
    assert!(
        joined.ends_with('…'),
        "expected truncated text runs to end with ellipsis, got {joined:?}"
    );
    for run in &frame.text_runs {
        assert!(
            run.x + run.width as i32 <= 92,
            "ellipsized text run overflowed 90px container: {run:?}"
        );
    }
    // Pixels well to the right of the 90px box must remain untouched white.
    assert_eq!(pixel(&frame, 120, 10), 0xffffff);
}
