// functions used to implement the XSLT that aren't supposed to be
// exposed to XPath
use ahash::{HashMap, HashMapExt};
use std::collections::HashSet;

use iri_string::types::{IriReferenceStr, IriString};
use xee_name::Namespaces;
use xee_xpath_ast::parse_name;
use xee_xpath_macros::xpath_fn;
use xot::xmlname::{NameStrInfo, OwnedName};
use xot::Xot;

use crate::atomic;
use crate::error;
use crate::function;
use crate::function::StaticFunctionDescription;
use crate::interpreter::Interpreter;
use crate::library::numeric;
use crate::sequence;
use crate::wrap_xpath_fn;
use crate::xml::DocumentsError;

// TODO: Things should really be hidden from XPath, and not be in the fn prefix

// https://www.w3.org/TR/xslt-30/#constructing-simple-content

#[xpath_fn("fn:simple-content($arg as item()*, $separator as xs:string) as xs:string?")]
fn simple_content(
    interpreter: &Interpreter,
    arg: &sequence::Sequence,
    separator: &str,
) -> error::Result<String> {
    let arg = simple_content_text_nodes(arg, interpreter.xot())?;
    // now atomize the sequence, putting in separators, except at the end
    let mut s = String::new();
    let mut first = true;
    for atom in arg.atomized(interpreter.xot()) {
        let atom = atom?;
        if !first {
            s.push_str(separator);
        }
        s.push_str(&atom.into_canonical());
        first = false;
    }
    Ok(s)
}

#[xpath_fn(
    "fn:group-by-first($seq as item()*, $key as function(item()) as xs:anyAtomicType*) as item()*"
)]
fn group_by_first(
    interpreter: &mut Interpreter,
    seq: &sequence::Sequence,
    key: sequence::Item,
) -> error::Result<sequence::Sequence> {
    let function = key.to_function()?;
    let mut seen: HashSet<atomic::Atomic> = HashSet::new();
    let mut result = Vec::new();

    for item in seq.iter() {
        let value = interpreter.call_function_with_arguments(&function, &[item.clone().into()])?;
        let keys = value
            .atomized(interpreter.xot())
            .collect::<error::Result<Vec<_>>>()?;
        if keys.into_iter().any(|atomic| seen.insert(atomic)) {
            result.push(item.clone());
        }
    }

    Ok(result.into())
}

#[xpath_fn("fn:xslt-try($body as function(*), $catch_patterns as array(*), $catch_handlers as array(*), $rollback_output as xs:string, $nonrecoverable_on_error as xs:string) as item()*")]
fn xslt_try(
    interpreter: &mut Interpreter,
    body: sequence::Item,
    catch_patterns: function::Array,
    catch_handlers: function::Array,
    rollback_output: &str,
    nonrecoverable_on_error: &str,
) -> error::Result<sequence::Sequence> {
    if catch_patterns.len() != catch_handlers.len() {
        return Err(error::Error::Unsupported(
            "Internal bug: xsl:try catch metadata mismatch".to_string(),
        ));
    }

    let body = body.to_function()?;
    let rollback_output = matches!(rollback_output, "yes" | "true" | "1");
    let nonrecoverable_on_error = matches!(nonrecoverable_on_error, "yes" | "true" | "1");
    match interpreter.call_function_with_arguments_catching_spanned_with_rollback(
        &body,
        &[],
        rollback_output,
    ) {
        Ok(result) => Ok(result),
        Err(caught_error) => {
            if interpreter.has_resolving_global_variable() {
                return Err(caught_error.error);
            }
            if nonrecoverable_on_error {
                return Err(error::Error::XTDE3530);
            }

            let (module_uri, line_number, column_number) =
                xslt_try_error_location(interpreter, &caught_error);

            let handler_arguments = vec![
                sequence::Sequence::from(vec![sequence::Item::Atomic(
                    caught_error.error.code_qname().into(),
                )]),
                sequence::Sequence::from(caught_error.error.message().to_string()),
                sequence::Sequence::default(),
                sequence::Sequence::from(module_uri),
                sequence::Sequence::from(line_number),
                sequence::Sequence::from(column_number),
            ];

            for (pattern_sequence, handler_sequence) in
                catch_patterns.iter().zip(catch_handlers.iter())
            {
                let pattern = pattern_sequence
                    .clone()
                    .one()?
                    .to_atomic()?
                    .cast_to_string()
                    .to_str()
                    .unwrap()
                    .to_string();
                if !xslt_try_matches(&pattern, &caught_error.error) {
                    continue;
                }

                let handler = handler_sequence.clone().one()?.to_function()?;
                return interpreter
                    .call_function_with_arguments_catching_spanned(&handler, &handler_arguments)
                    .map_err(|error| error.error);
            }

            Err(caught_error.error)
        }
    }
}

