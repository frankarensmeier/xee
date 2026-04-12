// functions used to implement the XSLT that aren't supposed to be
// exposed to XPath
use ahash::{HashMap, HashMapExt};

use iri_string::types::{IriReferenceStr, IriString};
use ibig::IBig;
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
use crate::pattern::PredicateMatcher;
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
    "fn:xslt-for-each-group-by($seq as item()*, $key as function(item()) as xs:anyAtomicType*, $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
)]
fn xslt_for_each_group_by(
    interpreter: &mut Interpreter,
    seq: &sequence::Sequence,
    key: sequence::Item,
    body: sequence::Item,
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<sequence::Sequence> {
    let key_function = key.to_function()?;
    let body_function = body.to_function()?;

    // Build groups preserving insertion order
    let mut group_keys: Vec<atomic::Atomic> = Vec::new();
    let mut groups: HashMap<atomic::Atomic, Vec<sequence::Item>> = HashMap::new();

    for item in seq.iter() {
        let value =
            interpreter.call_function_with_arguments(&key_function, &[item.clone().into()])?;
        let keys = value
            .atomized(interpreter.xot())
            .collect::<error::Result<Vec<_>>>()?;
        for k in keys {
            if !groups.contains_key(&k) {
                group_keys.push(k.clone());
            }
            groups.entry(k).or_default().push(item.clone());
        }
    }

    // Sort groups if a sort key function is provided
    let sorted_keys = if !sort_key.is_empty() {
        let sort_key_fn = sort_key.iter().next().unwrap().to_function()?;
        let mut keyed_groups: Vec<(atomic::Atomic, atomic::Atomic)> = Vec::new();
        for gk in &group_keys {
            let first_item: sequence::Sequence = groups
                .get(gk)
                .and_then(|g| g.first())
                .map(|item| item.clone().into())
                .unwrap_or_default();
            let sort_val = interpreter
                .call_function_with_arguments(&sort_key_fn, &[first_item])?;
            let sort_atomic = sort_val
                .atomized(interpreter.xot())
                .next()
                .transpose()?
                .unwrap_or(atomic::Atomic::from(""));
            keyed_groups.push((gk.clone(), sort_atomic));
        }

        let is_descending = matches!(sort_descending, "yes");
        let is_numeric = matches!(sort_numeric, "yes");

        keyed_groups.sort_by(|a, b| {
            let ordering = {
                let a_str = a.1.clone().into_canonical();
                let b_str = b.1.clone().into_canonical();
                if is_numeric {
                    let a_num: f64 = a_str.parse().unwrap_or(f64::NAN);
                    let b_num: f64 = b_str.parse().unwrap_or(f64::NAN);
                    a_num.partial_cmp(&b_num).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    a_str.cmp(&b_str)
                }
            };
            if is_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });

        keyed_groups.into_iter().map(|(k, _)| k).collect()
    } else {
        group_keys
    };

    // Iterate over groups in order, calling body with context item, position, last
    let mut result = Vec::new();
    let total_groups: IBig = sorted_keys.len().into();
    for (index, group_key) in sorted_keys.iter().enumerate() {
        let group_items: sequence::Sequence = groups
            .get(group_key)
            .cloned()
            .unwrap_or_default()
            .into();
        let first_item: sequence::Sequence = group_items
            .iter()
            .next()
            .map(|item| item.clone().into())
            .unwrap_or_default();

        let position: IBig = (index + 1).into();

        interpreter.push_current_group(group_items, Some(group_key.clone()));
        let body_result = interpreter.call_function_with_arguments(
            &body_function,
            &[
                first_item,
                atomic::Atomic::from(position).into(),
                atomic::Atomic::from(total_groups.clone()).into(),
            ],
        );
        interpreter.pop_current_group();

        result.extend(body_result?.iter());
    }

    Ok(result.into())
}

#[xpath_fn(
    "fn:xslt-for-each-group-adjacent($seq as item()*, $key as function(item()) as xs:anyAtomicType*, $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
)]
fn xslt_for_each_group_adjacent(
    interpreter: &mut Interpreter,
    seq: &sequence::Sequence,
    key: sequence::Item,
    body: sequence::Item,
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<sequence::Sequence> {
    let key_function = key.to_function()?;
    let body_function = body.to_function()?;

    // Build groups of adjacent items with the same key
    let mut group_keys: Vec<atomic::Atomic> = Vec::new();
    let mut group_items_list: Vec<Vec<sequence::Item>> = Vec::new();

    for item in seq.iter() {
        let value =
            interpreter.call_function_with_arguments(&key_function, &[item.clone().into()])?;
        let k = value
            .atomized(interpreter.xot())
            .next()
            .transpose()?
            .unwrap_or(atomic::Atomic::from(""));

        // Check if this key is the same as the previous group's key
        if let Some(last_key) = group_keys.last() {
            if *last_key == k {
                // Same key as previous — add to current group
                group_items_list.last_mut().unwrap().push(item.clone());
                continue;
            }
        }
        // New key — start a new group
        group_keys.push(k);
        group_items_list.push(vec![item.clone()]);
    }

    // Sort groups if a sort key function is provided
    let sorted_indices: Vec<usize> = if !sort_key.is_empty() {
        let sort_key_fn = sort_key.iter().next().unwrap().to_function()?;
        let mut keyed: Vec<(usize, atomic::Atomic)> = Vec::new();
        for (i, group) in group_items_list.iter().enumerate() {
            let first_item: sequence::Sequence = group
                .first()
                .map(|item| item.clone().into())
                .unwrap_or_default();
            let sort_val = interpreter
                .call_function_with_arguments(&sort_key_fn, &[first_item])?;
            let sort_atomic = sort_val
                .atomized(interpreter.xot())
                .next()
                .transpose()?
                .unwrap_or(atomic::Atomic::from(""));
            keyed.push((i, sort_atomic));
        }

        let is_descending = matches!(sort_descending, "yes");
        let is_numeric = matches!(sort_numeric, "yes");

        keyed.sort_by(|a, b| {
            let ordering = {
                let a_str = a.1.clone().into_canonical();
                let b_str = b.1.clone().into_canonical();
                if is_numeric {
                    let a_num: f64 = a_str.parse().unwrap_or(f64::NAN);
                    let b_num: f64 = b_str.parse().unwrap_or(f64::NAN);
                    a_num.partial_cmp(&b_num).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    a_str.cmp(&b_str)
                }
            };
            if is_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });

        keyed.into_iter().map(|(i, _)| i).collect()
    } else {
        (0..group_keys.len()).collect()
    };

    // Iterate over groups in order
    let mut result = Vec::new();
    let total_groups: IBig = sorted_indices.len().into();
    for (pos, &idx) in sorted_indices.iter().enumerate() {
        let group_seq: sequence::Sequence = group_items_list[idx].clone().into();
        let first_item: sequence::Sequence = group_seq
            .iter()
            .next()
            .map(|item| item.clone().into())
            .unwrap_or_default();

        let position: IBig = (pos + 1).into();
        let group_key = group_keys[idx].clone();

        interpreter.push_current_group(group_seq, Some(group_key));
        let body_result = interpreter.call_function_with_arguments(
            &body_function,
            &[
                first_item,
                atomic::Atomic::from(position).into(),
                atomic::Atomic::from(total_groups.clone()).into(),
            ],
        );
        interpreter.pop_current_group();

        result.extend(body_result?.iter());
    }

    Ok(result.into())
}

#[xpath_fn("fn:current-group() as item()*")]
fn current_group(interpreter: &Interpreter) -> error::Result<sequence::Sequence> {
    Ok(interpreter.current_group())
}

#[xpath_fn("fn:current-grouping-key() as xs:anyAtomicType?")]
fn current_grouping_key(interpreter: &Interpreter) -> error::Result<sequence::Sequence> {
    Ok(interpreter.current_grouping_key())
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
    // Per XSLT spec, the value is rounded to the nearest integer
    let number = match &atomic {
        atomic::Atomic::Float(f) => f.round() as i64,
        atomic::Atomic::Double(d) => d.round() as i64,
        atomic::Atomic::Decimal(d) => {
            let rounded = d.round();
            i64::try_from(rounded).map_err(|_| error::Error::XPTY0004)?
        }
        atomic::Atomic::Integer(_, i) => i64::try_from(i.as_ref())
            .map_err(|_| error::Error::XPTY0004)?,
        _ => atomic
            .cast_to_integer_value::<i64>()
            .map_err(|_| error::Error::XPTY0004)?,
    };
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

// xsl:number level="single" with compiled count/from patterns.
// count_index: index into declarations.number_patterns for the count pattern (-1 = default).
// from_index: index into declarations.number_patterns for the from pattern (-1 = none).
#[xpath_fn(
    "fn:xslt-number-count-single-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?) as xs:string"
)]
fn xslt_number_count_single_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
) -> error::Result<String> {
    let count_index: i64 = (&count_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let from_index: i64 = (&from_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let count = count_single_level_pattern(interpreter, node, count_index, from_index);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

fn count_single_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    let use_default_count = count_index < 0;

    // Collect ancestor-or-self first to avoid borrow conflicts
    let ancestor_or_self: Vec<_> = std::iter::once(node)
        .chain(interpreter.xot().ancestors(node))
        .collect();

    // Walk ancestor-or-self to find the first node matching the count pattern.
    // If from is specified, stop at the first ancestor matching from.
    let count_node = {
        let mut found = None;
        for n in ancestor_or_self {
            // If we hit a from-boundary ancestor, stop searching
            if from_index >= 0 {
                if node_matches_pattern(interpreter, n, from_index as usize) {
                    break;
                }
            }
            if use_default_count {
                if node_matches_default_count_for(interpreter.xot(), node, n) {
                    found = Some(n);
                    break;
                }
            } else if node_matches_pattern(interpreter, n, count_index as usize) {
                found = Some(n);
                break;
            }
        }
        found
    };

    let Some(count_node) = count_node else {
        return 0;
    };

    // Count 1 + preceding siblings that match the count pattern
    let mut count: i64 = 1;
    let siblings: Vec<_> = interpreter
        .xot()
        .axis(xot::Axis::PrecedingSibling, count_node)
        .collect();
    for sibling in siblings {
        if use_default_count {
            if node_matches_default_count_for(interpreter.xot(), node, sibling) {
                count += 1;
            }
        } else if node_matches_pattern(interpreter, sibling, count_index as usize) {
            count += 1;
        }
    }
    count
}

// xsl:number level="any" with compiled count/from patterns.
#[xpath_fn(
    "fn:xslt-number-count-any-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?) as xs:string"
)]
fn xslt_number_count_any_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
) -> error::Result<String> {
    let count_index: i64 = (&count_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let from_index: i64 = (&from_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let count = count_any_level_pattern(interpreter, node, count_index, from_index);
    format_xslt_number_value(count, format.unwrap_or("1"))
}

fn count_any_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    let use_default_count = count_index < 0;

    // Count the current node if it matches count
    let mut count: i64 = if use_default_count {
        if node_matches_default_count_for(interpreter.xot(), node, node) {
            1
        } else {
            0
        }
    } else if node_matches_pattern(interpreter, node, count_index as usize) {
        1
    } else {
        0
    };

    // Collect all preceding nodes first (can't borrow xot mutably while iterating)
    let preceding: Vec<_> = reverse_document_order(interpreter.xot(), node).collect();

    for n in preceding {
        // If we hit a from-boundary, stop counting
        if from_index >= 0 {
            if node_matches_pattern(interpreter, n, from_index as usize) {
                break;
            }
        }
        if use_default_count {
            if node_matches_default_count_for(interpreter.xot(), node, n) {
                count += 1;
            }
        } else if node_matches_pattern(interpreter, n, count_index as usize) {
            count += 1;
        }
    }

    count
}

// xsl:number level="multiple" with default count pattern (no explicit count/from).
// For each ancestor-or-self matching the default count, count 1 + preceding siblings matching.
// Returns the formatted multi-value string.
#[xpath_fn(
    "fn:xslt-number-count-multiple($node as node(), $format as xs:string?) as xs:string"
)]
fn xslt_number_count_multiple(
    interpreter: &Interpreter,
    node: xot::Node,
    format: Option<&str>,
) -> error::Result<String> {
    let xot = interpreter.xot();
    let numbers = count_multiple_level(xot, node);
    format_xslt_number_values(&numbers, format.unwrap_or("1"))
}

