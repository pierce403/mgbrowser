//! End-to-end Milestone 3 CSS & Layout fidelity regressions.
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

fn render_doc(html: &str, width: u32, height: u32, scroll: i32) -> (Document, Frame) {
    let doc = parse(
        &format!(
            "<!doctype html><html><head><style>html,body{{margin:0;padding:0;background:#ffffff}}*{{margin:0;padding:0;border:0;font-size:12px;line-height:16px}}</style></head><body>{html}</body></html>"
        ),
        "https://fixture.example/",
    );
    let f = render_scaled(
        &doc,
        &mut fonts(),
        Viewport {
            width,
            height,
            scroll,
        },
        &Controls::default(),
        1.0,
    );
    assert!(
        f.diagnostics.entries.is_empty(),
        "unexpected diagnostics: {:?}",
        f.diagnostics
    );
    (doc, f)
}

fn px(frame: &Frame, x: u32, y: u32) -> u32 {
    frame.canvas.pixels[(y * frame.canvas.width + x) as usize]
}

fn box_of<'a>(doc: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
    let id = doc.query_selector(0, selector).unwrap().unwrap();
    frame.boxes.iter().rev().find(|b| b.node == id).unwrap()
}

#[test]
fn border_radius_overlap_scaling_no_bleed_and_rounded_overflow_clipping() {
    // Red background under thick opaque green rounded border: zero red bleed outside border!
    let (_, f) = render_doc(
        "<div style='width:80px;height:40px;border-radius:999px;background:#ff0000;border:10px solid #00aa00;box-sizing:border-box'></div>",
        100,
        60,
        0,
    );
    // Exterior corner (0, 0) is untouched white.
    assert_eq!(px(&f, 0, 0), 0xffffff);
    // Interior center (40, 20) is solid red background.
    assert_eq!(px(&f, 40, 20), 0xff0000);
    // Top border midpoint (40, 5) is solid green border.
    assert_eq!(px(&f, 40, 5), 0x00aa00);
    // No outer pixel may contain red channel where green == 0 and blue == 0 outside padding box:
    // specifically along top/bottom outer rows y=0..3, any non-white AA pixel must blend green+white (R == B).
    for x in 0..80 {
        let c = px(&f, x, 1);
        let r = (c >> 16) & 255;
        let b = c & 255;
        assert_eq!(r, b, "red bled through green border at ({x}, 1): #{c:06x}");
    }

    // Rounded overflow:hidden clips overflowing child to rounded padding box.
    let (_, clipped) = render_doc(
        "<div style='width:60px;height:60px;border-radius:30px;overflow:hidden'><div style='width:60px;height:60px;background:#0000ff'></div></div>",
        80,
        80,
        0,
    );
    assert_eq!(
        px(&clipped, 2, 2),
        0xffffff,
        "child clipped at rounded corner"
    );
    assert_eq!(px(&clipped, 30, 30), 0x0000ff, "child visible at center");
}

#[test]
fn appendix_e_stacking_contexts_z_index_auto_vs_zero_and_opacity_isolation() {
    // 1. Static non-flex z-index is ignored; flex item static z-index creates context.
    let (_, static_non_flex) = render_doc(
        "<div style='width:40px;height:40px;background:#ff0000;z-index:99'></div><div style='width:40px;height:40px;margin-top:-40px;background:#00aa00'></div>",
        60,
        60,
        0,
    );
    assert_eq!(px(&static_non_flex, 20, 20), 0x00aa00);

    let (_, flex_item_z) = render_doc(
        "<div style='display:flex'><div style='width:40px;height:40px;background:#00aa00;z-index:2'></div><div style='width:40px;height:40px;margin-left:-40px;background:#ff0000;z-index:1'></div></div>",
        60,
        60,
        0,
    );
    assert_eq!(px(&flex_item_z, 20, 20), 0x00aa00);

    // 2. z-index: auto does NOT trap child z-index: 10, whereas z-index: 0 DOES trap child z-index: 10.
    let (_, auto_escapes) = render_doc(
        "<div style='position:relative;z-index:auto'><div style='position:absolute;z-index:10;width:40px;height:40px;background:#00aa00'></div></div><div style='position:relative;z-index:5;width:40px;height:40px;background:#ff0000'></div>",
        60,
        60,
        0,
    );
    assert_eq!(px(&auto_escapes, 20, 20), 0x00aa00);

    let (_, zero_traps) = render_doc(
        "<div style='position:relative;z-index:0'><div style='position:absolute;z-index:10;width:40px;height:40px;background:#ff0000'></div></div><div style='position:relative;z-index:5;width:40px;height:40px;background:#00aa00'></div>",
        60,
        60,
        0,
    );
    assert_eq!(px(&zero_traps, 20, 20), 0x00aa00);

    // 3. opacity < 1 creates stacking context so negative z-index child stays above ancestor sibling's background
    // and above own stacking context background!
    let (_, opacity_ctx) = render_doc(
        "<div style='width:40px;height:40px;background:#ff0000'></div><div style='margin-top:-40px;width:40px;height:40px;opacity:0.99;position:static'><div style='position:relative;z-index:-1;width:40px;height:40px;background:#00ff00'></div></div>",
        60,
        60,
        0,
    );
    let c = px(&opacity_ctx, 20, 20);
    assert!(
        ((c >> 8) & 255) > 250 && ((c >> 16) & 255) < 10,
        "negative z-index child inside opacity<1 context must paint above preceding red sibling: #{c:06x}"
    );
}

