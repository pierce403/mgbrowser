//! Authored adapter controls frozen before physical-justification implementation.
//! These exercise real Stylo snapshots, not upstream WPT or rendered geometry.

use super::*;

const SAFE_REVERSE_REASON: &str = "safe physical justification in reverse flex is not implemented";

fn computed_with_diagnostics(css: &str) -> (mg::ComputedStyle, mg::StyleDiagnostics) {
    let document = crate::document::parse(
        &format!("<html><body><div id=x style='{css}'></div></body></html>"),
        "https://fixture.example/",
    );
    let id = document.query_selector(0, "#x").unwrap().unwrap();
    let mut diagnostics = mg::StyleDiagnostics::default();
    let style = mg::compute_styles_with_diagnostics(
        &document,
        &document.stylesheets,
        (1024., 768.),
        &mut diagnostics,
    )
    .unwrap()
    .swap_remove(id);
    (style, diagnostics)
}

fn computed(css: &str) -> mg::ComputedStyle {
    computed_with_diagnostics(css).0
}

fn views(style: &mg::ComputedStyle) -> [StyleView<'_>; 2] {
    [StyleView::root_content(style), StyleView::item(style, None)]
}

fn expected(keyword: AlignContentKeyword, safety: AlignmentSafety) -> Option<JustifyContent> {
    Some(AlignContent { keyword, safety })
}

#[test]
fn physical_snapshot_preserves_keyword_safety_and_authored_axis() {
    for display in ["flex", "inline-flex", "grid", "inline-grid"] {
        for (direction, axis) in [
            ("row", mg::FlexDirection::Row),
            ("row-reverse", mg::FlexDirection::RowReverse),
            ("column", mg::FlexDirection::Column),
            ("column-reverse", mg::FlexDirection::ColumnReverse),
        ] {
            for (word, keyword) in [
                ("left", mg::AlignKeyword::Left),
                ("right", mg::AlignKeyword::Right),
            ] {
                for (prefix, safety) in [
                    ("", mg::AlignSafety::Default),
                    ("unsafe ", mg::AlignSafety::Unsafe),
                    ("safe ", mg::AlignSafety::Safe),
                ] {
                    let css = format!(
                        "display:{display};flex-direction:{direction};justify-content:{prefix}{word}"
                    );
                    let style = computed(&css);
                    assert_eq!(style.layout.flex_direction, axis, "{css}");
                    assert_eq!(
                        style.layout.justify_content,
                        mg::Alignment {
                            keyword,
                            safety,
                            legacy: false
                        },
                        "{css}"
                    );
                    assert!(!style.layout.unsupported, "{css}");
                }
            }
        }
    }
}

