//! Fixed first-Window-cohort source admission, not execution or outcome mapping.
//! The host separately verifies the frozen manifest and its aggregate input budget.
use mg_sparkle::{
    document::{self, Node},
    js_browser::{
        ClassicErrorVisibility, Request, classic_script_type, unsupported_external_attribute,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;

pub const MANIFEST_PATH: &str = "tools/wpt/window/corpus.json";
pub const MANIFEST_SHA: &str = "5223099747ed3b1669f311faa5007abb2c037bebb90aa7411b67c6470b4518d0";
pub const REVISION: &str = "84daed4ae966d9624bed22f43587c79283ad1fea";
const PREFIX: &str = "css/css-flexbox/parsing/";
const STYLE_SHA: &str = "39a2bf8324b868550c116c816cc3735ce0507475720bb1228869c92526affd7c";

/// Only failures established by this source validator. No resource/signal inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SourceError {
    UnknownCase,
    DocumentUrl,
    EmbeddedCase,
    EmbeddedHelper { index: u8 },
    DocumentBytes,
    SourceShape,
    InlineSource,
    SourceIdentity,
    HelperCount,
    HelperNode { index: u8 },
    HelperBytes { index: u8 },
    HelperProvenance { index: u8 },
}
impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fixed Window source admission: {self:?}")
    }
}
impl std::error::Error for SourceError {}
pub type Result<T> = std::result::Result<T, SourceError>;

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn require(value: bool, error: SourceError) -> Result<()> {
    if value { Ok(()) } else { Err(error) }
}

#[derive(Clone, Copy, Debug)]
pub struct HelperSpec {
    pub route: &'static str,
    pub body: &'static str,
    pub bytes: usize,
    pub sha256: &'static str,
}
pub static HELPERS: [HelperSpec; 3] = [
    HelperSpec {
        route: "/resources/testharness.js",
        body: include_str!("../../tools/wpt/upstream/resources/testharness.js"),
        bytes: 198_291,
        sha256: "d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e",
    },
    HelperSpec {
        route: "/resources/testharnessreport.js",
        body: include_str!("../../tools/wpt/upstream/resources/testharnessreport.js"),
        bytes: 1_231,
        sha256: "3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae",
    },
    HelperSpec {
        route: "/css/support/parsing-testcommon.js",
        body: include_str!("../../tools/wpt/upstream/css/support/parsing-testcommon.js"),
        bytes: 7_476,
        sha256: "ac482e1e1908e9b98f8f1c4b2c457ec77ae5ad2baa074205c4b0f7812f4024b5",
    },
];

