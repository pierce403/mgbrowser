//! Specified declaration parsing for modern DOM bindings, with no JS or IO.
//!
//! All parser/atom/serializer temporaries remain inside the existing worker
//! System/CPU/active containment. Stylo's Servo writer is a concrete String:
//! the output bound below is post-serialization, not a pre-allocation quota.
//! Before transferring Replace(String) into BrowserHost::attr, the bridge must
//! admit its actual capacity excess over the len that attr already charges.
//! For reads it must separately admit serializer String capacity and the actual
//! UTF-16 result Vec capacity. No independent allowance or fatal latch lives here.

use cssparser::{Parser, ParserInput, ToCss as _, Token};
use std::borrow::Cow;
use style::{
    context::QuirksMode,
    parser::ParserContext,
    properties::{
        Importance, PropertyDeclarationBlock, PropertyId, SourcePropertyDeclaration,
        SourcePropertyDeclarationUpdate, parse_one_declaration_into, parse_style_attribute,
    },
    stylesheets::{CssRuleType, Origin, UrlExtraData},
};
use style_traits::ParsingMode;
use url::Url;

const CQ_UNITS: [&str; 6] = ["cqb", "cqh", "cqi", "cqmax", "cqmin", "cqw"];
const SURROGATE_UNITS: [&str; 22] = [
    "dvb", "dvh", "dvi", "dvmax", "dvmin", "dvw", "lvb", "lvh", "lvi", "lvmax", "lvmin", "lvw",
    "svb", "svh", "svi", "svmax", "svmin", "svw", "rcap", "rch", "rex", "ric",
];

#[derive(Clone, Copy, Default)]
struct CqUnitMap {
    /// For each entry in `CQ_UNITS`, the chosen surrogate unit if active.
    surrogates: [Option<&'static str>; 6],
}

impl CqUnitMap {
    fn from_inputs(inputs: &[&str]) -> Self {
        if !inputs.iter().copied().any(may_contain_cq) {
            return Self::default();
        }
        let mut used = [false; SURROGATE_UNITS.len()];
        let mut cq_present = [false; CQ_UNITS.len()];
        for input in inputs {
            let mut pi = ParserInput::new(input);
            let mut parser = Parser::new(&mut pi);
            scan_dimension_units(&mut parser, &mut used, &mut cq_present);
        }
        if !cq_present.iter().any(|&present| present) {
            return Self::default();
        }
        let mut map = Self::default();
        let mut next_candidate = 0usize;
        for (idx, present) in cq_present.iter().copied().enumerate() {
            if !present {
                continue;
            }
            while next_candidate < SURROGATE_UNITS.len() && used[next_candidate] {
                next_candidate += 1;
            }
            if let Some(&surrogate) = SURROGATE_UNITS.get(next_candidate) {
                map.surrogates[idx] = Some(surrogate);
                next_candidate += 1;
            }
        }
        map
    }

    fn is_active(&self) -> bool {
        self.surrogates.iter().any(Option::is_some)
    }

    fn to_surrogate(&self, unit: &str) -> Option<&'static str> {
        CQ_UNITS
            .iter()
            .position(|candidate| unit.eq_ignore_ascii_case(candidate))
            .and_then(|idx| self.surrogates[idx])
    }

