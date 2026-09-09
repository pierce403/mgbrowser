//! Treat the child arena as untrusted structured input, not serialized authority.
use mg_sparkle::document::{self, Node};

const URL: &str = "https://example.test/original";

fn arena() -> Vec<Node> {
    document::parse("<html><head><title>Live</title></head><body><form id='form'><input name='q'></form><a href='/real'>Real</a></body></html>", URL).nodes
}
fn id(nodes: &[Node], tag: &str) -> usize {
    nodes.iter().position(|node| node.tag == tag).unwrap()
}

#[test]
fn projection_preserves_arena_and_maps_forms_and_items_to_original_nodes() {
    let nodes = arena();
    let doc = document::project_nodes(nodes.clone(), URL).unwrap();
    assert_eq!(doc.nodes, nodes);
    assert_eq!(doc.form_nodes, vec![id(&nodes, "form")]);
    assert_eq!(doc.items.len(), doc.item_nodes.len());
    assert!(doc.item_nodes.contains(&id(&nodes, "input")));
    assert!(doc.item_nodes.contains(&id(&nodes, "a")));
}

#[test]
fn detached_subtrees_remain_addressable_but_cannot_supply_forms_or_title() {
    let mut nodes = arena();
    let form = id(&nodes, "form");
    let parent = nodes[form].parent;
    nodes[parent].children.retain(|&child| child != form);
    nodes[form].parent = form;
    let doc = document::project_nodes(nodes.clone(), URL).unwrap();
    assert_eq!(doc.nodes, nodes);
    assert!(doc.forms.is_empty());
    assert_eq!(doc.title, "Live");
    assert!(!doc.item_nodes.contains(&id(&nodes, "input")));
}

#[test]
fn malformed_links_and_cycles_are_rejected_without_repair() {
    for mutation in 0..7 {
        let mut nodes = arena();
        let body = id(&nodes, "body");
        let form = id(&nodes, "form");
        let input = id(&nodes, "input");
        match mutation {
            0 => nodes[0].children.push(usize::MAX),
            1 => nodes[body].children.push(form),
            2 => nodes[form].parent = input,
            3 => nodes[input].children.push(body),
            4 => nodes[body].children.retain(|&n| n != form),
            5 => {
                nodes[0].children.push(0);
            }
            6 => {
                nodes[body].children.retain(|&n| n != form);
                nodes[form].parent = input;
                nodes[input].children.push(form);
            }
            _ => unreachable!(),
        }
        assert!(
            document::project_nodes(nodes, URL).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn invalid_node_kinds_attributes_and_source_bounds_are_rejected() {
    for mutation in 0..7 {
        let mut nodes = arena();
        let input = id(&nodes, "input");
        match mutation {
            0 => nodes[input].tag = "#document".into(),
            1 => nodes[input].tag = "bad<tag".into(),
            2 => nodes[input].text = "element must not carry text payload".into(),
            3 => nodes[input]
                .attributes
                .push(("name".into(), "duplicate".into())),
            4 => nodes[input].attributes.push(("bad key".into(), "x".into())),
            5 => nodes[input]
                .attributes
                .push(("value".into(), "x".repeat(16_385))),
            6 => nodes[0]
                .attributes
                .push(("authority".into(), "wrong".into())),
            _ => unreachable!(),
        }
        assert!(
            document::project_nodes(nodes, URL).is_err(),
            "mutation {mutation}"
        );
    }
    assert!(document::project_nodes(vec![], URL).is_err());
}

#[test]
fn detached_depth_is_also_bounded() {
    let mut nodes = arena();
    let start = nodes.len();
    for offset in 0..258 {
        let current = start + offset;
        nodes.push(Node {
            tag: "div".into(),
            attributes: vec![],
            text: String::new(),
            parent: if offset == 0 { current } else { current - 1 },
            children: vec![],
        });
        if offset > 0 {
            nodes[current - 1].children.push(current);
        }
    }
    assert!(document::project_nodes(nodes, URL).is_err());
}

#[test]
fn moved_base_and_title_follow_tree_order_without_renumbering() {
    let mut nodes = document::parse("<html><head><base href='/one/'><base href='/two/'><title>First</title><title>Second</title></head><body><a href='result'>Result</a></body></html>", URL).nodes;
    let head = id(&nodes, "head");
    nodes[head].children.reverse();
    let doc = document::project_nodes(nodes.clone(), URL).unwrap();
    assert_eq!(doc.nodes, nodes);
    assert_eq!(doc.base_url, "https://example.test/two/");
    assert_eq!(doc.title, "Second");
}