#[test]
fn abspos_hypothetical_static_positions_in_block_inline_flex_and_grid() {
    // Inline abspos does not break normal-flow inline line, and aligns to inline cursor.
    let (doc, f) = render_doc(
        "<div style='position:relative;width:200px'><span id=s1 style='display:inline-block;width:30px;height:20px;background:#0000aa'></span><span id=abs style='position:absolute;width:15px;height:20px;background:#00aa00'></span><span id=s2 style='display:inline-block;width:30px;height:20px;background:#aa0000'></span></div>",
        200,
        60,
        0,
    );
    assert_eq!(box_of(&doc, &f, "#s1").x, 0);
    assert_eq!(box_of(&doc, &f, "#abs").x, 30);
    assert_eq!(box_of(&doc, &f, "#s2").x, 30);
    assert_eq!(box_of(&doc, &f, "#s2").y, 0);

    // Flex container justify-content: center; align-items: flex-end aligns abspos child with auto insets.
    let (doc, flex_f) = render_doc(
        "<div style='position:relative;display:flex;justify-content:center;align-items:flex-end;width:100px;height:80px'><div id=a style='position:absolute;width:20px;height:30px;background:#00aa00'></div></div>",
        120,
        100,
        0,
    );
    let a = box_of(&doc, &flex_f, "#a");
    assert_eq!((a.x, a.y, a.width, a.height), (40, 50, 20, 30));

    // Grid container justify-items: end; align-items: center aligns abspos child.
    let (doc, grid_f) = render_doc(
        "<div style='position:relative;display:grid;justify-items:end;align-items:center;width:100px;height:80px'><div id=g style='position:absolute;width:20px;height:30px;background:#00aa00'></div></div>",
        120,
        100,
        0,
    );
    let g = box_of(&doc, &grid_f, "#g");
    assert_eq!((g.x, g.y, g.width, g.height), (80, 25, 20, 30));
}

#[test]
fn sticky_positioning_clamps_to_viewport_and_containing_block_bounds() {
    let html = "<div style='height:120px'><div id=s style='position:sticky;top:10px;width:40px;height:30px;background:#00aa00'></div></div><div style='height:300px'></div>";
    let (doc0, f0) = render_doc(html, 100, 100, 0);
    assert_eq!(box_of(&doc0, &f0, "#s").y, 10);

    // At scroll=50, top:10px puts document y=60 -> viewport y=10, well within 120px container (max document y=90).
    let (doc50, f50) = render_doc(html, 100, 100, 50);
    assert_eq!(box_of(&doc50, &f50, "#s").y, 10);
    assert_eq!(px(&f50, 20, 15), 0x00aa00);

    // At scroll=95, clamped at container bottom (document y=90 -> viewport y = 90 - 95 = -5).
    let (doc95, f95) = render_doc(html, 100, 100, 95);
    assert_eq!(box_of(&doc95, &f95, "#s").y, -5);
}