    fn from_surrogate(&self, unit: &str) -> Option<&'static str> {
        self.surrogates
            .iter()
            .position(|mapped| mapped.is_some_and(|surrogate| unit.eq_ignore_ascii_case(surrogate)))
            .map(|idx| CQ_UNITS[idx])
    }

    fn rewrite_value<'a>(&self, input: &'a str) -> Cow<'a, str> {
        if !self.is_active() || !may_contain_cq(input) {
            return Cow::Borrowed(input);
        }
        replace_dimensions(input, false, |unit| self.to_surrogate(unit))
    }

    fn rewrite_style_list<'a>(&self, input: &'a str) -> Cow<'a, str> {
        if !self.is_active() || !may_contain_cq(input) {
            return Cow::Borrowed(input);
        }
        replace_dimensions(input, true, |unit| self.to_surrogate(unit))
    }

    fn restore_value(&self, text: String) -> String {
        if !self.is_active() || text.is_empty() {
            return text;
        }
        match replace_dimensions(&text, false, |unit| self.from_surrogate(unit)) {
            Cow::Borrowed(_) => text,
            Cow::Owned(restored) => restored,
        }
    }

    fn restore_style_list(&self, text: String) -> String {
        if !self.is_active() || text.is_empty() {
            return text;
        }
        match replace_dimensions(&text, true, |unit| self.from_surrogate(unit)) {
            Cow::Borrowed(_) => text,
            Cow::Owned(restored) => restored,
        }
    }
}

fn may_contain_cq(text: &str) -> bool {
    text.as_bytes()
        .windows(2)
        .any(|pair| pair[0].eq_ignore_ascii_case(&b'c') && pair[1].eq_ignore_ascii_case(&b'q'))
}