#[derive(Clone, Copy, Debug)]
pub struct CaseSpec {
    path: &'static str,
    html: &'static str,
    bytes: usize,
    sha256: &'static str,
    inline_bytes: usize,
    inline_sha256: &'static str,
}
macro_rules! case {
    ($name:literal, $bytes:literal, $sha:literal, $inline_bytes:literal, $inline_sha:literal) => {
        CaseSpec {
            path: concat!("css/css-flexbox/parsing/", $name),
            html: include_str!(concat!(
                "../../tools/wpt/upstream/css/css-flexbox/parsing/",
                $name
            )),
            bytes: $bytes,
            sha256: $sha,
            inline_bytes: $inline_bytes,
            inline_sha256: $inline_sha,
        }
    };
}
pub static CASES: [CaseSpec; 16] = [
    case!(
        "flex-basis-invalid.html",
        968,
        "88737a191caeb2c79ee1c7c7d60686352933d5845f09bfc953b948d24cdac54e",
        346,
        "5aad30564f6d689a385c5ef768acca5888080045b819cc6ee6c251b66cc2cbbd"
    ),
    case!(
        "flex-basis-valid.html",
        975,
        "65f306ad53ebf9f7445a72015847aa84d2b15989023767da7ef203ff31598c2a",
        355,
        "80b23209e375e4e100c7191a8c48cd6e79ce288085329220ff50f901c4ffdad9"
    ),
    case!(
        "flex-direction-invalid.html",
        761,
        "903879c5d0bb7386cc5e801866485030cfa741bd94ca2fcc57b9a7335c46b63c",
        107,
        "c909bfbb7e98e24fb59486d066a6e39e7d1cd5c6f5cd4dfae300d9d8dc1634a2"
    ),
    case!(
        "flex-direction-valid.html",
        847,
        "7e36311e01e8b05c82710ae6382e3df311924d4514f72d1d3de28135945b8be8",
        195,
        "7919cdf2f9b76133824b711864ae56d2175885a83e141380c113a97930dc7f0b"
    ),
    case!(
        "flex-flow-invalid.html",
        737,
        "e25fcc0408dbedebb60e0fcb1434291c56cfa6e49a637bcfa2a7875c98043ccf",
        110,
        "88cde79bb764f5ba57b04c48f52be92f1658cdf5e45683cd2bc5439921954394"
    ),
    case!(
        "flex-flow-valid.html",
        1011,
        "af3df354e79807a419aa61c267db486459fcf0aa0c054765434db9ea5d8554d0",
        386,
        "ea9e043c88ba0ba6a8cb6628c71cf02a30c9941ed12943fc89df79e864388fe0"
    ),
    case!(
        "flex-grow-invalid.html",
        788,
        "78767371b250c3dba6a5781a75cf38cb8c50b45322f3348625b5acebea0fd3e3",
        184,
        "a00a41efa4d2aff60cc418cf50559fad78b6a3318c3fb1de2b727ddd4d0375d1"
    ),
    case!(
        "flex-grow-valid.html",
        842,
        "2657cdab3d79719107375f502fb152a355c292b2b3b9cc30f770cb142d9afc87",
        240,
        "c20b86063defb3d111d1255f25ac505f7ee37d6f4f5a340b5e9109f0c137c221"
    ),
    case!(
        "flex-invalid.html",
        894,
        "a8435252ba1160cc68745196a545ce664a69bb8d5ab11bd1a109d8502829543e",
        242,
        "a0ac11dcc06e7b21032f194da3b44e2161397d8ffe58946baaec5317196ad428"
    ),
    case!(
        "flex-shrink-invalid.html",
        802,
        "b5fdeb7fafd82e7db1a3d8ab046d6722ff02618cf8f8c9f1e87337e7b88c4cf2",
        192,
        "304c9994287e983a59acbcac91b150c2e88defdbfaca49fa3ebd590db604dd18"
    ),
    case!(
        "flex-shrink-valid.html",
        856,
        "80b23e9da6688b46e4939ebd2cacef790e8851d12945e1799850dfbc34bc284c",
        248,
        "e40b626976c90c9cd4e73920c641e4c59bb2a6122c8be2b6572b12daad24c4a9"
    ),
    case!(
        "flex-valid.html",
        1961,
        "4ed455373cac980e07161567c4b5319bff7aa2130eb0ec6b6b94496d9ed1993b",
        1132,
        "b7b7c47f49a43da60225cdf83f8d10beb784b79f426dd67abb2ced5ba2c46ad3"
    ),
    case!(
        "flex-wrap-invalid.html",
        714,
        "5e58df5228f192c26fb78607b0e996755f06089e726a57dbbeba21557933a15c",
        90,
        "aa831d16c4ce63d67d5800a6961880c05fa509945c925a92b7b245dce607cfce"
    ),
    case!(
        "flex-wrap-valid.html",
        750,
        "2cb703d12112ee6d0de7b32e142ac0f226af8632a0bec54c3dc51b673ac43e56",
        128,
        "8bbe2fd3eea0f1159d5824227f4e4429fe8e5358480dc836692969ebe70c3232"
    ),
    case!(
        "order-invalid.html",
        709,
        "8cf56a42e92e06ec3b894319a9b02eb29d51fb1960089d9914f24cebf138b0f1",
        116,
        "02096f3c3e23da345f014a6cf9c8af884552df5eb8f0bf7cbacceaa997cae3d0"
    ),
    case!(
        "order-valid.html",
        692,
        "7eb000f1a7309d0e17f311793027e704c50f43d18183e752cac03cf049dd03bf",
        101,
        "18f6d756c80cb134ab8d9de550e425278ce59a4bc365c3788c6f738032282b59"
    ),
];