/// For level="multiple" with default count pattern: walk ancestor-or-self,
/// for each matching ancestor count 1 + preceding siblings matching the same pattern.
/// Return the counts in outermost-first order.
fn count_multiple_level(xot: &Xot, node: xot::Node) -> Vec<i64> {
    let target_value_type = xot.value_type(node);
    let target_name = match xot.value(node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };

    let mut numbers = Vec::new();
    // xot.ancestors() includes self (indextree semantics), so no iter::once(node) needed
    for n in xot.ancestors(node) {
        if node_matches_default_count(xot, n, target_value_type, target_name) {
            let mut count: i64 = 1;
            for sibling in xot.axis(xot::Axis::PrecedingSibling, n) {
                if node_matches_default_count(xot, sibling, target_value_type, target_name) {
                    count += 1;
                }
            }
            numbers.push(count);
        }
    }
    // Reverse: we collected innermost-first, but spec wants outermost-first
    numbers.reverse();
    numbers
}

// xsl:number level="multiple" with compiled count/from patterns.
#[xpath_fn(
    "fn:xslt-number-count-multiple-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?) as xs:string"
)]
fn xslt_number_count_multiple_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
) -> error::Result<String> {
    let count_index: i64 = (&count_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let from_index: i64 = (&from_index).try_into().map_err(|_| error::Error::XPTY0004)?;
    let numbers = count_multiple_level_pattern(interpreter, node, count_index, from_index);
    format_xslt_number_values(&numbers, format.unwrap_or("1"))
}

fn count_multiple_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> Vec<i64> {
    let use_default_count = count_index < 0;

    // Collect ancestor-or-self first to avoid borrow conflicts
    // xot.ancestors() includes self (indextree semantics)
    let ancestor_or_self: Vec<_> = interpreter.xot().ancestors(node).collect();

    let mut numbers = Vec::new();
    for n in &ancestor_or_self {
        let n = *n;
        // If we hit a from-boundary ancestor, stop
        if from_index >= 0 {
            if node_matches_pattern(interpreter, n, from_index as usize) {
                break;
            }
        }
        let matches_count = if use_default_count {
            node_matches_default_count_for(interpreter.xot(), node, n)
        } else {
            node_matches_pattern(interpreter, n, count_index as usize)
        };
        if matches_count {
            // Count 1 + preceding siblings matching count pattern
            let siblings: Vec<_> = interpreter
                .xot()
                .axis(xot::Axis::PrecedingSibling, n)
                .collect();
            let mut count: i64 = 1;
            for sibling in siblings {
                let sibling_matches = if use_default_count {
                    node_matches_default_count_for(interpreter.xot(), node, sibling)
                } else {
                    node_matches_pattern(interpreter, sibling, count_index as usize)
                };
                if sibling_matches {
                    count += 1;
                }
            }
            numbers.push(count);
        }
    }
    // Reverse: collected innermost-first, spec wants outermost-first
    numbers.reverse();
    numbers
}

/// Test if a node matches the compiled pattern stored at the given index.
fn node_matches_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    pattern_index: usize,
) -> bool {
    let pattern = interpreter
        .runnable()
        .program()
        .declarations
        .number_pattern(pattern_index)
        .pattern
        .clone();
    let item = sequence::Item::from(node);
    interpreter.matches(&pattern, &item)
}