fn scan_dimension_units(
    parser: &mut Parser<'_, '_>,
    used: &mut [bool; SURROGATE_UNITS.len()],
    cq_present: &mut [bool; CQ_UNITS.len()],
) {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        match token {
            Token::Dimension { unit, .. } => {
                for (idx, candidate) in SURROGATE_UNITS.iter().enumerate() {
                    if unit.eq_ignore_ascii_case(candidate) {
                        used[idx] = true;
                    }
                }
                for (idx, cq) in CQ_UNITS.iter().enumerate() {
                    if unit.eq_ignore_ascii_case(cq) {
                        cq_present[idx] = true;
                    }
                }
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(|nested| {
                    scan_dimension_units(nested, used, cq_present);
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
            }
            _ => {}
        }
    }
}

fn replace_dimensions<'a, F>(input: &'a str, skip_custom_decls: bool, map_unit: F) -> Cow<'a, str>
where
    F: Fn(&str) -> Option<&'static str> + Copy,
{
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let mut pi = ParserInput::new(input);
    let mut parser = Parser::new(&mut pi);
    collect_dimension_spans(input, &mut parser, skip_custom_decls, map_unit, &mut spans);
    if spans.is_empty() {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    for (start, end, replacement) in spans {
        out.push_str(&input[cursor..start]);
        out.push_str(&replacement);
        cursor = end;
    }
    out.push_str(&input[cursor..]);
    Cow::Owned(out)
}

fn collect_dimension_spans<F>(
    source: &str,
    parser: &mut Parser<'_, '_>,
    skip_custom_decls: bool,
    map_unit: F,
    spans: &mut Vec<(usize, usize, String)>,
) where
    F: Fn(&str) -> Option<&'static str> + Copy,
{
    let mut at_decl_start = skip_custom_decls;
    let mut in_custom_decl = false;
    loop {
        let start = parser.position().byte_index();
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        let end = parser.position().byte_index();
        match token {
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::Semicolon if skip_custom_decls => {
                at_decl_start = true;
                in_custom_decl = false;
            }
            Token::Ident(name) if at_decl_start => {
                in_custom_decl = name.starts_with("--");
                at_decl_start = false;
            }
            Token::Dimension {
                has_sign,
                value,
                int_value,
                unit,
            } if !in_custom_decl => {
                at_decl_start = false;
                if let Some(target_unit) = map_unit(unit.as_ref()) {
                    let raw_slice = &source[start..end];
                    let replacement = if raw_slice.len() >= unit.len()
                        && raw_slice
                            .get(raw_slice.len() - unit.len()..)
                            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(unit.as_ref()))
                    {
                        let mut s =
                            String::with_capacity(raw_slice.len() - unit.len() + target_unit.len());
                        s.push_str(&raw_slice[..raw_slice.len() - unit.len()]);
                        s.push_str(target_unit);
                        s
                    } else {
                        let rewritten = Token::Dimension {
                            has_sign,
                            value,
                            int_value,
                            unit: target_unit.into(),
                        };
                        let mut s = String::new();
                        let _ = rewritten.to_css(&mut s);
                        s
                    };
                    spans.push((start, end, replacement));
                }
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                at_decl_start = false;
                let skip_inner = in_custom_decl;
                let _ = parser.parse_nested_block(|nested| {
                    if !skip_inner {
                        collect_dimension_spans(source, nested, false, map_unit, spans);
                    } else {
                        while nested.next_including_whitespace_and_comments().is_ok() {}
                    }
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
            }
            _ => {
                at_decl_start = false;
            }
        }
    }
}

// The current attribute boundary, not a new CSSOM allowance.
pub(crate) const ATTRIBUTE_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    CurrentStyle,
    CssName,
    Value,
    Priority,
    CssText,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeclarationError {
    TooLarge(Part),
    Serialization,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Mutation {
    Unchanged,
    Replace(String),
}

fn bounded(text: &str, part: Part) -> Result<(), DeclarationError> {
    if text.len() > ATTRIBUTE_BYTES {
        Err(DeclarationError::TooLarge(part))
    } else {
        Ok(())
    }
}

/// CSS names only: no trimming, camelCase conversion or property-value guessing.
/// Stylo preserves case-sensitive custom names and rejects disabled properties.
pub(crate) fn enabled_css_name(name: &str) -> Result<Option<PropertyId>, DeclarationError> {
    bounded(name, Part::CssName)?;
    Ok(PropertyId::parse_enabled_for_all_content(name).ok())
}

/// Optional named-accessor gate AFTER the separately reviewed JS-name mapping.
/// This does not itself implement CSSStyleProperties/expando/IDL name semantics.
pub(crate) fn enabled_non_custom_css_name(
    name: &str,
) -> Result<Option<PropertyId>, DeclarationError> {
    Ok(enabled_css_name(name)?.filter(|id| matches!(id, PropertyId::NonCustom(_))))
}

fn context(url: &UrlExtraData) -> ParserContext<'_> {
    ParserContext::new(
        Origin::Author,
        url,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    )
}

#[cfg(test)]
fn parse_current(
    raw: &str,
    url: &UrlExtraData,
) -> Result<PropertyDeclarationBlock, DeclarationError> {
    let cq_map = CqUnitMap::from_inputs(&[raw]);
    parse_current_mapped(raw, url, &cq_map)
}

fn parse_current_mapped(
    raw: &str,
    url: &UrlExtraData,
    cq_map: &CqUnitMap,
) -> Result<PropertyDeclarationBlock, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    let rewritten = cq_map.rewrite_style_list(raw);
    Ok(parse_style_attribute(
        &rewritten,
        url,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    ))
}

fn serialize(
    block: &PropertyDeclarationBlock,
    cq_map: &CqUnitMap,
) -> Result<String, DeclarationError> {
    let mut text = String::new();
    block
        .to_css(&mut text)
        .map_err(|_| DeclarationError::Serialization)?;
    let text = cq_map.restore_style_list(text);
    bounded(&text, Part::Output)?;
    Ok(text)
}

fn replacement(
    raw: &str,
    block: &PropertyDeclarationBlock,
    cq_map: &CqUnitMap,
) -> Result<Mutation, DeclarationError> {
    let text = serialize(block, cq_map)?;
    Ok(if text == raw {
        Mutation::Unchanged
    } else {
        Mutation::Replace(text)
    })
}

/// Canonical specified value, not computed style. Unknown/absent names are empty.
pub(crate) fn declared_value(
    raw: &str,
    css_name: &str,
    base: &Url,
) -> Result<String, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    let Some(id) = enabled_css_name(css_name)? else {
        return Ok(String::new());
    };
    let url = UrlExtraData::from(base.clone());
    let cq_map = CqUnitMap::from_inputs(&[raw]);
    let block = parse_current_mapped(raw, &url, &cq_map)?;
    specified_value(&block, &id, &cq_map)
}