fn xslt_try_error_location(
    interpreter: &Interpreter,
    caught_error: &error::SpannedError,
) -> (String, i64, i64) {
    let program = interpreter.runnable().program();
    let module_uri = program
        .static_context()
        .static_base_uri()
        .map(ToString::to_string)
        .unwrap_or_default();
    let (line_number, column_number) = caught_error
        .span
        .and_then(|span| program.source_location(span))
        .map(|(line, column)| (line as i64, column as i64))
        .unwrap_or((0, 0));

    (module_uri, line_number, column_number)
}

fn xslt_try_matches(pattern: &str, caught_error: &error::Error) -> bool {
    let error_qname = caught_error.code_qname();
    pattern
        .split_ascii_whitespace()
        .any(|token| xslt_try_token_matches(token, &error_qname))
}

fn xslt_try_token_matches(token: &str, error_qname: &OwnedName) -> bool {
    if token == "*" {
        return true;
    }

    if let Some(rest) = token.strip_prefix("Q{") {
        let Some((namespace, local_name)) = rest.split_once('}') else {
            return false;
        };
        return error_qname.namespace() == namespace && error_qname.local_name() == local_name;
    }

    if let Some((prefix, local_name)) = token.rsplit_once(':') {
        if prefix == "*" {
            return error_qname.local_name() == local_name;
        }

        return false;
    }

    error_qname.namespace().is_empty() && error_qname.local_name() == token
}

#[xpath_fn(
    "fn:resolve-xslt-qname($lexical_name as xs:string, $default_namespace as xs:string, $namespace_map as xs:string, $force_namespace as xs:string) as xs:QName"
)]
fn resolve_xslt_qname(
    lexical_name: &str,
    default_namespace: &str,
    namespace_map: &str,
    force_namespace: &str,
) -> error::Result<atomic::Atomic> {
    let namespaces = decode_namespaces(namespace_map, default_namespace)?;
    let mut name = parse_name(lexical_name, &namespaces)
        .map_err(|_| error::Error::FOCA0002)?
        .value;

    if !force_namespace.is_empty() {
        name = OwnedName::new(
            name.local_name().to_string(),
            force_namespace.to_string(),
            name.prefix().to_string(),
        );
    } else if name.namespace().is_empty() && !default_namespace.is_empty() {
        name = name.with_default_namespace(default_namespace);
    }

    Ok(name.into())
}

#[xpath_fn(
    "fn:format-number-lexical($value as xs:string, $picture as xs:string) as xs:string"
)]
fn format_number_lexical2(
    context: &crate::context::DynamicContext,
    value: &str,
    picture: &str,
) -> error::Result<String> {
    numeric::format_number_from_lexical(context, value, picture, None)
}

#[xpath_fn(
    "fn:format-number-lexical($value as xs:string, $picture as xs:string, $decimal_format_name as xs:string) as xs:string"
)]
fn format_number_lexical3(
    context: &crate::context::DynamicContext,
    value: &str,
    picture: &str,
    decimal_format_name: &str,
) -> error::Result<String> {
    numeric::format_number_from_lexical(context, value, picture, Some(decimal_format_name))
}

#[xpath_fn("fn:xslt-number-value($value as item()*, $format as xs:string?) as xs:string")]
fn xslt_number_value(
    interpreter: &Interpreter,
    value: &sequence::Sequence,
    format: Option<&str>,
) -> error::Result<String> {
    let atomic = sequence::one(value.atomized(interpreter.xot()))??;
    let number = atomic
        .cast_to_integer_value::<i64>()
        .map_err(|_| error::Error::XPTY0004)?;
    if number < 0 {
        return Err(error::Error::Unsupported(
            "xsl:number value must be non-negative".to_string(),
        ));
    }

    format_xslt_number_value(number, format.unwrap_or("1"))
}