impl CaseSpec {
    pub fn by_path(path: &str) -> Result<&'static Self> {
        CASES
            .iter()
            .find(|case| case.path == path)
            .ok_or(SourceError::UnknownCase)
    }
    pub fn path(&self) -> &'static str {
        self.path
    }
    pub fn html(&self) -> &'static str {
        self.html
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn sha256(&self) -> &'static str {
        self.sha256
    }

    /// Uses the real parser only when this method is actually invoked by the host.
    pub fn source_plan(&self, owned_url: &str) -> Result<SourcePlan> {
        checked_url(self, owned_url)?;
        require(
            self.html.len() == self.bytes && hash(self.html.as_bytes()) == self.sha256,
            SourceError::EmbeddedCase,
        )?;
        for (index, helper) in HELPERS.iter().enumerate() {
            require(
                helper.body.len() == helper.bytes && hash(helper.body.as_bytes()) == helper.sha256,
                SourceError::EmbeddedHelper { index: index as u8 },
            )?;
        }
        derive_plan(self, self.html, owned_url)
    }
}

/// Arena identities, never the source checker's ordinal positions or guessed IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SourcePlan {
    pub harness_node: usize,
    pub vendor_node: usize,
    pub parsing_node: usize,
    pub inline_node: usize,
}
impl SourcePlan {
    pub fn external_nodes(self) -> [usize; 3] {
        [self.harness_node, self.vendor_node, self.parsing_node]
    }
}

fn checked_url(case: &CaseSpec, owned_url: &str) -> Result<url::Url> {
    let fixed = "http://127.0.0.1:".len() + 1 + case.path.len();
    require(
        owned_url.is_ascii() && (fixed + 1..=fixed + 5).contains(&owned_url.len()),
        SourceError::DocumentUrl,
    )?;
    let page = url::Url::parse(owned_url).map_err(|_| SourceError::DocumentUrl)?;
    let authority = owned_url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.split_once('/').map(|(authority, _)| authority))
        .ok_or(SourceError::DocumentUrl)?;
    let port = authority
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(SourceError::DocumentUrl)?;
    require(
        page.scheme() == "http"
            && page.host_str() == Some("127.0.0.1")
            && page.port_or_known_default() == Some(port)
            && page.username().is_empty()
            && page.password().is_none()
            && page.query().is_none()
            && page.fragment().is_none()
            && owned_url == format!("http://127.0.0.1:{port}/{}", case.path)
            && page.path() == format!("/{}", case.path)
            && page.as_str() == owned_url,
        SourceError::DocumentUrl,
    )?;
    Ok(page)
}