fn specified_value(
    block: &PropertyDeclarationBlock,
    id: &PropertyId,
    cq_map: &CqUnitMap,
) -> Result<String, DeclarationError> {
    let mut text = String::new();
    block
        .property_value_to_css(id, &mut text)
        .map_err(|_| DeclarationError::Serialization)?;
    let text = if matches!(id, PropertyId::Custom(_)) {
        text
    } else {
        cq_map.restore_value(text)
    };
    bounded(&text, Part::Output)?;
    Ok(text)
}

pub(crate) fn declared_priority(
    raw: &str,
    css_name: &str,
    base: &Url,
) -> Result<String, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    let Some(id) = enabled_css_name(css_name)? else {
        return Ok(String::new());
    };
    let url = UrlExtraData::from(base.clone());
    let cq_map = CqUnitMap::from_inputs(&[raw]);
    let block = parse_current_mapped(raw, &url, &cq_map)?;
    Ok(if block.property_priority(&id).important() {
        "important".into()
    } else {
        String::new()
    })
}

/// Prepare both the specified return and all constituent removals from the same
/// block. An empty shorthand return does not imply its longhands are absent.
pub(crate) fn remove_declared_value(
    raw: &str,
    css_name: &str,
    base: &Url,
) -> Result<(String, Mutation), DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    let Some(id) = enabled_css_name(css_name)? else {
        return Ok((String::new(), Mutation::Unchanged));
    };
    let url = UrlExtraData::from(base.clone());
    let cq_map = CqUnitMap::from_inputs(&[raw]);
    let mut block = parse_current_mapped(raw, &url, &cq_map)?;
    let text = specified_value(&block, &id, &cq_map)?;
    let Some(first) = block.first_declaration_to_remove(&id) else {
        return Ok((text, Mutation::Unchanged));
    };
    block.remove_property(&id, first);
    Ok((text, replacement(raw, &block, &cq_map)?))
}

/// A read never rewrites the original raw attribute, including malformed input.
pub(crate) fn declared_css_text(raw: &str, base: &Url) -> Result<String, DeclarationError> {
    let url = UrlExtraData::from(base.clone());
    let cq_map = CqUnitMap::from_inputs(&[raw]);
    serialize(&parse_current_mapped(raw, &url, &cq_map)?, &cq_map)
}

/// One ordinary-priority property assignment. Invalid CSS is a no-op, while
/// resource/serialization failures remain errors. No mutation is committed here.
pub(crate) fn assign_declared_value(
    raw: &str,
    css_name: &str,
    value: &str,
    base: &Url,
) -> Result<Mutation, DeclarationError> {
    set_declared_value(raw, css_name, value, "", base)
}