// xsl:number level="single" with default count pattern (no explicit count/from).
// Counts 1 + preceding siblings that match the same node kind and expanded-QName.
#[xpath_fn(
    "fn:xslt-number-count-single($node as node(), $format as xs:string?) as xs:string"
)]
fn xslt_number_count_single(
    interpreter: &Interpreter,
    node: xot::Node,
    format: Option<&str>,
) -> error::Result<String> {
    let xot = interpreter.xot();
    let count = count_single_level(xot, node);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

/// For level="single" with default count pattern: find the first ancestor-or-self
/// matching the default count (same node kind and expanded-QName), then count
/// 1 + preceding siblings that also match.
fn count_single_level(xot: &Xot, node: xot::Node) -> i64 {
    // The default count pattern matches nodes with the same node kind and
    // (for elements/attributes) the same expanded-QName as the selected node.
    let target_value_type = xot.value_type(node);
    let target_name = match xot.value(node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };

    // Walk ancestor-or-self to find the first node matching the default count pattern
    let count_node = std::iter::once(node)
        .chain(xot.ancestors(node))
        .find(|&n| node_matches_default_count(xot, n, target_value_type, target_name));

    let Some(count_node) = count_node else {
        // No matching ancestor; spec says result is empty list → format 0
        return 0;
    };

    // Count 1 + preceding siblings that match the same default count pattern
    let mut count: i64 = 1;
    for sibling in xot.axis(xot::Axis::PrecedingSibling, count_node) {
        if node_matches_default_count(xot, sibling, target_value_type, target_name) {
            count += 1;
        }
    }
    count
}

fn node_matches_default_count(
    xot: &Xot,
    node: xot::Node,
    target_value_type: xot::ValueType,
    target_name: Option<xot::NameId>,
) -> bool {
    if xot.value_type(node) != target_value_type {
        return false;
    }
    match target_name {
        Some(name) => match xot.value(node) {
            xot::Value::Element(el) => el.name() == name,
            xot::Value::Attribute(attr) => attr.name() == name,
            _ => false,
        },
        // For non-named nodes (text, comment, PI), kind match is sufficient
        None => true,
    }
}

// xsl:number level="single" with explicit count and/or from patterns (simple element names).
// count_local/count_ns: element name to count (empty = use default).
// from_local/from_ns: ancestor boundary (empty = no boundary).
#[xpath_fn(
    "fn:xslt-number-count-single-named($node as node(), $count_local as xs:string, $count_ns as xs:string, $from_local as xs:string, $from_ns as xs:string, $format as xs:string?) as xs:string"
)]
fn xslt_number_count_single_named(
    interpreter: &Interpreter,
    node: xot::Node,
    count_local: &str,
    count_ns: &str,
    from_local: &str,
    from_ns: &str,
    format: Option<&str>,
) -> error::Result<String> {
    let xot = interpreter.xot();

    let count_name_id = if count_local.is_empty() {
        // Default count: use the node's own name
        match xot.value(node) {
            xot::Value::Element(el) => Some(el.name()),
            xot::Value::Attribute(attr) => Some(attr.name()),
            _ => None,
        }
    } else {
        let count_owned = OwnedName::new(
            count_local.to_string(),
            count_ns.to_string(),
            String::new(),
        );
        count_owned.maybe_to_ref(xot).map(|r| r.name_id())
    };

    let from_name_id = if from_local.is_empty() {
        None
    } else {
        let from_owned = OwnedName::new(
            from_local.to_string(),
            from_ns.to_string(),
            String::new(),
        );
        Some(from_owned.maybe_to_ref(xot).map(|r| r.name_id()))
    };

    let count = count_single_level_named(xot, node, count_name_id, from_name_id);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

fn count_single_level_named(
    xot: &Xot,
    node: xot::Node,
    count_name_id: Option<xot::NameId>,
    from_name_id: Option<Option<xot::NameId>>,
) -> i64 {
    // Walk ancestor-or-self to find the first node matching the count pattern.
    // If from is specified, stop at the first ancestor matching from.
    let count_node = std::iter::once(node)
        .chain(xot.ancestors(node))
        .find(|&n| {
            // If we hit a from-boundary ancestor, stop searching
            if let Some(from_id) = from_name_id {
                if node_is_named_element(xot, n, from_id) {
                    return false;
                }
            }
            node_is_named_element(xot, n, count_name_id)
        });

    let Some(count_node) = count_node else {
        return 0;
    };

    // Count 1 + preceding siblings that match the count pattern
    let mut count: i64 = 1;
    for sibling in xot.axis(xot::Axis::PrecedingSibling, count_node) {
        if node_is_named_element(xot, sibling, count_name_id) {
            count += 1;
        }
    }
    count
}

/// Test if a node is an element with the given name. If name_id is None,
/// the name hasn't been seen in any document yet, so no node can match.
fn node_is_named_element(xot: &Xot, node: xot::Node, name_id: Option<xot::NameId>) -> bool {
    let Some(name_id) = name_id else {
        return false;
    };
    matches!(xot.value(node), xot::Value::Element(el) if el.name() == name_id)
}

// xsl:number level="any" with default count pattern (no explicit count/from).
// Counts all preceding nodes (in document order) that match the same node kind
// and expanded-QName as the current node, including the current node itself.
#[xpath_fn(
    "fn:xslt-number-count-any($node as node(), $format as xs:string?) as xs:string"
)]
fn xslt_number_count_any(
    interpreter: &Interpreter,
    node: xot::Node,
    format: Option<&str>,
) -> error::Result<String> {
    let xot = interpreter.xot();
    let count = count_any_level(xot, node);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

/// For level="any" with default count pattern: count all nodes preceding
/// (or equal to) the current node in document order that match the same
/// node kind and expanded-QName.
fn count_any_level(xot: &Xot, node: xot::Node) -> i64 {
    let target_value_type = xot.value_type(node);
    let target_name = match xot.value(node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };

    // Count the current node if it matches
    let mut count: i64 = if node_matches_default_count(xot, node, target_value_type, target_name) {
        1
    } else {
        0
    };

    // Walk all preceding nodes in document order (via preceding axis + ancestors)
    for n in reverse_document_order(xot, node) {
        if node_matches_default_count(xot, n, target_value_type, target_name) {
            count += 1;
        }
    }

    count
}

// xsl:number level="any" with explicit count and/or from patterns (simple element names).
#[xpath_fn(
    "fn:xslt-number-count-any-named($node as node(), $count_local as xs:string, $count_ns as xs:string, $from_local as xs:string, $from_ns as xs:string, $format as xs:string?) as xs:string"
)]
fn xslt_number_count_any_named(
    interpreter: &Interpreter,
    node: xot::Node,
    count_local: &str,
    count_ns: &str,
    from_local: &str,
    from_ns: &str,
    format: Option<&str>,
) -> error::Result<String> {
    let xot = interpreter.xot();

    let count_name_id = if count_local.is_empty() {
        // Default count: use the node's own name
        match xot.value(node) {
            xot::Value::Element(el) => Some(el.name()),
            xot::Value::Attribute(attr) => Some(attr.name()),
            _ => None,
        }
    } else {
        let count_owned = OwnedName::new(
            count_local.to_string(),
            count_ns.to_string(),
            String::new(),
        );
        count_owned.maybe_to_ref(xot).map(|r| r.name_id())
    };

    let from_name_id = if from_local.is_empty() {
        None
    } else {
        let from_owned = OwnedName::new(
            from_local.to_string(),
            from_ns.to_string(),
            String::new(),
        );
        Some(from_owned.maybe_to_ref(xot).map(|r| r.name_id()))
    };

    let count = count_any_level_named(xot, node, count_name_id, from_name_id);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

fn count_any_level_named(
    xot: &Xot,
    node: xot::Node,
    count_name_id: Option<xot::NameId>,
    from_name_id: Option<Option<xot::NameId>>,
) -> i64 {
    // Count the current node if it matches count
    let mut count: i64 = if node_is_named_element(xot, node, count_name_id) {
        1
    } else {
        0
    };

    // Walk preceding nodes + ancestors in reverse document order
    for n in reverse_document_order(xot, node) {
        // If we hit a from-boundary, stop counting
        if let Some(from_id) = from_name_id {
            if node_is_named_element(xot, n, from_id) {
                break;
            }
        }
        if node_is_named_element(xot, n, count_name_id) {
            count += 1;
        }
    }

    count
}

/// Walk all nodes preceding `node` in reverse document order, including
/// ancestors but excluding the node itself. Stops at the document root.
fn reverse_document_order(xot: &Xot, node: xot::Node) -> impl Iterator<Item = xot::Node> + '_ {
    ReverseDocOrderIter {
        xot,
        current: Some(node),
    }
}