fn attrs(node: &Node, expected: &[(&str, &str)]) -> bool {
    node.attributes.len() == expected.len()
        && node
            .attributes
            .iter()
            .zip(expected)
            .all(|((key, value), (name, expected))| {
                key.as_str() == *name && value.as_str() == *expected
            })
}
fn parent_is(nodes: &[Node], index: usize, tag: &str) -> bool {
    let Some(node) = nodes.get(index) else {
        return false;
    };
    nodes
        .get(node.parent)
        .is_some_and(|parent| parent.tag == tag && parent.children.contains(&index))
}
fn active(nodes: &[Node], mut index: usize) -> bool {
    for _ in 0..=256 {
        let Some(node) = nodes.get(index) else {
            return false;
        };
        if matches!(node.tag.as_str(), "template" | "noscript") {
            return false;
        }
        if index == 0 {
            return node.tag == "#document" && node.parent == 0;
        }
        let Some(parent) = nodes.get(node.parent) else {
            return false;
        };
        if !parent.children.contains(&index) {
            return false;
        }
        index = node.parent;
    }
    false
}
fn single_text(nodes: &[Node], index: usize) -> Option<&str> {
    let node = nodes.get(index)?;
    if node.children.len() != 1 {
        return None;
    }
    let text = nodes.get(node.children[0])?;
    (text.tag == "#text"
        && text.parent == index
        && text.children.is_empty()
        && text.attributes.is_empty())
    .then_some(text.text.as_str())
}
fn derive_plan(case: &CaseSpec, html: &str, owned_url: &str) -> Result<SourcePlan> {
    let page = document::parse_with_scripting(html, owned_url, true);
    require(page.refresh.is_none(), SourceError::SourceShape)?;
    let mut scripts = [usize::MAX; 4];
    let (mut count, mut charset, mut assertion, mut author, mut help, mut styles, mut divs) =
        (0, 0, 0, 0, 0, 0, 0);
    let mut structural = [0; 4];
    let property = case
        .path
        .strip_prefix(PREFIX)
        .and_then(|name| {
            name.strip_suffix("-invalid.html")
                .or_else(|| name.strip_suffix("-valid.html"))
        })
        .ok_or(SourceError::UnknownCase)?;
    for (index, node) in page.nodes.iter().enumerate() {
        require(active(&page.nodes, index), SourceError::SourceShape)?;
        match node.tag.as_str() {
            "#document" => require(
                index == 0 && node.attributes.is_empty(),
                SourceError::SourceShape,
            )?,
            "#text" => {}
            tag @ ("html" | "head" | "body" | "title") => {
                let slot = match tag {
                    "html" => 0,
                    "head" => 1,
                    "body" => 2,
                    _ => 3,
                };
                structural[slot] += 1;
                let parent = match tag {
                    "html" => "#document",
                    "head" | "body" => "html",
                    _ => "head",
                };
                require(
                    node.attributes.is_empty() && parent_is(&page.nodes, index, parent),
                    SourceError::SourceShape,
                )?;
            }
            "meta" => {
                require(
                    parent_is(&page.nodes, index, "head"),
                    SourceError::SourceShape,
                )?;
                if attrs(node, &[("charset", "utf-8")]) {
                    charset += 1;
                } else if node.attributes.len() == 2
                    && node.attributes[0].0 == "name"
                    && node.attributes[0].1 == "assert"
                    && node.attributes[1].0 == "content"
                    && !node.attributes[1].1.is_empty()
                {
                    assertion += 1;
                } else {
                    return Err(SourceError::SourceShape);
                }
            }
            "link" => {
                require(
                    parent_is(&page.nodes, index, "head"),
                    SourceError::SourceShape,
                )?;
                if attrs(
                    node,
                    &[
                        ("rel", "author"),
                        ("title", "Eric Willigers"),
                        ("href", "mailto:ericwilligers@chromium.org"),
                    ],
                ) {
                    author += 1;
                } else if attrs(
                    node,
                    &[
                        ("rel", "help"),
                        (
                            "href",
                            &format!("https://drafts.csswg.org/css-flexbox/#propdef-{property}"),
                        ),
                    ],
                ) {
                    help += 1;
                } else {
                    return Err(SourceError::SourceShape);
                }
            }
            "script" => {
                require(count < 4, SourceError::SourceShape)?;
                scripts[count] = index;
                require(
                    parent_is(&page.nodes, index, if count < 3 { "head" } else { "body" }),
                    SourceError::SourceShape,
                )?;
                if count < 3 {
                    require(
                        attrs(node, &[("src", HELPERS[count].route)])
                            && node.children.is_empty()
                            && classic_script_type(node.attr("type").unwrap_or(""))
                            && unsupported_external_attribute(node).is_none()
                            && !node.has("defer"),
                        SourceError::SourceShape,
                    )?;
                } else {
                    require(node.attributes.is_empty(), SourceError::InlineSource)?;
                    let text = single_text(&page.nodes, index).ok_or(SourceError::InlineSource)?;
                    require(
                        text.len() == case.inline_bytes
                            && text.starts_with('\n')
                            && text.ends_with('\n')
                            && hash(text.as_bytes()) == case.inline_sha256,
                        SourceError::InlineSource,
                    )?;
                }
                count += 1;
            }
            "style" => {
                require(
                    case.path.ends_with("/flex-valid.html")
                        && node.attributes.is_empty()
                        && parent_is(&page.nodes, index, "head"),
                    SourceError::SourceShape,
                )?;
                let text = single_text(&page.nodes, index).ok_or(SourceError::SourceShape)?;
                require(
                    text.len() == 107 && hash(text.as_bytes()) == STYLE_SHA,
                    SourceError::SourceShape,
                )?;
                styles += 1;
            }
            "div" => {
                require(
                    case.path.ends_with("/flex-valid.html"),
                    SourceError::SourceShape,
                )?;
                match divs {
                    0 => require(
                        attrs(node, &[("id", "container")])
                            && parent_is(&page.nodes, index, "body"),
                        SourceError::SourceShape,
                    )?,
                    1 => require(
                        attrs(node, &[("id", "target")])
                            && parent_is(&page.nodes, index, "div")
                            && page.nodes[node.parent].attr("id") == Some("container"),
                        SourceError::SourceShape,
                    )?,
                    _ => return Err(SourceError::SourceShape),
                }
                divs += 1;
            }
            _ => return Err(SourceError::SourceShape),
        }
    }
    let flex = case.path.ends_with("/flex-valid.html");
    require(
        count == 4
            && structural == [1; 4]
            && charset == 1
            && assertion == 1
            && author == 1
            && help == 1
            && styles == usize::from(flex)
            && divs == if flex { 2 } else { 0 },
        SourceError::SourceShape,
    )?;
    Ok(SourcePlan {
        harness_node: scripts[0],
        vendor_node: scripts[1],
        parsing_node: scripts[2],
        inline_node: scripts[3],
    })
}