/// Test if a candidate node matches the default count pattern for the reference node.
/// Default count matches nodes with the same node kind and expanded-QName.
fn node_matches_default_count_for(xot: &Xot, reference_node: xot::Node, candidate: xot::Node) -> bool {
    let target_value_type = xot.value_type(reference_node);
    let target_name = match xot.value(reference_node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };
    node_matches_default_count(xot, candidate, target_value_type, target_name)
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
    format_xslt_number_values(&[number], picture)
}

/// Format a sequence of numbers according to the XSLT format picture.
/// For level="single"/"any" this is a single number; for level="multiple" it may be several.
fn format_xslt_number_values(numbers: &[i64], picture: &str) -> error::Result<String> {
    if numbers.is_empty() {
        return Ok(String::new());
    }

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
        let mut result = picture.to_string();
        for (i, &num) in numbers.iter().enumerate() {
            if i > 0 {
                result.push('.');
            }
            result.push_str(&format_number_token(num, "1")?);
        }
        return Ok(result);
    }

    let prefix: String = chars[..tokens[0].0].iter().collect();
    let suffix: String = chars[tokens[tokens.len() - 1].1..].iter().collect();

    let mut result = prefix;

    for (i, &num) in numbers.iter().enumerate() {
        if i > 0 {
            // Use the separator between token[i-1] and token[i] if it exists,
            // otherwise use the separator between the last two tokens,
            // otherwise use "."
            let sep = if i < tokens.len() {
                let prev_end = tokens[i - 1].1;
                let curr_start = tokens[i].0;
                let s: String = chars[prev_end..curr_start].iter().collect();
                if s.is_empty() { ".".to_string() } else { s }
            } else if tokens.len() >= 2 {
                let prev_end = tokens[tokens.len() - 2].1;
                let curr_start = tokens[tokens.len() - 1].0;
                let s: String = chars[prev_end..curr_start].iter().collect();
                if s.is_empty() { ".".to_string() } else { s }
            } else {
                ".".to_string()
            };
            result.push_str(&sep);
        }

        // Use token[i] if it exists, otherwise use the last token
        let token_idx = if i < tokens.len() { i } else { tokens.len() - 1 };
        let (t_start, t_end) = tokens[token_idx];
        let token: String = chars[t_start..t_end].iter().collect();
        result.push_str(&format_number_token(num, &token)?);
    }

    result.push_str(&suffix);
    Ok(result)
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
        let (substring, function, is_match) = match &entry {
            AnalyzeEntry::Match(match_entries) => {
                let mut s = String::new();
                collect_match_text(match_entries, &mut s);
                (s, &match_function, true)
            }
            AnalyzeEntry::NonMatch(s) => (s.clone(), &non_match_function, false),
        };
        let arg: sequence::Sequence =
            sequence::Item::Atomic(atomic::Atomic::from(substring)).into();
        if is_match {
            if let AnalyzeEntry::Match(match_entries) = &entry {
                let groups = extract_regex_groups(match_entries);
                interpreter.push_regex_groups(groups);
            }
        }
        let items = interpreter.call_function_with_arguments(function, &[arg])?;
        if is_match {
            interpreter.pop_regex_groups();
        }
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