#[test]
fn direction_rtl_aspect_ratio_calc_pseudos_and_visibility_collapse() {
    // 1. RTL block + RTL inline boxes + RTL table columns
    let (doc, rtl_f) = render_doc(
        "<div style='direction:rtl;width:100px'><div id=blk style='width:40px;height:10px'></div><div><span id=i1 style='display:inline-block;width:20px;height:10px'></span><span id=i2 style='display:inline-block;width:30px;height:10px'></span></div><table style='width:100px;border-spacing:0'><tr><td id=c1 style='width:35px;height:10px'></td><td id=c2 style='width:65px;height:10px'></td></tr></table></div>",
        120,
        80,
        0,
    );
    assert_eq!(box_of(&doc, &rtl_f, "#blk").x, 60);
    assert_eq!(box_of(&doc, &rtl_f, "#i1").x, 80);
    assert_eq!(box_of(&doc, &rtl_f, "#i2").x, 50);
    assert_eq!(box_of(&doc, &rtl_f, "#c1").x, 65);
    assert_eq!(box_of(&doc, &rtl_f, "#c2").x, 0);

    // 2. aspect-ratio height->width & width->height + linear calc(% + px) in Flex/Grid
    let (doc, ar_f) = render_doc(
        "<div style='display:flex;width:200px;gap:calc(10% + 4px)'><div id=ar style='height:40px;aspect-ratio:2/1;background:#00aa00'></div><div id=calc style='width:calc(25% + 10px);height:20px;background:#0000aa'></div></div>",
        240,
        80,
        0,
    );
    let ar = box_of(&doc, &ar_f, "#ar");
    let calc = box_of(&doc, &ar_f, "#calc");
    assert_eq!((ar.width, ar.height), (80, 40));
    // gap = 10% of 200 + 4 = 24 -> calc.x = 80 + 24 = 104, width = 25% of 200 + 10 = 60.
    assert_eq!((calc.x, calc.width), (104, 60));

    // 3. ::before / ::after materialization without synthetic DOM ID leakage
    let (doc, pseudo_f) = render_doc(
        "<style>#host::before{content:'';display:inline-block;width:25px;height:20px;background:#00aa00}#host::after{content:'OK'}</style><div id=host><span id=mid style='display:inline-block;width:15px;height:20px;background:#0000aa'></span></div>",
        120,
        60,
        0,
    );
    assert_eq!(px(&pseudo_f, 10, 10), 0x00aa00);
    assert_eq!(box_of(&doc, &pseudo_f, "#mid").x, 25);
    assert!(
        pseudo_f.boxes.iter().all(|b| b.node < doc.nodes.len()),
        "synthetic pseudo node leaked into Frame.boxes"
    );
    assert!(
        pseudo_f.text_runs.iter().all(|t| t.node < doc.nodes.len()),
        "synthetic pseudo node leaked into Frame.text_runs"
    );

    // 4. visibility: collapse on flex item preserves cross-size strut while collapsing main size;
    // visibility: visible inside visibility: hidden paints visibly.
    let (doc, vis_f) = render_doc(
        "<div id=flex style='display:flex;align-items:stretch'><div style='visibility:collapse;width:70px;height:50px;background:#ff0000'></div><div id=kept style='width:30px;background:#00aa00'></div></div><div style='visibility:hidden;width:40px;height:20px;background:#ff0000'><div style='visibility:visible;width:20px;height:20px;background:#0000ff'></div></div>",
        120,
        100,
        0,
    );
    let kept = box_of(&doc, &vis_f, "#kept");
    assert_eq!((kept.x, kept.y, kept.width, kept.height), (0, 0, 30, 50));
    assert_eq!(px(&vis_f, 10, 60), 0x0000ff);
    assert_eq!(px(&vis_f, 30, 60), 0xffffff);
}

#[test]
fn floats_clear_bfc_avoidance_and_generated_content_parity() {
    let (_, flex_gen) = render_doc(
        "<style>div{background:#3366cc;display:flex} div::after,p{content:'xxx';background:yellow;margin:1em;width:200px;height:2em} div::after{content:'yyy';display:block}</style><div><p>FAIL</p></div>",
        800,
        200,
        0,
    );
    let (_, float_ref) = render_doc(
        "<style>div{background:#3366cc;height:4em} div::after,p{content:'xxx';background:yellow;margin:1em;width:200px;height:2em;float:left} div::after{content:'yyy';display:block}</style><div><p>FAIL</p></div>",
        800,
        200,
        0,
    );
    assert_eq!(flex_gen.canvas.pixels, float_ref.canvas.pixels);

    let (doc, bfc_f) = render_doc(
        "<div id=fl style='float:left;width:60px;height:40px;background:#0000ff'></div><div id=beside style='overflow:hidden;height:20px;background:#00aa00'></div><div id=cleared style='clear:both;width:80px;height:20px;background:#ff0000'></div>",
        200,
        120,
        0,
    );
    let beside = box_of(&doc, &bfc_f, "#beside");
    let cleared = box_of(&doc, &bfc_f, "#cleared");
    assert_eq!((beside.x, beside.y, beside.width), (60, 0, 140));
    assert_eq!((cleared.x, cleared.y), (0, 40));
}