struct ReverseDocOrderIter<'a> {
    xot: &'a Xot,
    current: Option<xot::Node>,
}

impl<'a> Iterator for ReverseDocOrderIter<'a> {
    type Item = xot::Node;

    fn next(&mut self) -> Option<xot::Node> {
        let node = self.current?;

        // Try previous sibling, then drill to its last descendant
        if let Some(prev) = self.xot.previous_sibling(node) {
            self.current = Some(Self::last_descendant(self.xot, prev));
        } else {
            // No previous sibling: go to parent
            self.current = self.xot.parent(node);
        }

        self.current
    }
}

impl<'a> ReverseDocOrderIter<'a> {
    fn last_descendant(xot: &Xot, node: xot::Node) -> xot::Node {
        let mut current = node;
        while let Some(last) = xot.last_child(current) {
            current = last;
        }
        current
    }
}

fn format_xslt_number_value(number: i64, picture: &str) -> error::Result<String> {
    if picture.is_empty() {
        return Err(error::Error::FODF1310);
    }

    // Parse the format picture into tokens and separators per XSLT spec.
    // Format tokens are sequences of alphanumeric characters.
    // Separator tokens are sequences of non-alphanumeric characters between format tokens.
    // The prefix is everything before the first format token.
    // The suffix is everything after the last format token.
    let chars: Vec<char> = picture.chars().collect();

    // Find all format token boundaries
    let mut tokens: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() {
            let start = i;
            while i < chars.len() && chars[i].is_alphanumeric() {
                i += 1;
            }
            tokens.push((start, i));
        } else {
            i += 1;
        }
    }

    if tokens.is_empty() {
        // No format tokens at all — use default "1"
        let formatted = format_number_token(number, "1")?;
        return Ok(format!("{picture}{formatted}"));
    }

    // For a single number (level="single"/"any"), use the first format token
    let (first_start, first_end) = tokens[0];
    let (_, last_end) = tokens[tokens.len() - 1];

    let prefix: String = chars[..first_start].iter().collect();
    let token: String = chars[first_start..first_end].iter().collect();
    let suffix: String = chars[last_end..].iter().collect();

    let formatted = format_number_token(number, &token)?;
    Ok(format!("{prefix}{formatted}{suffix}"))
}