pub(crate) fn set_declared_value(
    raw: &str,
    css_name: &str,
    value: &str,
    priority: &str,
    base: &Url,
) -> Result<Mutation, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    bounded(value, Part::Value)?;
    bounded(priority, Part::Priority)?;
    let Some(id) = enabled_css_name(css_name)? else {
        return Ok(Mutation::Unchanged);
    };
    let url = UrlExtraData::from(base.clone());
    // Also enforce the exact author Style-rule context before parse_into's
    // documented allowed-in-context precondition, not a testing/unchecked API.
    if PropertyId::parse(css_name, &context(&url)).is_err() {
        return Ok(Mutation::Unchanged);
    }
    if value.is_empty() {
        let cq_map = CqUnitMap::from_inputs(&[raw]);
        let mut block = parse_current_mapped(raw, &url, &cq_map)?;
        let Some(first) = block.first_declaration_to_remove(&id) else {
            return Ok(Mutation::Unchanged);
        };
        block.remove_property(&id, first);
        return replacement(raw, &block, &cq_map);
    }
    let importance = if priority.is_empty() {
        Importance::Normal
    } else if priority.eq_ignore_ascii_case("important") {
        Importance::Important
    } else {
        return Ok(Mutation::Unchanged);
    };
    let cq_map = if matches!(id, PropertyId::Custom(_)) {
        CqUnitMap::from_inputs(&[raw])
    } else {
        CqUnitMap::from_inputs(&[raw, value])
    };
    let rewritten_value = if matches!(id, PropertyId::Custom(_)) {
        Cow::Borrowed(value)
    } else {
        cq_map.rewrite_value(value)
    };
    let mut parsed = SourcePropertyDeclaration::default();
    if parse_one_declaration_into(
        &mut parsed,
        id,
        &rewritten_value,
        Origin::Author,
        &url,
        None,
        ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    )
    .is_err()
    {
        // This entrypoint parses the ENTIRE value and does not accept an
        // !important suffix or a second top-level declaration.
        return Ok(Mutation::Unchanged);
    }
    let mut block = parse_current_mapped(raw, &url, &cq_map)?;
    let mut updates = SourcePropertyDeclarationUpdate::default();
    if !block.prepare_for_update(&parsed, importance, &mut updates) {
        return Ok(Mutation::Unchanged);
    }
    // extend/push would implement parser cascade precedence and incorrectly
    // preserve an earlier important declaration. CSSOM assignment replaces it.
    block.update(parsed.drain(), importance, &mut updates);
    replacement(raw, &block, &cq_map)
}

