use ahash::{HashSet, HashSetExt};
use xee_xpath_macros::xpath_fn;
use xot::xmlname::OwnedName;
use xot::{Node, Xot};

use crate::atomic;
use crate::context::DynamicContext;
use crate::error::Error;
use crate::function;
use crate::function::StaticFunctionDescription;
use crate::interpreter::Interpreter;
use crate::pattern::PredicateMatcher;
use crate::sequence;
use crate::{wrap_xpath_fn, xml};

#[xpath_fn(
    "fn:id($arg as xs:string*, $node as node()) as element()*",
    context_last
)]
fn id(
    context: &DynamicContext,
    interpreter: &Interpreter,
    arg: impl Iterator<Item = Result<String, Error>>,
    node: Node,
) -> Result<Vec<Node>, Error> {
    ids_helper(
        arg,
        node,
        interpreter.xot(),
        context
            .documents()
            .borrow()
            .document_order_access(interpreter.xot()),
    )
}

#[xpath_fn(
    "fn:element-with-id($arg as xs:string*, $node as node()) as element()*",
    context_last
)]
fn element_with_id(
    context: &DynamicContext,
    interpreter: &Interpreter,
    arg: impl Iterator<Item = Result<String, Error>>,
    node: Node,
) -> Result<Vec<Node>, Error> {
    // we only support xml:id so in the absence of schema information that
    // identifies an ID element, the behavior is the same as for fn:id
    ids_helper(
        arg,
        node,
        interpreter.xot(),
        context
            .documents()
            .borrow()
            .document_order_access(interpreter.xot()),
    )
}

fn ids_helper(
    arg: impl Iterator<Item = Result<String, Error>>,
    node: Node,
    xot: &Xot,
    annotations: xml::DocumentOrderAccess,
) -> Result<Vec<Node>, Error> {
    let document_node = xot.root(node);
    let mut result: Vec<Node> = Vec::new();
    let mut seen = HashSet::new();
    for idrefs in arg {
        let idrefs = idrefs?;
        // split idrefs into individual ids
        for idref in idrefs.split_whitespace() {
            if seen.contains(idref) {
                continue;
            }
            seen.insert(idref.to_string());
            // find the element with the given id
            // if found, return it
            // if not found, return an empty sequence
            if let Some(node) = xot.xml_id_node(document_node, idref) {
                result.push(node);
            }
        }
    }
    result.sort_by_key(|n| annotations.get(*n));
    Ok(result)
}

#[xpath_fn("fn:generate-id($arg as node()?) as xs:string", context_first)]
fn generate_id(
    context: &DynamicContext,
    interpreter: &Interpreter,
    arg: Option<xot::Node>,
) -> String {
    if let Some(arg) = arg {
        let documents = context.documents();
        let documents = documents.borrow();
        let annotations = documents.document_order_access(interpreter.xot());
        let annotation = annotations.get(arg);
        annotation.generate_id()
    } else {
        "".to_string()
    }
}

// key($key-name, $key-value) - 2-arg form searches the context node's document
#[xpath_fn(
    "fn:key($key_name as xs:string, $key_value as xs:anyAtomicType*, $top as node()) as node()*",
    context_last
)]
fn key(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    key_name: &str,
    key_value: &sequence::Sequence,
    top: Node,
) -> Result<Vec<Node>, Error> {
    key_helper(context, interpreter, key_name, key_value, top)
}

fn key_helper(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    key_name: &str,
    key_value: &sequence::Sequence,
    top: Node,
) -> Result<Vec<Node>, Error> {
    // Resolve the key name as a QName (no namespace for now, matching DocBook usage)
    let key_owned_name = OwnedName::new(key_name.to_string(), "".to_string(), "".to_string());

    // Find all key declarations with this name
    let key_decls: Vec<_> = interpreter
        .runnable()
        .program()
        .declarations
        .keys_by_name(&key_owned_name)
        .into_iter()
        .cloned()
        .collect();

    if key_decls.is_empty() {
        return Err(Error::Unsupported(format!(
            "No xsl:key declaration found for '{key_name}'"
        )));
    }

    // Collect the search values as strings for comparison
    let search_values: Vec<String> = key_value
        .atomized(interpreter.xot())
        .map(|a| a.map(|a| a.into_canonical()))
        .collect::<Result<Vec<_>, _>>()?;

    // Collect all nodes under `top` (including `top` itself)
    let nodes: Vec<Node> = std::iter::once(top)
        .chain(interpreter.xot().descendants(top))
        .collect();

    // Walk nodes, check each against the key's match pattern, evaluate the use
    // expression for matches, and collect nodes whose key value matches.
    let mut result: Vec<Node> = Vec::new();

    for node in nodes {
        let item = sequence::Item::from(node);

        for key_decl in &key_decls {
            // Test if this node matches the key's pattern
            if !interpreter.matches(&key_decl.pattern, &item) {
                continue;
            }

            // Evaluate the use expression with this node as context
            let use_function =
                function::InlineFunctionData::new(key_decl.use_function_id, Vec::new()).into();
            let arguments = [
                item.clone().into(),
                sequence::Sequence::from(atomic::Atomic::from(1u64)),
                sequence::Sequence::from(atomic::Atomic::from(1u64)),
            ];
            let key_values = interpreter.call_function_with_arguments(&use_function, &arguments)?;

            // Compare each produced key value against the search values
            for atom in key_values.atomized(interpreter.xot()) {
                let atom = atom?;
                let canonical = atom.into_canonical();
                if search_values.contains(&canonical) {
                    result.push(node);
                    break;
                }
            }
        }
    }

    // Sort by document order and deduplicate
    let documents = context.documents();
    let documents = documents.borrow();
    let annotations = documents.document_order_access(interpreter.xot());
    result.sort_by_key(|n| annotations.get(*n));
    result.dedup();
    Ok(result)
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(id),
        wrap_xpath_fn!(element_with_id),
        wrap_xpath_fn!(generate_id),
        wrap_xpath_fn!(key),
    ]
}