#[test]
fn flex_rows_resolve_physical_sides_without_reversing_the_keyword() {
    for display in ["flex", "inline-flex"] {
        for (direction, axis) in [
            ("row", FlexDirection::Row),
            ("row-reverse", FlexDirection::RowReverse),
        ] {
            for (word, keyword) in [
                ("left", AlignContentKeyword::Start),
                ("right", AlignContentKeyword::End),
            ] {
                for prefix in ["", "unsafe "] {
                    let css = format!(
                        "display:{display};flex-direction:{direction};justify-content:{prefix}{word}"
                    );
                    let style = computed(&css);
                    for view in views(&style) {
                        assert_eq!(view.validate(), Ok(()), "{css}");
                        assert_eq!(view.flex_direction(), axis, "{css}");
                        assert_eq!(
                            FlexboxContainerStyle::justify_content(&view),
                            expected(keyword, AlignmentSafety::Unsafe),
                            "{css}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn flex_columns_resolve_both_physical_sides_to_start() {
    for display in ["flex", "inline-flex"] {
        for (direction, axis) in [
            ("column", FlexDirection::Column),
            ("column-reverse", FlexDirection::ColumnReverse),
        ] {
            for word in ["left", "right"] {
                for prefix in ["", "unsafe "] {
                    let css = format!(
                        "display:{display};flex-direction:{direction};justify-content:{prefix}{word}"
                    );
                    let style = computed(&css);
                    for view in views(&style) {
                        assert_eq!(view.validate(), Ok(()), "{css}");
                        assert_eq!(view.flex_direction(), axis, "{css}");
                        assert_eq!(
                            FlexboxContainerStyle::justify_content(&view),
                            expected(AlignContentKeyword::Start, AlignmentSafety::Unsafe),
                            "{css}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn grid_physical_inline_axis_ignores_all_authored_flex_directions() {
    for display in ["grid", "inline-grid"] {
        for direction in ["row", "row-reverse", "column", "column-reverse"] {
            for (word, keyword) in [
                ("left", AlignContentKeyword::Start),
                ("right", AlignContentKeyword::End),
            ] {
                for (prefix, safety) in [
                    ("", AlignmentSafety::Unsafe),
                    ("unsafe ", AlignmentSafety::Unsafe),
                    ("safe ", AlignmentSafety::Safe),
                ] {
                    let css = format!(
                        "display:{display};flex-direction:{direction};justify-content:{prefix}{word}"
                    );
                    let style = computed(&css);
                    for view in views(&style) {
                        assert_eq!(view.validate(), Ok(()), "{css}");
                        assert_eq!(
                            GridContainerStyle::justify_content(&view),
                            expected(keyword, safety),
                            "{css}"
                        );
                        // The existing grid route delegates to this getter. Its
                        // answer must depend on actual grid display, not flex axis.
                        assert_eq!(
                            FlexboxContainerStyle::justify_content(&view),
                            expected(keyword, safety),
                            "{css}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn safe_forward_flex_preserves_safety_on_the_resolved_keyword() {
    for display in ["flex", "inline-flex"] {
        for direction in ["row", "column"] {
            for word in ["left", "right"] {
                let css = format!(
                    "display:{display};flex-direction:{direction};justify-content:safe {word}"
                );
                let style = computed(&css);
                let keyword = if direction == "row" && word == "right" {
                    AlignContentKeyword::End
                } else {
                    AlignContentKeyword::Start
                };
                for view in views(&style) {
                    assert_eq!(view.validate(), Ok(()), "{css}");
                    assert_eq!(
                        FlexboxContainerStyle::justify_content(&view),
                        expected(keyword, AlignmentSafety::Safe),
                        "{css}"
                    );
                }
            }
        }
    }
}

#[test]
fn safe_reverse_flex_has_the_explicit_refusal_before_layout() {
    for display in ["flex", "inline-flex"] {
        for direction in ["row-reverse", "column-reverse"] {
            for word in ["left", "right"] {
                let css = format!(
                    "display:{display};flex-direction:{direction};justify-content:safe {word}"
                );
                let style = computed(&css);
                assert_eq!(
                    style.layout.justify_content.safety,
                    mg::AlignSafety::Safe,
                    "{css}"
                );
                for view in views(&style) {
                    assert_eq!(view.validate(), Err(SAFE_REVERSE_REASON), "{css}");
                    assert_eq!(FlexboxContainerStyle::justify_content(&view), None, "{css}");
                }
            }
        }
    }
}

#[test]
fn physical_content_values_are_inert_on_block_and_table_boxes() {
    for display in [
        "block",
        "inline-block",
        "table",
        "inline-table",
        "table-row",
        "table-cell",
    ] {
        for word in ["left", "right"] {
            for prefix in ["", "unsafe ", "safe "] {
                let css = format!(
                    "display:{display};flex-direction:row-reverse;justify-content:{prefix}{word}"
                );
                let style = computed(&css);
                for view in views(&style) {
                    assert_eq!(view.validate(), Ok(()), "{css}");
                    assert_eq!(FlexboxContainerStyle::justify_content(&view), None, "{css}");
                    assert_eq!(GridContainerStyle::justify_content(&view), None, "{css}");
                }
            }
        }
    }
}

#[test]
fn anonymous_views_never_inherit_physical_formatting_or_refusal() {
    for display in ["flex", "grid"] {
        for word in ["left", "right"] {
            let mut style = computed(&format!(
                "display:{display};flex-direction:row-reverse;justify-content:safe {word}"
            ));
            style.layout.unsupported = true;
            style.layout.justify_content.legacy = true;
            style.layout.flex_grow = f32::NAN;
            let view = StyleView::anonymous(&style);
            assert_eq!(view.validate(), Ok(()));
            assert_eq!(view.flex_direction(), FlexDirection::Row);
            assert_eq!(FlexboxContainerStyle::justify_content(&view), None);
            assert_eq!(GridContainerStyle::justify_content(&view), None);
        }
    }
}

#[test]
fn legacy_physical_values_are_rejected_before_inert_display_resolution() {
    for display in ["block", "table", "flex", "grid"] {
        for word in ["left", "right"] {
            let mut style = computed(&format!("display:{display};justify-content:{word}"));
            style.layout.justify_content.legacy = true;
            for view in views(&style) {
                assert_eq!(view.validate(), Err("legacy alignment is not implemented"));
                assert_eq!(FlexboxContainerStyle::justify_content(&view), None);
                assert_eq!(GridContainerStyle::justify_content(&view), None);
            }
        }
    }
}

#[test]
fn physical_keywords_do_not_leak_into_cross_or_item_alignment() {
    let original = computed("display:flex;justify-content:start");
    for keyword in [mg::AlignKeyword::Left, mg::AlignKeyword::Right] {
        for field in [
            "align-content",
            "align-items",
            "justify-items",
            "align-self",
            "justify-self",
        ] {
            let mut style = original.clone();
            let value = mg::Alignment::new(keyword);
            match field {
                "align-content" => style.layout.align_content = value,
                "align-items" => style.layout.align_items = value,
                "justify-items" => style.layout.justify_items = value,
                "align-self" => style.layout.align_self = value,
                "justify-self" => style.layout.justify_self = value,
                _ => unreachable!(),
            }
            for view in views(&style) {
                assert!(view.validate().is_err(), "{field}: {keyword:?}");
            }
        }
    }
}

#[test]
fn existing_nonphysical_keywords_keep_their_exact_trait_values() {
    for context in [
        "display:flex;flex-direction:row-reverse",
        "display:grid;flex-direction:column",
    ] {
        for (word, keyword) in [
            ("start", AlignContentKeyword::Start),
            ("end", AlignContentKeyword::End),
            ("flex-start", AlignContentKeyword::FlexStart),
            ("flex-end", AlignContentKeyword::FlexEnd),
            ("center", AlignContentKeyword::Center),
        ] {
            for (prefix, safety) in [
                ("", AlignmentSafety::Unsafe),
                ("unsafe ", AlignmentSafety::Unsafe),
                ("safe ", AlignmentSafety::Safe),
            ] {
                let css = format!("{context};justify-content:{prefix}{word}");
                let style = computed(&css);
                for view in views(&style) {
                    assert_eq!(view.validate(), Ok(()), "{css}");
                    assert_eq!(
                        FlexboxContainerStyle::justify_content(&view),
                        expected(keyword, safety),
                        "{css}"
                    );
                    assert_eq!(
                        GridContainerStyle::justify_content(&view),
                        expected(keyword, safety),
                        "{css}"
                    );
                }
            }
        }
        for (word, value) in [
            ("normal", None),
            ("stretch", Some(AlignContent::STRETCH)),
            ("space-between", Some(AlignContent::SPACE_BETWEEN)),
            ("space-around", Some(AlignContent::SPACE_AROUND)),
            ("space-evenly", Some(AlignContent::SPACE_EVENLY)),
        ] {
            let css = format!("{context};justify-content:{word}");
            let style = computed(&css);
            for view in views(&style) {
                assert_eq!(view.validate(), Ok(()), "{css}");
                assert_eq!(
                    FlexboxContainerStyle::justify_content(&view),
                    value,
                    "{css}"
                );
                assert_eq!(GridContainerStyle::justify_content(&view), value, "{css}");
            }
        }
    }
}

#[test]
fn nonfinite_and_unrepresented_state_still_precedes_layout() {
    let original = computed("display:flex;justify-content:left");
    let mut style = original.clone();
    style.layout.unsupported = true;
    for view in views(&style) {
        assert_eq!(
            view.validate(),
            Err("computed layout contains an unrepresented CSS value")
        );
    }
    let mut style = original.clone();
    style.layout.flex_grow = f32::NAN;
    for view in views(&style) {
        assert_eq!(view.validate(), Err("non-finite or negative flex factor"));
    }
    let mut style = original.clone();
    style.width = mg::Length::Px(f32::INFINITY);
    for view in views(&style) {
        assert_eq!(view.validate(), Err("non-finite layout length"));
    }
    let mut style = original.clone();
    style.border_width[0] = f32::NAN;
    for view in views(&style) {
        assert_eq!(view.validate(), Err("non-finite or negative border width"));
    }
    assert_eq!(
        StyleView::item(&original, Some(f32::INFINITY)).validate(),
        Err("invalid intrinsic image aspect ratio")
    );
}

#[test]
fn rtl_physical_justify_flips_inline_edges_and_nonhorizontal_writing_modes_retain_diagnostics() {
    for display in ["flex", "grid"] {
        for (kw, expected) in [("right", AlignContent::START), ("left", AlignContent::END)] {
            let css = format!("display:{display};justify-content:{kw};direction:rtl");
            let (style, diagnostics) = computed_with_diagnostics(&css);
            assert!(!style.layout.unsupported, "{css}");
            assert!(diagnostics.entries.is_empty(), "{css}: {diagnostics:?}");
            for view in views(&style) {
                assert_eq!(view.validate(), Ok(()), "{css}");
                assert_eq!(view.direction(), Direction::Rtl, "{css}");
                assert_eq!(
                    FlexboxContainerStyle::justify_content(&view),
                    Some(expected),
                    "{css}"
                );
            }
        }
        for writing_mode in ["vertical-rl", "vertical-lr"] {
            let css =
                format!("display:{display};justify-content:right;writing-mode:{writing_mode}");
            let (style, diagnostics) = computed_with_diagnostics(&css);
            // The pinned Servo configuration rejects this property at parse
            // time. Its retained initial horizontal snapshot is not itself an
            // unsupported computed value, and must not imply vertical support.
            assert!(!style.layout.unsupported, "{css}");
            assert_eq!(diagnostics.omitted, 0, "{css}");
            assert!(
                diagnostics.entries.iter().any(
                    |entry| entry.kind == "css-parse" && entry.message.contains("writing-mode")
                ),
                "{css}"
            );
            for view in views(&style) {
                assert_eq!(view.validate(), Ok(()), "{css}");
                assert_eq!(
                    FlexboxContainerStyle::justify_content(&view),
                    Some(AlignContent::END),
                    "{css}"
                );
            }
        }
    }
}

#[test]
fn physical_resolution_keeps_root_outer_edges_stripped_and_item_edges_intact() {
    let style = computed(
        "display:flex;justify-content:right;width:40px;height:60px;margin:3px;padding:5px;border:2px solid;gap:7px 11px",
    );
    let root = StyleView::root_content(&style);
    let item = StyleView::item(&style, None);
    assert_eq!(root.validate(), Ok(()));
    assert_eq!(item.validate(), Ok(()));
    assert_eq!(
        FlexboxContainerStyle::justify_content(&root),
        Some(AlignContent::END)
    );
    assert_eq!(
        FlexboxContainerStyle::justify_content(&item),
        Some(AlignContent::END)
    );
    assert_eq!(
        root.size(),
        Size {
            width: Dimension::auto(),
            height: Dimension::auto()
        }
    );
    assert_eq!(
        item.size(),
        Size {
            width: Dimension::length(40.),
            height: Dimension::length(60.)
        }
    );
    assert_eq!(root.margin(), rect([LengthPercentageAuto::length(0.); 4]));
    assert_eq!(item.margin(), rect([LengthPercentageAuto::length(3.); 4]));
    assert_eq!(root.padding(), rect([LengthPercentage::length(0.); 4]));
    assert_eq!(item.padding(), rect([LengthPercentage::length(5.); 4]));
    assert_eq!(root.border(), rect([LengthPercentage::length(0.); 4]));
    assert_eq!(item.border(), rect([LengthPercentage::length(2.); 4]));
    assert_eq!(
        FlexboxContainerStyle::gap(&root),
        Size {
            width: LengthPercentage::length(11.),
            height: LengthPercentage::length(7.)
        }
    );
    assert_eq!(
        FlexboxContainerStyle::gap(&item),
        FlexboxContainerStyle::gap(&root)
    );
}