/// Full-list cssText assignment intentionally drops malformed declarations.
pub(crate) fn replace_declared_css_text(
    raw: &str,
    input: &str,
    base: &Url,
) -> Result<Mutation, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    bounded(input, Part::CssText)?;
    let url = UrlExtraData::from(base.clone());
    let cq_map = CqUnitMap::from_inputs(&[input]);
    let rewritten = cq_map.rewrite_style_list(input);
    let block = parse_style_attribute(
        &rewritten,
        &url,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    );
    // cssText assignment updates its owner even when serialization is equal,
    // including creating style="" on an element whose attribute was absent.
    Ok(Mutation::Replace(serialize(&block, &cq_map)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://owned.example/dir/page.html").unwrap()
    }
    fn changed(value: Mutation) -> String {
        match value {
            Mutation::Replace(text) => text,
            Mutation::Unchanged => panic!("expected a prepared replacement"),
        }
    }

    #[test]
    fn enabled_names_do_not_guess_camelcase_or_fold_custom_case() {
        assert!(enabled_css_name("COLOR").unwrap().is_some());
        assert!(enabled_css_name("unknown-property").unwrap().is_none());
        assert!(enabled_css_name("backgroundColor").unwrap().is_none());
        assert!(enabled_css_name(" color").unwrap().is_none());
        assert_ne!(
            enabled_css_name("--Case").unwrap(),
            enabled_css_name("--case").unwrap()
        );
        assert!(enabled_non_custom_css_name("--Case").unwrap().is_none());
        assert!(enabled_non_custom_css_name("margin-top").unwrap().is_some());
    }

    #[test]
    fn canonical_css_text_is_not_raw_echo_and_reads_leave_input_untouched() {
        let raw = "color:red;; unknown-property: nope; width:001px";
        assert_eq!(
            declared_css_text(raw, &base()).unwrap(),
            "color: red; width: 1px;"
        );
        assert_eq!(declared_value(raw, "width", &base()).unwrap(), "1px");
        assert_eq!(declared_value(raw, "not-a-property", &base()).unwrap(), "");
        assert_eq!(raw, "color:red;; unknown-property: nope; width:001px");
    }

    #[test]
    fn single_assignment_replaces_importance_without_injecting_declarations() {
        let raw = "color:red!important; width: 7px";
        for invalid in ["not-a-color", "blue; width: 99px", "blue!important"] {
            assert_eq!(
                assign_declared_value(raw, "color", invalid, &base()).unwrap(),
                Mutation::Unchanged
            );
        }
        let next = changed(assign_declared_value(raw, "color", "blue", &base()).unwrap());
        assert_eq!(declared_value(&next, "color", &base()).unwrap(), "blue");
        assert_eq!(declared_value(&next, "width", &base()).unwrap(), "7px");
        let url = UrlExtraData::from(base());
        let block = parse_current(&next, &url).unwrap();
        assert_eq!(
            block.property_priority(&enabled_css_name("color").unwrap().unwrap()),
            Importance::Normal
        );
    }

    #[test]
    fn shorthand_expansion_lookup_and_empty_removal_are_generic() {
        let raw = "color:red; margin: 1px 2px";
        assert_eq!(declared_value(raw, "margin-left", &base()).unwrap(), "2px");
        assert_eq!(declared_value(raw, "margin", &base()).unwrap(), "1px 2px");
        assert_eq!(
            declared_value(
                "margin-top:1px!important; margin-right:2px; margin-bottom:1px; margin-left:2px",
                "margin",
                &base()
            )
            .unwrap(),
            ""
        );
        let next = changed(assign_declared_value(raw, "margin", "", &base()).unwrap());
        assert_eq!(declared_value(&next, "margin-left", &base()).unwrap(), "");
        assert_eq!(declared_value(&next, "color", &base()).unwrap(), "red");
        assert_eq!(
            assign_declared_value(&next, "margin", "", &base()).unwrap(),
            Mutation::Unchanged
        );
        let flex = changed(assign_declared_value("", "flex-flow", "column wrap", &base()).unwrap());
        assert_eq!(
            declared_value(&flex, "flex-direction", &base()).unwrap(),
            "column"
        );
        assert_eq!(declared_value(&flex, "flex-wrap", &base()).unwrap(), "wrap");
    }

    #[test]
    fn calc_stays_specified_and_its_serialization_round_trips() {
        let first = changed(assign_declared_value("", "width", "calc(2px + 3%)", &base()).unwrap());
        let value = declared_value(&first, "width", &base()).unwrap();
        assert!(value.starts_with("calc(") && value.ends_with(')'));
        let second = changed(assign_declared_value("", "width", &value, &base()).unwrap());
        assert_eq!(declared_value(&second, "width", &base()).unwrap(), value);
    }

    #[test]
    fn quoted_separators_and_custom_names_survive_full_list_parsing() {
        let next = changed(
            replace_declared_css_text(
                "color:green",
                "font-family:'a;b:c'; --Case:alpha; --case:beta; width:5px",
                &base(),
            )
            .unwrap(),
        );
        assert!(
            declared_value(&next, "font-family", &base())
                .unwrap()
                .contains("a;b:c")
        );
        assert_eq!(declared_value(&next, "--Case", &base()).unwrap(), "alpha");
        assert_eq!(declared_value(&next, "--case", &base()).unwrap(), "beta");
        assert_eq!(declared_value(&next, "width", &base()).unwrap(), "5px");
        assert_eq!(declared_value(&next, "color", &base()).unwrap(), "");
    }

    #[test]
    fn url_parsing_uses_real_base_without_fetching_or_computing() {
        use style::properties::PropertyDeclaration;
        use style::values::specified::image::Image;
        let raw = "background-image:url('../asset.png')";
        let url = UrlExtraData::from(base());
        let block = parse_current(raw, &url).unwrap();
        let id = enabled_css_name("background-image").unwrap().unwrap();
        let (PropertyDeclaration::BackgroundImage(images), _) =
            block.get(id.as_shorthand().unwrap_err()).unwrap()
        else {
            panic!("expected the specified background image");
        };
        let Image::Url(parsed) = &images.0[0] else {
            panic!("expected URL data, not a resource")
        };
        assert_eq!(
            parsed.url().unwrap().as_str(),
            "https://owned.example/asset.png"
        );
        assert!(
            declared_value(raw, "background-image", &base())
                .unwrap()
                .contains("../asset.png")
        );
    }

    #[test]
    fn oversize_is_a_real_error_even_for_unknown_or_invalid_css() {
        let large = "x".repeat(ATTRIBUTE_BYTES + 1);
        assert_eq!(
            enabled_css_name(&large),
            Err(DeclarationError::TooLarge(Part::CssName))
        );
        assert_eq!(
            declared_value(&large, "unknown-property", &base()),
            Err(DeclarationError::TooLarge(Part::CurrentStyle))
        );
        assert_eq!(
            assign_declared_value("color:red", "unknown-property", &large, &base()),
            Err(DeclarationError::TooLarge(Part::Value))
        );
        assert_eq!(
            replace_declared_css_text("color:red", &large, &base()),
            Err(DeclarationError::TooLarge(Part::CssText))
        );
        let exact_input = format!("--x:{}", "a".repeat(ATTRIBUTE_BYTES - 4));
        assert_eq!(exact_input.len(), ATTRIBUTE_BYTES);
        // Canonical ': ' and final ';' expand this admitted input; there is no
        // truncated replacement or independent post-canonical allowance.
        assert_eq!(
            replace_declared_css_text("color:red", &exact_input, &base()),
            Err(DeclarationError::TooLarge(Part::Output))
        );
    }

    #[test]
    fn no_op_preserves_raw_style_and_clear_replaces_the_full_list() {
        let raw = "color:red!important; width:7px";
        assert_eq!(
            assign_declared_value(raw, "unknown-property", "blue", &base()).unwrap(),
            Mutation::Unchanged
        );
        assert_eq!(
            assign_declared_value(raw, "width", "7px", &base()).unwrap(),
            Mutation::Unchanged
        );
        assert_eq!(
            replace_declared_css_text(raw, "", &base()).unwrap(),
            Mutation::Replace(String::new())
        );
        assert_eq!(
            replace_declared_css_text("", "", &base()).unwrap(),
            Mutation::Replace(String::new())
        );
    }

    #[test]
    fn container_query_dimensions_roundtrip_without_touching_strings_idents_or_surrogates() {
        let raw = changed(
            assign_declared_value(
                "",
                "flex",
                "calc(10 + (sign(20cqw - 10px) * 5)) calc(10 + (sign(20cqw - 10px) * 5)) 1px",
                &base(),
            )
            .unwrap(),
        );
        assert_eq!(
            declared_value(&raw, "flex", &base()).unwrap(),
            "calc(10 + (5 * sign(20cqw - 10px))) calc(10 + (5 * sign(20cqw - 10px))) 1px"
        );

        let raw2 = changed(
            assign_declared_value(
                "",
                "flex",
                "1 1 calc(10px + (sign(20cqw - 10px) * 5px))",
                &base(),
            )
            .unwrap(),
        );
        assert_eq!(
            declared_value(&raw2, "flex", &base()).unwrap(),
            "1 1 calc(10px + (5px * sign(20cqw - 10px)))"
        );

        // Collision avoidance when dvw/dvh are authored alongside cqw/cqh/cqi/cqb/cqmin/cqmax.
        let mixed = changed(
            replace_declared_css_text(
                "",
                "width: calc(10dvw + 5cqw + 2cqh); height: calc(3dvh + 4cqi + 1cqb + 2cqmin + 3cqmax); content: \"20cqw 10dvw\"; font-family: cqw; --raw: 20CQW;",
                &base(),
            )
            .unwrap(),
        );
        assert_eq!(
            declared_value(&mixed, "width", &base()).unwrap(),
            "calc(2cqh + 10dvw + 5cqw)"
        );
        assert_eq!(
            declared_value(&mixed, "height", &base()).unwrap(),
            "calc(1cqb + 3dvh + 4cqi + 3cqmax + 2cqmin)"
        );
        assert_eq!(
            declared_value(&mixed, "content", &base()).unwrap(),
            "\"20cqw 10dvw\""
        );
        assert_eq!(
            declared_value(&mixed, "font-family", &base()).unwrap(),
            "cqw"
        );
        assert_eq!(declared_value(&mixed, "--raw", &base()).unwrap(), "20CQW");
    }
}