fn format_number_token(number: i64, token: &str) -> error::Result<String> {
    if token.is_empty() {
        return Ok(number.to_string());
    }

    // Handle zero-padded decimal pictures like "01", "001"
    let chars: Vec<char> = token.chars().collect();
    if chars.iter().all(|c| *c == '0' || *c == '1')
        && chars.last() == Some(&'1')
        && chars.len() > 1
    {
        let min_width = chars.len();
        return Ok(format!("{:0>width$}", number, width = min_width));
    }

    if chars.len() > 1 {
        return Err(error::Error::Unsupported(format!(
            "xsl:number value formatting token not supported yet: {token}"
        )));
    }

    match chars[0] {
        '1' => Ok(number.to_string()),
        'a' => format_alphabetic_number(number, false),
        'A' => format_alphabetic_number(number, true),
        'i' => format_roman_number(number, false),
        'I' => format_roman_number(number, true),
        _ => Err(error::Error::Unsupported(format!(
            "xsl:number value formatting token not supported yet: {token}"
        ))),
    }
}

fn format_alphabetic_number(number: i64, uppercase: bool) -> error::Result<String> {
    if number == 0 {
        return Ok("0".to_string());
    }

    let mut value = u64::try_from(number).map_err(|_| error::Error::XPTY0004)?;
    let mut result = String::new();
    while value > 0 {
        value -= 1;
        let base = if uppercase { b'A' } else { b'a' };
        result.push((base + (value % 26) as u8) as char);
        value /= 26;
    }
    Ok(result.chars().rev().collect())
}

fn format_roman_number(number: i64, uppercase: bool) -> error::Result<String> {
    if number == 0 {
        return Ok("0".to_string());
    }

    let mut value = u64::try_from(number).map_err(|_| error::Error::XPTY0004)?;
    let numerals = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (magnitude, numeral) in numerals {
        while value >= magnitude {
            result.push_str(numeral);
            value -= magnitude;
        }
    }

    if uppercase {
        Ok(result)
    } else {
        Ok(result.to_ascii_lowercase())
    }
}

#[xpath_fn(
    "fn:store-result-document($href as xs:string, $content as item()*) as item()*",
    context_first
)]
fn store_result_document(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    href: &str,
    content: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    if href.is_empty() {
        context.store_principal_result_document(content.clone(), context.serialization_parameters().clone());
        return Ok(sequence::Sequence::default());
    }

    let document = content.normalize(" ", interpreter.xot_mut())?;
    let uri = absolute_result_document_uri(context, href)?;

    {
        let documents = context.documents();
        let mut documents = documents.borrow_mut();
        documents
            .add_root(Some(uri.as_ref()), document)
            .map_err(|error| match error {
                DocumentsError::DuplicateUri(_) => error::Error::XTDE1490,
                DocumentsError::Parse(_) => error::Error::FOXT0002,
            })?;
    }

    context.store_secondary_result_document(
        href.to_string(),
        sequence::Sequence::from(vec![sequence::Item::Node(document)]),
    );
    Ok(sequence::Sequence::default())
}