pub struct SourceInventory {
    case: &'static CaseSpec,
    owned_url: String,
    page: url::Url,
    plan: SourcePlan,
}
impl SourceInventory {
    pub fn new(case: &'static CaseSpec, owned_url: &str) -> Result<Self> {
        let page = checked_url(case, owned_url)?;
        let plan = case.source_plan(owned_url)?;
        Ok(Self {
            case,
            owned_url: owned_url.to_owned(),
            page,
            plan,
        })
    }
    pub fn case(&self) -> &'static CaseSpec {
        self.case
    }
    pub fn document_url(&self) -> &str {
        &self.owned_url
    }
    pub fn plan(&self) -> SourcePlan {
        self.plan
    }
    pub fn helper_url(&self, index: usize) -> Option<String> {
        HELPERS.get(index).map(|helper| {
            format!(
                "{}{}",
                self.page.origin().ascii_serialization(),
                helper.route
            )
        })
    }

    /// Call before forwarding to the real LinuxScripts::start; no JS is run here.
    pub fn verify(&self, request: &Request) -> Result<()> {
        require(request.url == self.owned_url, SourceError::DocumentUrl)?;
        checked_url(self.case, &request.url)?;
        require(
            request.html.len() == self.case.bytes
                && request.html == self.case.html
                && hash(request.html.as_bytes()) == self.case.sha256,
            SourceError::DocumentBytes,
        )?;
        require(
            derive_plan(self.case, &request.html, &request.url)? == self.plan,
            SourceError::SourceIdentity,
        )?;
        let sources = request
            .external_scripts
            .as_ref()
            .ok_or(SourceError::HelperCount)?;
        require(sources.len() == 3, SourceError::HelperCount)?;
        for (index, ((source, expected), node)) in sources
            .iter()
            .zip(HELPERS.iter())
            .zip(self.plan.external_nodes())
            .enumerate()
        {
            require(
                source.node == node,
                SourceError::HelperNode { index: index as u8 },
            )?;
            require(
                source.source.len() == expected.bytes
                    && source.source == expected.body
                    && hash(source.source.as_bytes()) == expected.sha256,
                SourceError::HelperBytes { index: index as u8 },
            )?;
            let error = SourceError::HelperProvenance { index: index as u8 };
            let provenance = source.provenance.as_ref().ok_or(error)?;
            require(
                Some(provenance.response_url.as_str()) == self.helper_url(index).as_deref(),
                error,
            )?;
            let response = url::Url::parse(&provenance.response_url).map_err(|_| error)?;
            require(
                provenance.error_visibility == ClassicErrorVisibility::SameOrigin
                    && response.origin() == self.page.origin()
                    && response.username().is_empty()
                    && response.password().is_none()
                    && response.query().is_none()
                    && response.fragment().is_none(),
                error,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::js_browser::{ClassicSourceProvenance, ExternalScriptSource};

    fn url(case: &CaseSpec) -> String {
        format!("http://127.0.0.1:38545/{}", case.path())
    }
    fn request(inventory: &SourceInventory) -> Request {
        Request {
            url: inventory.document_url().into(),
            html: inventory.case().html().into(),
            external_scripts: Some(
                HELPERS
                    .iter()
                    .zip(inventory.plan().external_nodes())
                    .enumerate()
                    .map(|(index, (helper, node))| ExternalScriptSource {
                        node,
                        source: helper.body.into(),
                        provenance: Some(ClassicSourceProvenance {
                            response_url: inventory.helper_url(index).unwrap(),
                            error_visibility: ClassicErrorVisibility::SameOrigin,
                        }),
                    })
                    .collect(),
            ),
        }
    }
    #[test]
    fn all_fixed_cases_bind_original_bytes_and_four_actual_parser_identities() {
        assert_eq!(CASES.len(), 16);
        for pair in CASES.windows(2) {
            assert!(pair[0].path() < pair[1].path());
        }
        for case in &CASES {
            assert_eq!(
                CaseSpec::by_path(case.path()).unwrap().sha256(),
                case.sha256()
            );
            let inventory = SourceInventory::new(case, &url(case)).unwrap();
            let plan = inventory.plan();
            assert!(
                plan.harness_node < plan.vendor_node
                    && plan.vendor_node < plan.parsing_node
                    && plan.parsing_node < plan.inline_node
            );
            let page = document::parse_with_scripting(case.html(), &url(case), true);
            for (index, node) in plan.external_nodes().iter().enumerate() {
                assert_eq!(page.nodes[*node].attr("src"), Some(HELPERS[index].route));
            }
            assert_eq!(
                single_text(&page.nodes, plan.inline_node).unwrap().len(),
                case.inline_bytes
            );
            inventory.verify(&request(&inventory)).unwrap();
        }
    }
    #[test]
    fn unknown_cases_and_document_url_aliases_reject() {
        for path in [
            "flex-valid.html",
            "/css/css-flexbox/parsing/flex-valid.html",
            "css/css-flexbox/parsing/flex-computed.html",
            "css/css-flexbox/parsing/flex-valid.html?x",
        ] {
            assert!(matches!(
                CaseSpec::by_path(path),
                Err(SourceError::UnknownCase)
            ));
        }
        let case = &CASES[0];
        for prefix in [
            "https://127.0.0.1:38545/",
            "http://localhost:38545/",
            "http://127.1:38545/",
            "http://127.0.0.1/",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:80/",
            "http://127.0.0.1:038545/",
            "http://user@127.0.0.1:38545/",
        ] {
            assert_eq!(
                case.source_plan(&format!("{prefix}{}", case.path())),
                Err(SourceError::DocumentUrl)
            );
        }
        for suffix in ["?x", "#fragment", "/", "%2f"] {
            assert_eq!(
                case.source_plan(&(url(case) + suffix)),
                Err(SourceError::DocumentUrl)
            );
        }
        assert_eq!(
            case.source_plan(&url(case).replace("/parsing/", "/parsing/../parsing/")),
            Err(SourceError::DocumentUrl)
        );
    }
    #[test]
    fn embedded_html_and_inline_pins_are_not_optional() {
        let mut case = CASES[0];
        case.bytes += 1;
        assert_eq!(
            case.source_plan(&url(&case)),
            Err(SourceError::EmbeddedCase)
        );
        case = CASES[0];
        case.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            case.source_plan(&url(&case)),
            Err(SourceError::EmbeddedCase)
        );
        case = CASES[0];
        case.inline_sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            case.source_plan(&url(&case)),
            Err(SourceError::InlineSource)
        );
    }
    #[test]
    fn actual_document_and_url_are_exact_before_worker_start() {
        let case = &CASES[11];
        let inventory = SourceInventory::new(case, &url(case)).unwrap();
        let mut actual = request(&inventory);
        actual.html = actual.html.replace("width: 10px", "width: 11px");
        assert_eq!(inventory.verify(&actual), Err(SourceError::DocumentBytes));
        actual = request(&inventory);
        actual.html.push('\n');
        assert_eq!(inventory.verify(&actual), Err(SourceError::DocumentBytes));
        actual = request(&inventory);
        actual.url.push_str("?changed");
        assert_eq!(inventory.verify(&actual), Err(SourceError::DocumentUrl));
    }
    #[test]
    fn missing_extra_reordered_and_wrong_actual_source_nodes_reject() {
        let case = &CASES[0];
        let inventory = SourceInventory::new(case, &url(case)).unwrap();
        let mut actual = request(&inventory);
        actual.external_scripts = None;
        assert_eq!(inventory.verify(&actual), Err(SourceError::HelperCount));
        actual = request(&inventory);
        actual.external_scripts.as_mut().unwrap().pop();
        assert_eq!(inventory.verify(&actual), Err(SourceError::HelperCount));
        actual = request(&inventory);
        let extra = ExternalScriptSource {
            node: inventory.plan().harness_node,
            source: HELPERS[0].body.into(),
            provenance: Some(ClassicSourceProvenance {
                response_url: inventory.helper_url(0).unwrap(),
                error_visibility: ClassicErrorVisibility::SameOrigin,
            }),
        };
        actual.external_scripts.as_mut().unwrap().push(extra);
        assert_eq!(inventory.verify(&actual), Err(SourceError::HelperCount));
        actual = request(&inventory);
        actual.external_scripts.as_mut().unwrap().swap(0, 2);
        assert_eq!(
            inventory.verify(&actual),
            Err(SourceError::HelperNode { index: 0 })
        );
        for index in 0..3 {
            actual = request(&inventory);
            actual.external_scripts.as_mut().unwrap()[index].node = inventory.plan().inline_node;
            assert_eq!(
                inventory.verify(&actual),
                Err(SourceError::HelperNode { index: index as u8 })
            );
        }
    }
    #[test]
    fn all_complete_helper_bodies_including_third_are_pinned() {
        let case = &CASES[0];
        let inventory = SourceInventory::new(case, &url(case)).unwrap();
        for index in 0..3 {
            let mut actual = request(&inventory);
            actual.external_scripts.as_mut().unwrap()[index]
                .source
                .replace_range(0..1, "!");
            assert_eq!(
                inventory.verify(&actual),
                Err(SourceError::HelperBytes { index: index as u8 })
            );
        }
    }
    #[test]
    fn every_actual_helper_requires_exact_trusted_response_provenance() {
        let case = &CASES[0];
        let inventory = SourceInventory::new(case, &url(case)).unwrap();
        for index in 0..3 {
            for mutation in 0..8 {
                let mut actual = request(&inventory);
                let source = &mut actual.external_scripts.as_mut().unwrap()[index];
                if mutation == 0 {
                    source.provenance = None;
                } else {
                    let provenance = source.provenance.as_mut().unwrap();
                    match mutation {
                        1 => provenance.error_visibility = ClassicErrorVisibility::Muted,
                        2 => provenance.response_url.push_str("?query"),
                        3 => provenance.response_url.push_str("#fragment"),
                        4 => {
                            provenance.response_url =
                                provenance.response_url.replace(":38545", ":38546")
                        }
                        5 => {
                            provenance.response_url =
                                provenance.response_url.replace("127.0.0.1", "localhost")
                        }
                        6 => {
                            provenance.response_url =
                                provenance.response_url.replace("http://", "http://user@")
                        }
                        _ => {
                            provenance.response_url = provenance
                                .response_url
                                .replace("/resources/", "/resources/../resources/")
                                .replace("/css/", "/css/../css/")
                        }
                    }
                }
                assert_eq!(
                    inventory.verify(&actual),
                    Err(SourceError::HelperProvenance { index: index as u8 })
                );
            }
        }
    }
    #[test]
    fn structural_guards_reject_changed_order_attributes_ancestry_and_inline_lf() {
        let case = &CASES[0];
        for (before, after) in [
            ("<script src=", "<script defer src="),
            ("<script src=", "<script async src="),
            ("<script src=", "<script type=\"module\" src="),
            ("<script src=", "<script crossorigin src="),
            ("<script src=", "<script integrity=\"x\" src="),
            ("<script src=", "<template><script src="),
            ("<script>\n", "<script>"),
            ("<script>\n", "<script>\n\n"),
            ("<body>", "<body><script>unexpected()</script>"),
            (
                "/resources/testharness.js",
                "/resources/testharnessreport.js",
            ),
            ("<head>", "<head><base href=\"http://elsewhere/\">"),
            (
                "<meta charset=\"utf-8\">",
                "<meta http-equiv=\"refresh\" content=\"0;url=/trap\">",
            ),
        ] {
            let changed = case.html().replacen(before, after, 1);
            assert!(derive_plan(case, &changed, &url(case)).is_err(), "{after}");
        }
        let inventory = SourceInventory::new(case, &url(case)).unwrap();
        // The parser creates separate text nodes on each side of this ignored
        // comment; adding whitespace to one existing text run would not shift IDs.
        let shifted = case
            .html()
            .replace("<title>", "<!--identity split-->\n<title>");
        let shifted_plan = derive_plan(case, &shifted, &url(case)).unwrap();
        assert_ne!(
            inventory.plan(),
            shifted_plan,
            "Actual parser identities must not be source ordinals"
        );
        let mut actual = request(&inventory);
        actual.html = shifted;
        assert_eq!(inventory.verify(&actual), Err(SourceError::DocumentBytes));
    }
}
