//! Specified declaration parsing for modern DOM bindings, with no JS or IO.
//!
//! All parser/atom/serializer temporaries remain inside the existing worker
//! System/CPU/active containment. Stylo's Servo writer is a concrete String:
//! the output bound below is post-serialization, not a pre-allocation quota.
//! Before transferring Replace(String) into BrowserHost::attr, the bridge must
//! admit its actual capacity excess over the len that attr already charges.
//! For reads it must separately admit serializer String capacity and the actual
//! UTF-16 result Vec capacity. No independent allowance or fatal latch lives here.

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

fn parse_current(
    raw: &str,
    url: &UrlExtraData,
) -> Result<PropertyDeclarationBlock, DeclarationError> {
    bounded(raw, Part::CurrentStyle)?;
    Ok(parse_style_attribute(
        raw,
        url,
        None,
        QuirksMode::NoQuirks,
        CssRuleType::Style,
    ))
}

fn serialize(block: &PropertyDeclarationBlock) -> Result<String, DeclarationError> {
    let mut text = String::new();
    block
        .to_css(&mut text)
        .map_err(|_| DeclarationError::Serialization)?;
    bounded(&text, Part::Output)?;
    Ok(text)
}

fn replacement(raw: &str, block: &PropertyDeclarationBlock) -> Result<Mutation, DeclarationError> {
    let text = serialize(block)?;
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
    let block = parse_current(raw, &url)?;
    specified_value(&block, &id)
}

fn specified_value(
    block: &PropertyDeclarationBlock,
    id: &PropertyId,
) -> Result<String, DeclarationError> {
    let mut text = String::new();
    block
        .property_value_to_css(id, &mut text)
        .map_err(|_| DeclarationError::Serialization)?;
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
    let block = parse_current(raw, &url)?;
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
    let mut block = parse_current(raw, &url)?;
    let text = specified_value(&block, &id)?;
    let Some(first) = block.first_declaration_to_remove(&id) else {
        return Ok((text, Mutation::Unchanged));
    };
    block.remove_property(&id, first);
    Ok((text, replacement(raw, &block)?))
}

/// A read never rewrites the original raw attribute, including malformed input.
pub(crate) fn declared_css_text(raw: &str, base: &Url) -> Result<String, DeclarationError> {
    let url = UrlExtraData::from(base.clone());
    serialize(&parse_current(raw, &url)?)
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
        let mut block = parse_current(raw, &url)?;
        let Some(first) = block.first_declaration_to_remove(&id) else {
            return Ok(Mutation::Unchanged);
        };
        block.remove_property(&id, first);
        return replacement(raw, &block);
    }
    let importance = if priority.is_empty() {
        Importance::Normal
    } else if priority.eq_ignore_ascii_case("important") {
        Importance::Important
    } else {
        return Ok(Mutation::Unchanged);
    };
    let mut parsed = SourcePropertyDeclaration::default();
    if parse_one_declaration_into(
        &mut parsed,
        id,
        value,
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
    let mut block = parse_current(raw, &url)?;
    let mut updates = SourcePropertyDeclarationUpdate::default();
    if !block.prepare_for_update(&parsed, importance, &mut updates) {
        return Ok(Mutation::Unchanged);
    }
    // extend/push would implement parser cascade precedence and incorrectly
    // preserve an earlier important declaration. CSSOM assignment replaces it.
    block.update(parsed.drain(), importance, &mut updates);
    replacement(raw, &block)
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
    let block = parse_style_attribute(input, &url, None, QuirksMode::NoQuirks, CssRuleType::Style);
    // cssText assignment updates its owner even when serialization is equal,
    // including creating style="" on an element whose attribute was absent.
    Ok(Mutation::Replace(serialize(&block)?))
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
}
