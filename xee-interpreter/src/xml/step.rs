use xot::{ValueType, Xot};
use xot::xmlname::NameStrInfo;

use ahash::HashMap;
use xee_xpath_ast::ast;

use crate::sequence;

use super::kind_test::kind_test;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub axis: ast::Axis,
    pub node_test: ast::NodeTest,
}

pub(crate) fn resolve_step(
    step: &Step,
    node: xot::Node,
    xot: &mut Xot,
    namespace_parents: &mut HashMap<xot::Node, xot::Node>,
) -> sequence::Sequence {
    if matches!(step.axis, ast::Axis::Namespace) {
        return resolve_namespace_step(step, node, xot, namespace_parents);
    }
    // For namespace nodes, handle parent/ancestor axes via our mapping
    if matches!(xot.value(node), xot::Value::Namespace(_)) {
        return resolve_step_from_namespace_node(step, node, xot, namespace_parents);
    }
    let mut new_items = Vec::new();
    for axis_node in node_take_axis(&step.axis, xot, node) {
        if node_test(&step.node_test, &step.axis, xot, axis_node) {
            new_items.push(sequence::Item::Node(axis_node));
        }
    }
    new_items.into()
}

fn resolve_step_from_namespace_node(
    step: &Step,
    ns_node: xot::Node,
    xot: &mut Xot,
    namespace_parents: &HashMap<xot::Node, xot::Node>,
) -> sequence::Sequence {
    let parent = namespace_parents.get(&ns_node).copied();
    match step.axis {
        ast::Axis::Parent => {
            if let Some(parent) = parent {
                if node_test(&step.node_test, &step.axis, xot, parent) {
                    return vec![sequence::Item::Node(parent)].into();
                }
            }
            sequence::Sequence::default()
        }
        ast::Axis::Ancestor => {
            let mut items = Vec::new();
            if let Some(parent) = parent {
                for anc in xot.ancestors(parent) {
                    if node_test(&step.node_test, &step.axis, xot, anc) {
                        items.push(sequence::Item::Node(anc));
                    }
                }
            }
            // xot.ancestors returns nearest-first; XPath needs document order
            items.reverse();
            items.into()
        }
        ast::Axis::AncestorOrSelf => {
            let mut items = Vec::new();
            if let Some(parent) = parent {
                for anc in xot.ancestors(parent) {
                    if node_test(&step.node_test, &step.axis, xot, anc) {
                        items.push(sequence::Item::Node(anc));
                    }
                }
            }
            // xot.ancestors returns nearest-first; XPath needs document order
            items.reverse();
            // self goes after ancestors in document order
            if node_test(&step.node_test, &step.axis, xot, ns_node) {
                items.push(sequence::Item::Node(ns_node));
            }
            items.into()
        }
        ast::Axis::Self_ => {
            if node_test(&step.node_test, &step.axis, xot, ns_node) {
                vec![sequence::Item::Node(ns_node)].into()
            } else {
                sequence::Sequence::default()
            }
        }
        // Following/preceding from namespace node: delegate to parent element
        ast::Axis::Following | ast::Axis::FollowingSibling
        | ast::Axis::Preceding | ast::Axis::PrecedingSibling => {
            if let Some(parent) = parent {
                let mut items = Vec::new();
                for axis_node in node_take_axis(&step.axis, xot, parent) {
                    if node_test(&step.node_test, &step.axis, xot, axis_node) {
                        items.push(sequence::Item::Node(axis_node));
                    }
                }
                // xot returns reverse axes (Preceding, PrecedingSibling) in
                // reverse document order; XPath 2.0+ needs document order
                if matches!(step.axis, ast::Axis::Preceding | ast::Axis::PrecedingSibling) {
                    items.reverse();
                }
                items.into()
            } else {
                sequence::Sequence::default()
            }
        }
        // Namespace nodes don't have children, attributes, descendants, etc.
        _ => sequence::Sequence::default(),
    }
}

fn resolve_namespace_step(
    step: &Step,
    node: xot::Node,
    xot: &mut Xot,
    namespace_parents: &mut HashMap<xot::Node, xot::Node>,
) -> sequence::Sequence {
    // Namespace axis only applies to elements
    if xot.value_type(node) != ValueType::Element {
        return sequence::Sequence::default();
    }
    let ns_bindings: Vec<_> = xot.namespaces_in_scope(node).collect();
    let mut new_items = Vec::new();
    for (prefix_id, namespace_id) in ns_bindings {
        let ns_node = xot.new_namespace_node(prefix_id, namespace_id);
        // Track parent so parent/ancestor axes work from namespace nodes
        namespace_parents.insert(ns_node, node);
        if node_test(&step.node_test, &step.axis, xot, ns_node) {
            new_items.push(sequence::Item::Node(ns_node));
        }
    }
    new_items.into()
}