/// Extract regex group strings from match entries.
/// Group 0 = entire match, groups 1..N = capture groups by number.
fn extract_regex_groups(entries: &[regexml::MatchEntry]) -> Vec<String> {
    // First collect the full match text as group 0
    let mut full_match = String::new();
    collect_match_text(entries, &mut full_match);

    // Find the maximum group number
    let max_group = max_group_nr(entries);

    // Build groups vector: index 0 = full match, index N = group N
    let mut groups = vec![String::new(); max_group + 1];
    groups[0] = full_match;

    // Collect text for each numbered group
    for entry in entries {
        collect_group_texts(entry, &mut groups);
    }

    groups
}

fn max_group_nr(entries: &[regexml::MatchEntry]) -> usize {
    let mut max = 0;
    for entry in entries {
        if let regexml::MatchEntry::Group { nr, value } = entry {
            max = max.max(*nr);
            max = max.max(max_group_nr(value));
        }
    }
    max
}

fn collect_group_texts(entry: &regexml::MatchEntry, groups: &mut [String]) {
    if let regexml::MatchEntry::Group { nr, value } = entry {
        let mut text = String::new();
        collect_match_text(value, &mut text);
        if *nr < groups.len() {
            groups[*nr] = text;
        }
        for sub in value {
            collect_group_texts(sub, groups);
        }
    }
}