#[xpath_fn(
    "fn:store-principal-result-document($content as item()*, $method as xs:string, $byte_order_mark as xs:string, $cdata as xs:string, $doctype_public as xs:string, $doctype_system as xs:string, $include_content_type as xs:string, $media_type as xs:string, $omit_xml_declaration as xs:string, $standalone as xs:string, $html_version as xs:string, $use_character_maps as xs:string, $version as xs:string) as item()*",
    context_first
)]
fn store_principal_result_document(
    context: &crate::context::DynamicContext,
    content: &sequence::Sequence,
    method: &str,
    byte_order_mark: &str,
    cdata: &str,
    doctype_public: &str,
    doctype_system: &str,
    include_content_type: &str,
    media_type: &str,
    omit_xml_declaration: &str,
    standalone: &str,
    html_version: &str,
    use_character_maps: &str,
    version: &str,
) -> error::Result<sequence::Sequence> {
    let mut parameters = context.serialization_parameters().clone();
    if !method.is_empty() {
        parameters.method = sequence::QNameOrString::String(method.to_string());
    }
    if !byte_order_mark.is_empty() {
        parameters.byte_order_mark = parse_boolean(byte_order_mark)?;
    }
    if !cdata.is_empty() {
        parameters.cdata_section_elements = parse_cdata_section_elements(context, cdata)?;
    }
    if !doctype_system.is_empty() {
        parameters.doctype_system = Some(doctype_system.to_string());
        if !doctype_public.is_empty() {
            parameters.doctype_public = Some(doctype_public.to_string());
        }
    }
    if !include_content_type.is_empty() {
        parameters.include_content_type = matches!(include_content_type, "yes" | "true" | "1");
    }
    if !media_type.is_empty() {
        parameters.media_type = Some(media_type.to_string());
    }
    if !omit_xml_declaration.is_empty() {
        parameters.omit_xml_declaration = parse_boolean(omit_xml_declaration)?;
    }
    if !standalone.is_empty() {
        parameters.standalone = parse_standalone(standalone)?;
    }
    if !html_version.is_empty() {
        parameters.html_version = rust_decimal::Decimal::from_str_exact(html_version)
            .map_err(|_| error::Error::SEPM0016)?;
        parameters.explicit_html_version = true;
    }
    if !use_character_maps.is_empty() {
        parameters.use_character_maps = parse_character_maps(use_character_maps)?;
    }
    if !version.is_empty() {
        parameters.version = version.to_string();
    }
    context.store_principal_result_document(content.clone(), parameters);
    Ok(sequence::Sequence::default())
}

#[xpath_fn(
    "fn:xslt-evaluate($xpath as xs:string?, $context_item as item()*, $namespace_context as item()*, $with_params as item()*) as item()*"
)]
fn xslt_evaluate(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    xpath: Option<&str>,
    context_item: &sequence::Sequence,
    namespace_context: &sequence::Sequence,
    with_params: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    let evaluator = context.dynamic_xpath_evaluator().ok_or_else(|| {
        error::Error::Unsupported("xsl:evaluate is not configured for this program".to_string())
    })?;

    let namespace_context = namespace_context.clone().option()?;
    let with_params = match with_params.clone().option()? {
        None => None,
        Some(sequence::Item::Function(function::Function::Map(map))) => Some(map),
        Some(_) => return Err(error::Error::XPTY0004),
    };

    let request = crate::interpreter::DynamicXPathRequest {
        xpath: xpath.ok_or(error::Error::XPTY0004)?.to_string(),
        context_item: context_item.clone().option()?,
        namespace_context,
        with_params,
    };

    evaluator
        .evaluate(&request, context, interpreter)
        .map_err(|error| error.error)
}

fn absolute_result_document_uri(
    context: &crate::context::DynamicContext,
    href: &str,
) -> error::Result<IriString> {
    let href: &IriReferenceStr = href.try_into().map_err(|_| error::Error::FOXT0002)?;
    Ok(match href.to_iri() {
        Ok(uri) => uri.into(),
        Err(relative_uri) => {
            let base = context
                .static_context()
                .static_base_uri()
                .ok_or(error::Error::FOXT0002)?;
            relative_uri.resolve_against(base).into()
        }
    })
}