fn convert_axis(axis: &ast::Axis) -> xot::Axis {
    match axis {
        ast::Axis::Child => xot::Axis::Child,
        ast::Axis::Descendant => xot::Axis::Descendant,
        ast::Axis::Parent => xot::Axis::Parent,
        ast::Axis::Ancestor => xot::Axis::Ancestor,
        ast::Axis::FollowingSibling => xot::Axis::FollowingSibling,
        ast::Axis::PrecedingSibling => xot::Axis::PrecedingSibling,
        ast::Axis::Following => xot::Axis::Following,
        ast::Axis::Preceding => xot::Axis::Preceding,
        ast::Axis::DescendantOrSelf => xot::Axis::DescendantOrSelf,
        ast::Axis::AncestorOrSelf => xot::Axis::AncestorOrSelf,
        ast::Axis::Self_ => xot::Axis::Self_,
        ast::Axis::Attribute => xot::Axis::Attribute,
        ast::Axis::Namespace => unreachable!("Namespace axis handled in resolve_namespace_step"),
    }
}

fn node_take_axis<'a>(
    axis: &ast::Axis,
    xot: &'a Xot,
    node: xot::Node,
) -> Box<dyn Iterator<Item = xot::Node> + 'a> {
    let axis = convert_axis(axis);
    xot.axis(axis, node)
}

fn node_test(node_test: &ast::NodeTest, axis: &ast::Axis, xot: &Xot, node: xot::Node) -> bool {
    match node_test {
        ast::NodeTest::KindTest(kt) => kind_test(kt, xot, node),
        ast::NodeTest::NameTest(name_test) => {
            if xot.value_type(node) != principal_node_kind(axis) {
                return false;
            }
            match name_test {
                ast::NameTest::Name(name) => {
                    // For namespace axis, compare local name directly to prefix
                    if let xot::Value::Namespace(n) = xot.value(node) {
                        let local_name = name.value.local_name();
                        return xot.prefix_str(n.prefix()) == local_name;
                    }
                    let name = name.value.maybe_to_ref(xot);
                    if let Some(name) = name {
                        let name_id = name.name_id();
                        match xot.value(node) {
                            xot::Value::Element(element) => element.name() == name_id,
                            xot::Value::Attribute(attribute) => attribute.name() == name_id,
                            _ => false,
                        }
                    } else {
                        // if name isn't present in any XML document it's certainly
                        // false
                        false
                    }
                }
                ast::NameTest::Star => true,
                ast::NameTest::LocalName(local_name) => match xot.value(node) {
                    xot::Value::Element(element) => {
                        let name_id = element.name();
                        let (name_str, _) = xot.name_ns_str(name_id);
                        name_str == local_name
                    }
                    xot::Value::Attribute(attribute) => {
                        xot.local_name_str(attribute.name()) == local_name
                    }
                    xot::Value::Namespace(n) => {
                        xot.prefix_str(n.prefix()) == local_name.as_str()
                    }
                    _ => false,
                },
                ast::NameTest::Namespace(uri) => match xot.value(node) {
                    xot::Value::Element(element) => {
                        let name_id = element.name();
                        let namespace_str = xot.uri_str(name_id);
                        namespace_str == uri
                    }
                    xot::Value::Attribute(attribute) => {
                        let namespace_str = xot.uri_str(attribute.name());
                        namespace_str == uri
                    }
                    xot::Value::Namespace(n) => {
                        xot.namespace_str(n.namespace()) == uri.as_str()
                    }
                    _ => false,
                },
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalNodeKind {
    Element,
    Attribute,
    Namespace,
}

fn principal_node_kind(axis: &ast::Axis) -> ValueType {
    match axis {
        ast::Axis::Attribute => ValueType::Attribute,
        ast::Axis::Namespace => ValueType::Namespace,
        _ => ValueType::Element,
    }
}

#[cfg(test)]
mod tests {
    use ahash::HashMapExt;
    use xee_xpath_ast::{ast, WithSpan};

    use super::*;

    fn xot_nodes_to_value(node: &[xot::Node]) -> sequence::Sequence {
        node.iter()
            .map(|&node| sequence::Item::Node(node))
            .collect::<Vec<_>>()
            .into()
    }

    #[test]
    fn test_child_axis_star() -> Result<(), xot::Error> {
        let mut xot = Xot::new();
        let doc = xot.parse(r#"<root><a/><b/></root>"#).unwrap();
        let doc_el = xot.document_element(doc)?;
        let a = xot.first_child(doc_el).unwrap();
        let b = xot.next_sibling(a).unwrap();

        let step = Step {
            axis: ast::Axis::Child,
            node_test: ast::NodeTest::NameTest(ast::NameTest::Star),
        };
        let value = resolve_step(&step, doc_el, &mut xot, &mut HashMap::new());
        assert_eq!(value, xot_nodes_to_value(&[a, b]));
        Ok(())
    }

    #[test]
    fn test_child_axis_name() -> Result<(), xot::Error> {
        let mut xot = Xot::new();
        let doc = xot.parse(r#"<root><a/><b/></root>"#).unwrap();
        let doc_el = xot.document_element(doc)?;
        let a = xot.first_child(doc_el).unwrap();

        let step = Step {
            axis: ast::Axis::Child,
            node_test: ast::NodeTest::NameTest(ast::NameTest::Name(
                ast::Name::name("a").with_empty_span(),
            )),
        };
        let value = resolve_step(&step, doc_el, &mut xot, &mut HashMap::new());
        assert_eq!(value, xot_nodes_to_value(&[a]));
        Ok(())
    }
}