#[xpath_fn("fn:regex-group($group_number as xs:integer) as xs:string")]
fn regex_group(interpreter: &mut Interpreter, group_number: IBig) -> error::Result<String> {
    let n: usize = (&group_number)
        .try_into()
        .map_err(|_| error::Error::FOAR0002)?;
    Ok(interpreter.regex_group(n))
}

#[xpath_fn("fn:xslt-message-terminate($namespace as xs:string, $local_name as xs:string, $prefix as xs:string) as item()*")]
fn xslt_message_terminate(
    namespace: &str,
    local_name: &str,
    prefix: &str,
) -> error::Result<sequence::Sequence> {
    let qname = OwnedName::new(
        local_name.to_string(),
        namespace.to_string(),
        prefix.to_string(),
    );
    Err(error::Error::Application(Box::new(
        error::ApplicationError::new(
            qname,
            "Processing terminated by xsl:message".to_string(),
        ),
    )))
}

#[xpath_fn("fn:transform($options as map(*)) as map(*)")]
fn fn_transform(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    options: function::Map,
) -> error::Result<function::Map> {
    let evaluator = context.transform_evaluator().ok_or_else(|| {
        error::Error::Unsupported("fn:transform is not configured for this program".to_string())
    })?;

    let request = crate::interpreter::TransformRequest { options };

    evaluator
        .transform(&request, context, interpreter)
        .map_err(|error| error.error)
}