fn parse_cdata_section_elements(
    context: &crate::context::DynamicContext,
    cdata: &str,
) -> error::Result<Vec<OwnedName>> {
    cdata
        .split_ascii_whitespace()
        .map(|name| {
            if let Some((prefix, local_name)) = name.split_once(':') {
                OwnedName::prefixed(prefix, local_name, |lookup_prefix| {
                    context
                        .static_context()
                        .namespaces()
                        .by_prefix(lookup_prefix)
                        .map(str::to_string)
                })
                .map_err(|_| error::Error::XTSE0020)
            } else {
                Ok(OwnedName::name(name))
            }
        })
        .collect()
}

fn parse_character_maps(encoded: &str) -> error::Result<ahash::HashMap<char, String>> {
    let mut character_maps = ahash::HashMap::default();
    for entry in encoded.split('|') {
        let Some((character, replacement)) = entry.split_once('=') else {
            return Err(error::Error::FOXT0002);
        };
        let character = u32::from_str_radix(character, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or(error::Error::FOXT0002)?;
        let replacement = replacement
            .as_bytes()
            .chunks(2)
            .map(|chunk| {
                let hex = std::str::from_utf8(chunk).map_err(|_| error::Error::FOXT0002)?;
                u8::from_str_radix(hex, 16).map_err(|_| error::Error::FOXT0002)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let replacement = String::from_utf8(replacement).map_err(|_| error::Error::FOXT0002)?;
        character_maps.insert(character, replacement);
    }
    Ok(character_maps)
}

fn decode_namespaces(namespace_map: &str, default_namespace: &str) -> error::Result<Namespaces> {
    let mut namespaces = HashMap::new();
    if !namespace_map.is_empty() {
        for entry in namespace_map.split('|') {
            let Some((prefix, uri)) = entry.split_once('=') else {
                return Err(error::Error::FOCA0002);
            };
            let prefix = decode_hex(prefix)?;
            let uri = decode_hex(uri)?;
            namespaces.insert(prefix, uri);
        }
    }
    Ok(Namespaces::new(
        namespaces,
        default_namespace.to_string(),
        "".to_string(),
    ))
}

fn decode_hex(value: &str) -> error::Result<String> {
    let bytes = value
        .as_bytes()
        .chunks(2)
        .map(|chunk| {
            let hex = std::str::from_utf8(chunk).map_err(|_| error::Error::FOCA0002)?;
            u8::from_str_radix(hex, 16).map_err(|_| error::Error::FOCA0002)
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| error::Error::FOCA0002)
}

fn parse_standalone(standalone: &str) -> error::Result<Option<bool>> {
    match standalone.trim() {
        "yes" | "true" | "1" => Ok(Some(true)),
        "no" | "false" | "0" => Ok(Some(false)),
        "omit" => Ok(None),
        _ => Err(error::Error::SEPM0016),
    }
}

fn parse_boolean(value: &str) -> error::Result<bool> {
    match value.trim() {
        "yes" | "true" | "1" => Ok(true),
        "no" | "false" | "0" => Ok(false),
        _ => Err(error::Error::SEPM0016),
    }
}

fn simple_content_text_nodes(
    arg: &sequence::Sequence,
    xot: &Xot,
) -> error::Result<sequence::Sequence> {
    // 1. zero-length text nodes in the sequence are discarded
    // 2. adjacent text nodes are merged into a single text node

    // Note: to avoid having to create xot nodes on the fly, we actually
    // turn adjecent text nodes into atomic string nodes, which should be
    // fine.
    let mut r: Vec<sequence::Item> = Vec::new();
    let mut last_text: Option<String> = None;
    for item in arg.iter() {
        if let sequence::Item::Node(node) = item {
            if let xot::Value::Text(text) = xot.value(node) {
                let text = text.get();
                if text.is_empty() {
                    continue;
                }
                if let Some(mut s) = last_text.take() {
                    // add the text to the last text node
                    s.push_str(text);
                    last_text = Some(s);
                } else {
                    // set the last text node instead
                    last_text = Some(text.to_string());
                }
                continue;
            }
        }
        if let Some(s) = last_text.take() {
            r.push(sequence::Item::Atomic(s.into()));
        }
        r.push(item.clone());
    }
    // set the last text node
    if let Some(s) = last_text.take() {
        r.push(sequence::Item::Atomic(s.into()));
    }
    Ok(r.into())
}

#[xpath_fn(
    "fn:xslt-analyze-string($input as xs:string, $regex as xs:string, $flags as xs:string, $match_fn as function(*), $non_match_fn as function(*)) as item()*"
)]
fn xslt_analyze_string(
    interpreter: &mut Interpreter,
    input: &str,
    regex: &str,
    flags: &str,
    match_fn: sequence::Item,
    non_match_fn: sequence::Item,
) -> error::Result<sequence::Sequence> {
    use regexml::AnalyzeEntry;

    let compiled_regex = interpreter.regex(regex, flags)?;
    let analyze_results = compiled_regex.analyze(input)?;

    let match_function = match_fn.to_function()?;
    let non_match_function = non_match_fn.to_function()?;

    let mut result = Vec::new();
    for entry in analyze_results {
        let (substring, function) = match &entry {
            AnalyzeEntry::Match(match_entries) => {
                let mut s = String::new();
                collect_match_text(match_entries, &mut s);
                (s, &match_function)
            }
            AnalyzeEntry::NonMatch(s) => (s.clone(), &non_match_function),
        };
        let arg: sequence::Sequence =
            sequence::Item::Atomic(atomic::Atomic::from(substring)).into();
        let items = interpreter.call_function_with_arguments(function, &[arg])?;
        for item in items.iter() {
            result.push(item.clone());
        }
    }
    Ok(result.into())
}

fn collect_match_text(entries: &[regexml::MatchEntry], out: &mut String) {
    for entry in entries {
        match entry {
            regexml::MatchEntry::String(s) => out.push_str(s),
            regexml::MatchEntry::Group { value, .. } => collect_match_text(value, out),
        }
    }
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(simple_content),
        wrap_xpath_fn!(group_by_first),
        wrap_xpath_fn!(xslt_try),
        wrap_xpath_fn!(resolve_xslt_qname),
        wrap_xpath_fn!(format_number_lexical2),
        wrap_xpath_fn!(format_number_lexical3),
        wrap_xpath_fn!(xslt_number_value),
        wrap_xpath_fn!(xslt_number_count_single),
        wrap_xpath_fn!(xslt_number_count_single_named),
        wrap_xpath_fn!(xslt_number_count_any),
        wrap_xpath_fn!(xslt_number_count_any_named),
        wrap_xpath_fn!(xslt_evaluate),
        wrap_xpath_fn!(store_result_document),
        wrap_xpath_fn!(store_principal_result_document),
        wrap_xpath_fn!(xslt_analyze_string),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    use sequence::{Item, Sequence};

    #[test]
    fn test_filter_empty_text_nodes() {
        let mut xot = Xot::new();
        let empty = xot.new_text("");

        let sequence = Sequence::from(vec![
            Item::Node(empty),
            Item::Atomic(1.into()),
            Item::Node(empty),
            Item::Atomic(2.into()),
            Item::Node(empty),
        ]);
        let result = simple_content_text_nodes(&sequence, &xot).unwrap();
        assert_eq!(result.len(), 2);
        let items = result.iter().collect::<Vec<_>>();
        assert_eq!(items, vec![Item::Atomic(1.into()), Item::Atomic(2.into())]);
    }

    #[test]
    fn test_concatenate_adjacent_text_nodes() {
        let mut xot = Xot::new();
        let a = xot.new_text("a");
        let b = xot.new_text("b");
        let sequence = Sequence::from(vec![Item::Node(a), Item::Node(b)]);
        let result = simple_content_text_nodes(&sequence, &xot).unwrap();
        let items = result.iter().collect::<Vec<_>>();
        assert_eq!(items, vec![Item::Atomic("ab".into())]);
    }

    #[test]
    fn test_concatenate_adjacent_text_nodes_three() {
        let mut xot = Xot::new();
        let a = xot.new_text("a");
        let b = xot.new_text("b");
        let c = xot.new_text("c");

        let sequence = Sequence::from(vec![Item::Node(a), Item::Node(b), Item::Node(c)]);
        let result = simple_content_text_nodes(&sequence, &xot).unwrap();
        let items = result.iter().collect::<Vec<_>>();
        assert_eq!(items, vec![Item::Atomic("abc".into())]);
    }

    #[test]
    fn test_concatenate_adjacent_text_nodes_not_ending() {
        let mut xot = Xot::new();
        let a = xot.new_text("a");
        let b = xot.new_text("b");
        let c = xot.new_text("c");

        let sequence = Sequence::from(vec![
            Item::Node(a),
            Item::Node(b),
            Item::Node(c),
            Item::Atomic(1.into()),
        ]);
        let result = simple_content_text_nodes(&sequence, &xot).unwrap();
        let items = result.iter().collect::<Vec<_>>();
        assert_eq!(
            items,
            vec![Item::Atomic("abc".into()), Item::Atomic(1.into())]
        );
    }
}