#[xpath_fn("fn:xslt-where-populated($content as item()*) as item()*")]
fn xslt_where_populated(
    interpreter: &Interpreter,
    content: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    if is_populated(interpreter.xot(), content) {
        Ok(content.clone())
    } else {
        Ok(sequence::Sequence::default())
    }
}

/// Check whether a sequence is "populated" per XSLT 3.0 section 11.2.1.
/// A sequence is vacuous if every item is a zero-length text node, or an
/// element/document node whose children are all vacuous.
fn is_populated(xot: &Xot, sequence: &sequence::Sequence) -> bool {
    sequence.iter().any(|item| is_item_populated(xot, item))
}

fn is_item_populated(xot: &Xot, item: sequence::Item) -> bool {
    match item {
        sequence::Item::Node(node) => match xot.value(node) {
            xot::Value::Text(text) => !text.get().is_empty(),
            xot::Value::Element(_) | xot::Value::Document => {
                xot.children(node)
                    .any(|child| is_item_populated(xot, sequence::Item::Node(child)))
            }
            // PI, comment, attribute, namespace nodes count as populated
            _ => true,
        },
        // Atomic values and functions count as populated
        _ => true,
    }
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(simple_content),
        wrap_xpath_fn!(xslt_for_each_group_by),
        wrap_xpath_fn!(xslt_for_each_group_adjacent),
        wrap_xpath_fn!(current_group),
        wrap_xpath_fn!(current_grouping_key),
        wrap_xpath_fn!(xslt_try),
        wrap_xpath_fn!(resolve_xslt_qname),
        wrap_xpath_fn!(format_number_lexical2),
        wrap_xpath_fn!(format_number_lexical3),
        wrap_xpath_fn!(xslt_number_value),
        wrap_xpath_fn!(xslt_number_count_single),
        wrap_xpath_fn!(xslt_number_count_any),
        wrap_xpath_fn!(xslt_number_count_single_pattern),
        wrap_xpath_fn!(xslt_number_count_any_pattern),
        wrap_xpath_fn!(xslt_number_count_multiple),
        wrap_xpath_fn!(xslt_number_count_multiple_pattern),
        wrap_xpath_fn!(xslt_evaluate),
        wrap_xpath_fn!(store_result_document),
        wrap_xpath_fn!(store_principal_result_document),
        wrap_xpath_fn!(xslt_analyze_string),
        wrap_xpath_fn!(regex_group),
        wrap_xpath_fn!(xslt_message_terminate),
        wrap_xpath_fn!(fn_transform),
        wrap_xpath_fn!(xslt_where_populated),
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
