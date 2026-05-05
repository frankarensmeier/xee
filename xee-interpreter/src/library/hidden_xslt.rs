// functions used to implement the XSLT that aren't supposed to be
// exposed to XPath
use ahash::{HashMap, HashMapExt};

use ibig::IBig;
use iri_string::types::{IriReferenceStr, IriString};
use xee_name::{Name, Namespaces};
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
    "fn:xslt-for-each-group-by($seq as item()*, $key as function(*), $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
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

    let total_items: IBig = seq.len().into();
    for (index, item) in seq.iter().enumerate() {
        let position: IBig = (index + 1).into();
        let value = interpreter.call_function_with_arguments(
            &key_function,
            vec![
                item.clone().into(),
                atomic::Atomic::from(position).into(),
                atomic::Atomic::from(total_items.clone()).into(),
            ],
        )?;
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
        let total_groups: IBig = group_keys.len().into();
        for (idx, gk) in group_keys.iter().enumerate() {
            let first_item: sequence::Sequence = groups
                .get(gk)
                .and_then(|g| g.first())
                .map(|item| item.clone().into())
                .unwrap_or_default();
            let pos: IBig = (idx + 1).into();
            let sort_val = interpreter.call_function_with_arguments(
                &sort_key_fn,
                vec![first_item, pos.into(), total_groups.clone().into()],
            )?;
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
                    a_num
                        .partial_cmp(&b_num)
                        .unwrap_or(std::cmp::Ordering::Equal)
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
        let group_items: sequence::Sequence =
            groups.get(group_key).cloned().unwrap_or_default().into();
        let first_item: sequence::Sequence = group_items
            .iter()
            .next()
            .map(|item| item.clone().into())
            .unwrap_or_default();

        let position: IBig = (index + 1).into();

        interpreter.push_current_group(group_items, Some(group_key.clone()));
        let body_result = interpreter.call_function_with_arguments(
            &body_function,
            vec![
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
    "fn:xslt-for-each-group-adjacent($seq as item()*, $key as function(*), $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
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

    let total_items: IBig = seq.len().into();
    for (index, item) in seq.iter().enumerate() {
        let position: IBig = (index + 1).into();
        let value = interpreter.call_function_with_arguments(
            &key_function,
            vec![
                item.clone().into(),
                atomic::Atomic::from(position).into(),
                atomic::Atomic::from(total_items.clone()).into(),
            ],
        )?;
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
        let total_groups: IBig = group_items_list.len().into();
        for (i, group) in group_items_list.iter().enumerate() {
            let first_item: sequence::Sequence = group
                .first()
                .map(|item| item.clone().into())
                .unwrap_or_default();
            let pos: IBig = (i + 1).into();
            let sort_val = interpreter.call_function_with_arguments(
                &sort_key_fn,
                vec![first_item, pos.into(), total_groups.clone().into()],
            )?;
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
                    a_num
                        .partial_cmp(&b_num)
                        .unwrap_or(std::cmp::Ordering::Equal)
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
            vec![
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
    "fn:xslt-for-each-group-starting-with($seq as item()*, $pattern as function(*), $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
)]
fn xslt_for_each_group_starting_with(
    interpreter: &mut Interpreter,
    seq: &sequence::Sequence,
    pattern: sequence::Item,
    body: sequence::Item,
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<sequence::Sequence> {
    let matcher = pattern.to_function()?;
    let body_function = body.to_function()?;

    let mut groups: Vec<Vec<sequence::Item>> = Vec::new();
    let total_items = seq.len();
    for (index, item) in seq.iter().enumerate() {
        let is_start = matcher_matches_item(interpreter, &matcher, &item, index + 1, total_items)?;
        if is_start && !groups.is_empty() {
            groups.push(vec![item.clone()]);
        } else if let Some(group) = groups.last_mut() {
            group.push(item.clone());
        } else {
            groups.push(vec![item.clone()]);
        }
    }

    evaluate_pattern_groups(
        interpreter,
        &body_function,
        groups,
        sort_key,
        sort_descending,
        sort_numeric,
    )
}

#[xpath_fn(
    "fn:xslt-for-each-group-ending-with($seq as item()*, $pattern as function(*), $body as function(*), $sort_key as item()*, $sort_descending as xs:string, $sort_numeric as xs:string) as item()*"
)]
fn xslt_for_each_group_ending_with(
    interpreter: &mut Interpreter,
    seq: &sequence::Sequence,
    pattern: sequence::Item,
    body: sequence::Item,
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<sequence::Sequence> {
    let matcher = pattern.to_function()?;
    let body_function = body.to_function()?;

    let mut groups: Vec<Vec<sequence::Item>> = Vec::new();
    let mut current_group = Vec::new();
    let total_items = seq.len();
    for (index, item) in seq.iter().enumerate() {
        current_group.push(item.clone());
        if matcher_matches_item(interpreter, &matcher, &item, index + 1, total_items)? {
            groups.push(std::mem::take(&mut current_group));
        }
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }

    evaluate_pattern_groups(
        interpreter,
        &body_function,
        groups,
        sort_key,
        sort_descending,
        sort_numeric,
    )
}

#[xpath_fn("fn:current-group() as item()*")]
fn current_group(interpreter: &Interpreter) -> error::Result<sequence::Sequence> {
    Ok(interpreter.current_group())
}

#[xpath_fn("fn:current-grouping-key() as xs:anyAtomicType?")]
fn current_grouping_key(interpreter: &Interpreter) -> error::Result<sequence::Sequence> {
    Ok(interpreter.current_grouping_key())
}

#[xpath_fn("fn:copy-of($arg as item()*) as item()*", context_first)]
fn copy_of(
    interpreter: &mut Interpreter,
    arg: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    copy_sequence(interpreter, arg)
}

#[xpath_fn("fn:snapshot($arg as item()*) as item()*", context_first)]
fn snapshot(
    interpreter: &mut Interpreter,
    arg: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    let _ = interpreter;
    Ok(arg.clone())
}

#[xpath_fn("fn:strip-space-document($doc as node()?) as node()?")]
fn strip_space_document(
    interpreter: &mut Interpreter,
    doc: Option<xot::Node>,
) -> error::Result<Option<xot::Node>> {
    let Some(doc) = doc else {
        return Ok(None);
    };

    strip_whitespace_only_text_children(interpreter.xot_mut(), doc);
    Ok(Some(doc))
}

fn copy_sequence(
    interpreter: &mut Interpreter,
    arg: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    let mut result = Vec::with_capacity(arg.len());
    for item in arg.iter() {
        let copy = match &item {
            sequence::Item::Atomic(_) | sequence::Item::Function(_) => item.clone(),
            sequence::Item::Node(node) => sequence::Item::Node(interpreter.xot_mut().clone_node(*node)),
        };
        result.push(copy);
    }
    Ok(result.into())
}

fn evaluate_pattern_groups(
    interpreter: &mut Interpreter,
    body_function: &function::Function,
    groups: Vec<Vec<sequence::Item>>,
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<sequence::Sequence> {
    let sorted_indices = sort_group_indices(
        interpreter,
        &groups,
        sort_key,
        sort_descending,
        sort_numeric,
    )?;

    let mut result = Vec::new();
    let total_groups: IBig = sorted_indices.len().into();
    for (pos, &idx) in sorted_indices.iter().enumerate() {
        let group_seq: sequence::Sequence = groups[idx].clone().into();
        let first_item: sequence::Sequence = group_seq
            .iter()
            .next()
            .map(|item| item.clone().into())
            .unwrap_or_default();

        let position: IBig = (pos + 1).into();
        interpreter.push_current_group(group_seq, None);
        let body_result = interpreter.call_function_with_arguments(
            body_function,
            vec![
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

fn sort_group_indices(
    interpreter: &mut Interpreter,
    groups: &[Vec<sequence::Item>],
    sort_key: &sequence::Sequence,
    sort_descending: &str,
    sort_numeric: &str,
) -> error::Result<Vec<usize>> {
    if sort_key.is_empty() {
        return Ok((0..groups.len()).collect());
    }

    let sort_key_fn = sort_key.iter().next().unwrap().to_function()?;
    let mut keyed: Vec<(usize, atomic::Atomic)> = Vec::new();
    let total_groups: IBig = groups.len().into();
    for (i, group) in groups.iter().enumerate() {
        let first_item: sequence::Sequence = group
            .first()
            .map(|item| item.clone().into())
            .unwrap_or_default();
        let pos: IBig = (i + 1).into();
        let sort_val = interpreter.call_function_with_arguments(
            &sort_key_fn,
            vec![first_item, pos.into(), total_groups.clone().into()],
        )?;
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
                a_num
                    .partial_cmp(&b_num)
                    .unwrap_or(std::cmp::Ordering::Equal)
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

    Ok(keyed.into_iter().map(|(i, _)| i).collect())
}

fn matcher_matches_item(
    interpreter: &mut Interpreter,
    matcher: &function::Function,
    item: &sequence::Item,
    position: usize,
    size: usize,
) -> error::Result<bool> {
    let result = interpreter.call_function_with_arguments(
        matcher,
        vec![
            item.clone().into(),
            atomic::Atomic::from(position as u64).into(),
            atomic::Atomic::from(size as u64).into(),
        ],
    )?;
    let matched = result.effective_boolean_value()?;
    Ok(matched)
}

pub(crate) fn strip_whitespace_only_text_children(xot: &mut Xot, node: xot::Node) {
    let mut to_remove = Vec::new();
    for descendant in xot.descendants(node) {
        let xot::Value::Text(text) = xot.value(descendant) else {
            continue;
        };

        if !is_xml_whitespace(text.get()) {
            continue;
        }

        if xot.parent(descendant).is_some_and(|parent| xot.is_element(parent)) {
            to_remove.push(descendant);
        }
    }

    for node in to_remove {
        let _ = xot.remove(node);
    }
}

fn is_xml_whitespace(s: &str) -> bool {
    s.chars()
        .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

// Returns the absolute URI of the resource being written to by the current
// output destination. Returns empty sequence when the URI is not known.
#[xpath_fn("fn:current-output-uri() as xs:anyURI?")]
fn current_output_uri() -> error::Result<sequence::Sequence> {
    // Stub: we don't track output URIs yet, so always return empty sequence.
    Ok(sequence::Sequence::default())
}

// Returns the URI of an unparsed entity declared in the DTD.
// Stub: always returns empty string since we don't track DTD entities.
#[xpath_fn("fn:unparsed-entity-uri($entity as xs:string) as xs:anyURI")]
fn unparsed_entity_uri(_entity: &str) -> error::Result<String> {
    Ok(String::new())
}

// Returns the public identifier of an unparsed entity declared in the DTD.
// Stub: always returns empty string since we don't track DTD entities.
#[xpath_fn("fn:unparsed-entity-public-id($entity as xs:string) as xs:string")]
fn unparsed_entity_public_id(_entity: &str) -> error::Result<String> {
    Ok(String::new())
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
    // Track how many globals are resolving before the try body executes.
    // If the error came from a NEW global resolution (count increased), it
    // must not be caught (XSLT spec: errors during global variable evaluation
    // are not caught). But if the count is unchanged, the try/catch is inside
    // the global's own body and should work normally.
    let resolving_before = interpreter.resolving_global_variable_count();
    match interpreter.call_function_with_arguments_catching_spanned_with_rollback(
        &body,
        vec![],
        rollback_output,
    ) {
        Ok(result) => Ok(result),
        Err(caught_error) => {
            if interpreter.resolving_global_variable_count() > resolving_before {
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
                    .call_function_with_arguments_catching_spanned(&handler, handler_arguments)
                    .map_err(|error| error.error);
            }

            Err(caught_error.error)
        }
    }
}

#[xpath_fn(
    "fn:xslt-with-temporary-output-state($body as function(*)) as item()*",
    context_first
)]
fn xslt_with_temporary_output_state(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    body: sequence::Item,
) -> error::Result<sequence::Sequence> {
    let body = body.to_function()?;
    context.push_temporary_output_state();
    let result = interpreter.call_function_with_arguments(&body, vec![]);
    context.pop_temporary_output_state();
    result
}

#[xpath_fn(
    "fn:xslt-mark-temporary-tree($tree as item()*) as item()*",
    context_first
)]
fn xslt_mark_temporary_tree(
    context: &crate::context::DynamicContext,
    interpreter: &Interpreter,
    tree: &sequence::Sequence,
) -> error::Result<sequence::Sequence> {
    context.mark_temporary_tree(tree, interpreter.xot());
    Ok(tree.clone())
}

#[xpath_fn(
    "fn:xslt-unsupported-merge($sources as array(*), $key_functions as array(*)) as item()*"
)]
fn xslt_unsupported_merge(
    interpreter: &mut Interpreter,
    sources: function::Array,
    key_functions: function::Array,
) -> error::Result<sequence::Sequence> {
    for (source, key_function) in sources.iter().zip(key_functions.iter()) {
        let key_function = key_function.clone().one()?.to_function()?;
        for item in source.iter() {
            interpreter.call_function_with_arguments(&key_function, vec![item.clone().into()])?;
        }
    }

    Err(error::Error::Unsupported(
        "xsl:merge is not supported yet".to_string(),
    ))
}

#[xpath_fn("fn:accumulator-before($name as xs:string) as item()*")]
fn accumulator_before(_name: &str) -> error::Result<sequence::Sequence> {
    Err(error::Error::Unsupported(
        "xsl:accumulator is not supported yet".to_string(),
    ))
}

#[xpath_fn("fn:accumulator-after($name as xs:string) as item()*")]
fn accumulator_after(_name: &str) -> error::Result<sequence::Sequence> {
    Err(error::Error::Unsupported(
        "xsl:accumulator is not supported yet".to_string(),
    ))
}

#[xpath_fn("fn:xslt-accumulator-before($name as xs:string, $node as node()?) as item()*")]
fn xslt_accumulator_before(
    interpreter: &mut Interpreter,
    name: &str,
    node: Option<xot::Node>,
) -> error::Result<sequence::Sequence> {
    accumulator_probe(interpreter, name, node)
}

#[xpath_fn("fn:xslt-accumulator-after($name as xs:string, $node as node()?) as item()*")]
fn xslt_accumulator_after(
    interpreter: &mut Interpreter,
    name: &str,
    node: Option<xot::Node>,
) -> error::Result<sequence::Sequence> {
    accumulator_probe(interpreter, name, node)
}

fn accumulator_probe(
    interpreter: &mut Interpreter,
    name: &str,
    node: Option<xot::Node>,
) -> error::Result<sequence::Sequence> {
    let Some(name) = parse_accumulator_name(name) else {
        return Err(error::Error::Unsupported(
            "xsl:accumulator is not supported yet".to_string(),
        ));
    };
    let Some(accumulator) = interpreter
        .runnable()
        .program()
        .declarations
        .accumulator_by_name(&name)
        .cloned()
    else {
        return Err(error::Error::Unsupported(
            "xsl:accumulator is not supported yet".to_string(),
        ));
    };

    let Some(context_node) = node else {
        return Err(error::Error::Unsupported(
            "xsl:accumulator is not supported yet".to_string(),
        ));
    };
    let dynamic_context = interpreter.runnable().dynamic_context();
    let context_root = interpreter.xot().root(context_node);
    let differs_from_initial_context_root = dynamic_context
        .context_item()
        .and_then(|item| item.to_node().ok())
        .map(|initial_node| interpreter.xot().root(initial_node) != context_root)
        .unwrap_or(false);
    let is_temporary_tree = dynamic_context.is_temporary_tree_node(context_node, interpreter.xot())
        || differs_from_initial_context_root;

    if is_temporary_tree
        && accumulator
            .rules
            .iter()
            .any(|rule| rule.probe_temporary_output_state)
    {
        return Err(error::Error::XTDE1480);
    }

    Err(error::Error::Unsupported(
        "xsl:accumulator is not supported yet".to_string(),
    ))
}

fn parse_accumulator_name(name: &str) -> Option<OwnedName> {
    if let Some(rest) = name.strip_prefix("Q{") {
        let (namespace, local_name) = rest.split_once('}')?;
        return Some(OwnedName::new(
            local_name.to_string(),
            namespace.to_string(),
            String::new(),
        ));
    }

    if name.contains(':') {
        return None;
    }

    Some(OwnedName::new(
        name.to_string(),
        String::new(),
        String::new(),
    ))
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

#[xpath_fn("fn:format-number-lexical($value as xs:string, $picture as xs:string) as xs:string")]
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

#[xpath_fn(
    "fn:store-result-document($href as xs:string, $content as item()*, $format as xs:string, $format_namespaces as xs:string, $named_outputs as xs:string, $method as xs:string, $byte_order_mark as xs:string, $cdata as xs:string, $doctype_public as xs:string, $doctype_system as xs:string, $include_content_type as xs:string, $media_type as xs:string, $item_separator as xs:string, $omit_xml_declaration as xs:string, $standalone as xs:string, $html_version as xs:string, $use_character_maps as xs:string, $version as xs:string) as item()*",
    context_first
)]
fn store_result_document(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    href: &str,
    content: &sequence::Sequence,
    format: &str,
    format_namespaces: &str,
    named_outputs: &str,
    method: &str,
    byte_order_mark: &str,
    cdata: &str,
    doctype_public: &str,
    doctype_system: &str,
    include_content_type: &str,
    media_type: &str,
    item_separator: &str,
    omit_xml_declaration: &str,
    standalone: &str,
    html_version: &str,
    use_character_maps: &str,
    version: &str,
) -> error::Result<sequence::Sequence> {
    if context.in_temporary_output_state() {
        return Err(error::Error::XTDE1480);
    }

    let parameters = resolve_result_document_parameters(
        context,
        format,
        format_namespaces,
        named_outputs,
        method,
        byte_order_mark,
        cdata,
        doctype_public,
        doctype_system,
        include_content_type,
        media_type,
        item_separator,
        omit_xml_declaration,
        standalone,
        html_version,
        use_character_maps,
        version,
    )?;

    if href.is_empty() {
        context.store_principal_result_document(content.clone(), parameters);
        return Ok(sequence::Sequence::default());
    }

    let document = content.normalize(&parameters.item_separator, interpreter.xot_mut())?;
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
        parameters,
    );
    Ok(sequence::Sequence::default())
}

#[xpath_fn(
    "fn:store-principal-result-document($content as item()*, $format as xs:string, $format_namespaces as xs:string, $named_outputs as xs:string, $method as xs:string, $byte_order_mark as xs:string, $cdata as xs:string, $doctype_public as xs:string, $doctype_system as xs:string, $include_content_type as xs:string, $media_type as xs:string, $item_separator as xs:string, $omit_xml_declaration as xs:string, $standalone as xs:string, $html_version as xs:string, $use_character_maps as xs:string, $version as xs:string) as item()*",
    context_first
)]
fn store_principal_result_document(
    context: &crate::context::DynamicContext,
    content: &sequence::Sequence,
    format: &str,
    format_namespaces: &str,
    named_outputs: &str,
    method: &str,
    byte_order_mark: &str,
    cdata: &str,
    doctype_public: &str,
    doctype_system: &str,
    include_content_type: &str,
    media_type: &str,
    item_separator: &str,
    omit_xml_declaration: &str,
    standalone: &str,
    html_version: &str,
    use_character_maps: &str,
    version: &str,
) -> error::Result<sequence::Sequence> {
    if context.in_temporary_output_state() {
        return Err(error::Error::XTDE1480);
    }

    let parameters = resolve_result_document_parameters(
        context,
        format,
        format_namespaces,
        named_outputs,
        method,
        byte_order_mark,
        cdata,
        doctype_public,
        doctype_system,
        include_content_type,
        media_type,
        item_separator,
        omit_xml_declaration,
        standalone,
        html_version,
        use_character_maps,
        version,
    )?;
    context.store_principal_result_document(content.clone(), parameters);
    Ok(sequence::Sequence::default())
}

fn resolve_result_document_parameters(
    context: &crate::context::DynamicContext,
    format: &str,
    format_namespaces: &str,
    named_outputs: &str,
    method: &str,
    byte_order_mark: &str,
    cdata: &str,
    doctype_public: &str,
    doctype_system: &str,
    include_content_type: &str,
    media_type: &str,
    item_separator: &str,
    omit_xml_declaration: &str,
    standalone: &str,
    html_version: &str,
    use_character_maps: &str,
    version: &str,
) -> error::Result<sequence::SerializationParameters> {
    let mut parameters = context.serialization_parameters().clone();
    let merge_with_named_output = if format.is_empty() {
        false
    } else {
        let named_output = resolve_named_output_parameters(
            context,
            format,
            format_namespaces,
            named_outputs,
        )?;
        apply_serialization_parameter_strings(
            context,
            &mut parameters,
            &named_output.method,
            &named_output.byte_order_mark,
            &named_output.cdata,
            &named_output.doctype_public,
            &named_output.doctype_system,
            &named_output.include_content_type,
            &named_output.media_type,
            &named_output.item_separator,
            &named_output.omit_xml_declaration,
            &named_output.standalone,
            &named_output.html_version,
            &named_output.use_character_maps,
            &named_output.version,
            false,
        )?;
        true
    };
    apply_serialization_parameter_strings(
        context,
        &mut parameters,
        method,
        byte_order_mark,
        cdata,
        doctype_public,
        doctype_system,
        include_content_type,
        media_type,
        item_separator,
        omit_xml_declaration,
        standalone,
        html_version,
        use_character_maps,
        version,
        merge_with_named_output,
    )?;
    Ok(parameters)
}

#[derive(Debug, Clone)]
struct EncodedNamedOutput {
    method: String,
    byte_order_mark: String,
    cdata: String,
    doctype_public: String,
    doctype_system: String,
    include_content_type: String,
    media_type: String,
    item_separator: String,
    omit_xml_declaration: String,
    standalone: String,
    html_version: String,
    use_character_maps: String,
    version: String,
}

fn apply_serialization_parameter_strings(
    context: &crate::context::DynamicContext,
    parameters: &mut sequence::SerializationParameters,
    method: &str,
    byte_order_mark: &str,
    cdata: &str,
    doctype_public: &str,
    doctype_system: &str,
    include_content_type: &str,
    media_type: &str,
    item_separator: &str,
    omit_xml_declaration: &str,
    standalone: &str,
    html_version: &str,
    use_character_maps: &str,
    version: &str,
    merge_with_existing: bool,
) -> error::Result<()> {
    if !method.is_empty() {
        parameters.method = sequence::QNameOrString::String(method.to_string());
        parameters.explicit_method = true;
    }
    if !byte_order_mark.is_empty() {
        parameters.byte_order_mark = parse_boolean(byte_order_mark)?;
    }
    if !cdata.is_empty() {
        let parsed = parse_cdata_section_elements(context, cdata)?;
        if merge_with_existing {
            for name in parsed {
                if !parameters
                    .cdata_section_elements
                    .iter()
                    .any(|existing| existing == &name)
                {
                    parameters.cdata_section_elements.push(name);
                }
            }
        } else {
            parameters.cdata_section_elements = parsed;
        }
    }
    if !doctype_system.is_empty() {
        parameters.doctype_system = Some(doctype_system.to_string());
        if !doctype_public.is_empty() {
            parameters.doctype_public = Some(doctype_public.to_string());
        }
    } else if merge_with_existing && !doctype_public.is_empty() && parameters.doctype_system.is_some() {
        parameters.doctype_public = Some(doctype_public.to_string());
    }
    if !include_content_type.is_empty() {
        parameters.include_content_type = matches!(include_content_type, "yes" | "true" | "1");
    }
    if !media_type.is_empty() {
        parameters.media_type = Some(media_type.to_string());
    }
    if !item_separator.is_empty() {
        parameters.item_separator = if item_separator == "#absent" {
            sequence::SerializationParameters::default().item_separator
        } else {
            item_separator.to_string()
        };
    }
    if !omit_xml_declaration.is_empty() {
        parameters.omit_xml_declaration = parse_boolean(omit_xml_declaration)?;
    }
    if !standalone.is_empty() {
        parameters.standalone = parse_standalone(standalone)?;
        // Explicitly setting standalone (yes, no, or omit) implies the
        // XML declaration should be output.
        parameters.omit_xml_declaration = false;
    }
    if !html_version.is_empty() {
        parameters.html_version = rust_decimal::Decimal::from_str_exact(html_version)
            .map_err(|_| error::Error::SEPM0016)?;
        parameters.explicit_html_version = true;
    }
    if !use_character_maps.is_empty() {
        let parsed = parse_character_maps(use_character_maps)?;
        if merge_with_existing {
            for (character, replacement) in parsed {
                parameters.use_character_maps.insert(character, replacement);
            }
        } else {
            parameters.use_character_maps = parsed;
        }
    }
    if !version.is_empty() {
        parameters.version = version.to_string();
    }
    Ok(())
}

fn resolve_named_output_parameters(
    context: &crate::context::DynamicContext,
    format: &str,
    format_namespaces: &str,
    named_outputs: &str,
) -> error::Result<EncodedNamedOutput> {
    let (namespace, local_name) = resolve_result_document_format(context, format, format_namespaces)?;
    for entry in named_outputs.split(';') {
        if entry.is_empty() {
            continue;
        }
        let fields = entry.split(',').collect::<Vec<_>>();
        if fields.len() != 15 {
            return Err(error::Error::FOCA0002);
        }
        let entry_namespace = decode_hex(fields[0])?;
        let entry_local_name = decode_hex(fields[1])?;
        if entry_namespace == namespace && entry_local_name == local_name {
            return Ok(EncodedNamedOutput {
                method: decode_hex(fields[2])?,
                byte_order_mark: decode_hex(fields[3])?,
                cdata: decode_hex(fields[4])?,
                doctype_public: decode_hex(fields[5])?,
                doctype_system: decode_hex(fields[6])?,
                include_content_type: decode_hex(fields[7])?,
                media_type: decode_hex(fields[8])?,
                item_separator: decode_hex(fields[9])?,
                omit_xml_declaration: decode_hex(fields[10])?,
                standalone: decode_hex(fields[11])?,
                html_version: decode_hex(fields[12])?,
                use_character_maps: decode_hex(fields[13])?,
                version: decode_hex(fields[14])?,
            });
        }
    }
    Err(error::Error::Unsupported(
        "Unknown xsl:result-document @format".to_string(),
    ))
}

fn resolve_result_document_format(
    context: &crate::context::DynamicContext,
    format: &str,
    format_namespaces: &str,
) -> error::Result<(String, String)> {
    let format = format.trim();
    if let Some(rest) = format.strip_prefix("Q{") {
        let Some(close_brace) = rest.find('}') else {
            return Err(error::Error::XTSE0020);
        };
        let namespace = &rest[..close_brace];
        let local_name = &rest[close_brace + 1..];
        if local_name.is_empty() {
            return Err(error::Error::XTSE0020);
        }
        return Ok((namespace.to_string(), local_name.to_string()));
    }

    if let Some((prefix, local_name)) = format.split_once(':') {
        let local_namespaces = decode_namespaces(format_namespaces, "")?;
        let namespace = local_namespaces
            .by_prefix(prefix)
            .or_else(|| context.static_context().namespaces().by_prefix(prefix))
            .ok_or(error::Error::XTSE0020)?;
        return Ok((namespace.to_string(), local_name.to_string()));
    }

    Ok((String::new(), format.to_string()))
}

#[xpath_fn(
    "fn:xslt-evaluate-put-param($with_params as item()*, $key as xs:QName, $value as item()*) as map(*)"
)]
fn xslt_evaluate_put_param(
    with_params: &sequence::Sequence,
    key: Name,
    value: &sequence::Sequence,
) -> error::Result<function::Map> {
    let map = match with_params.clone().option()? {
        None => function::Map::new(Vec::new())?,
        Some(sequence::Item::Function(function::Function::Map(map))) => map,
        Some(_) => {
            return Err(error::Error::type_error(
                "xsl:evaluate with-params must be a map",
            ))
        }
    };

    let key_atomic: atomic::Atomic = key.into();
    if map.get(&key_atomic).is_some() {
        Ok(map)
    } else {
        map.put(key_atomic, value)
    }
}

#[xpath_fn(
    "fn:xslt-evaluate($xpath as xs:string?, $context_item as item()*, $context_item_supplied as item()*, $xpath_default_namespace as xs:string, $default_collation as xs:string, $namespace_context as item()*, $with_params as item()*, $base_uri as xs:string?) as item()*"
)]
fn xslt_evaluate(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    xpath: Option<&str>,
    context_item: &sequence::Sequence,
    context_item_supplied: &sequence::Sequence,
    xpath_default_namespace: &str,
    default_collation: &str,
    namespace_context: &sequence::Sequence,
    with_params: &sequence::Sequence,
    base_uri: Option<&str>,
) -> error::Result<sequence::Sequence> {
    let evaluator = context.dynamic_xpath_evaluator().ok_or_else(|| {
        error::Error::Unsupported("xsl:evaluate is not configured for this program".to_string())
    })?;

    let context_item_supplied = !context_item_supplied.is_empty();
    let context_item = if context_item_supplied {
        context_item
            .clone()
            .option()
            .map_err(|_| error::Error::XTTE3210)?
    } else {
        None
    };
    let namespace_context = namespace_context.clone().option()?;
    let with_params = match with_params.clone().option()? {
        None => None,
        Some(sequence::Item::Function(function::Function::Map(map))) => Some(map),
        Some(_) => {
            return Err(error::Error::type_error(
                "xsl:evaluate with-params must be a map",
            ))
        }
    };

    let request = crate::interpreter::DynamicXPathRequest {
        xpath: xpath
            .ok_or(error::Error::type_error(
                "xsl:evaluate xpath expression is empty",
            ))?
            .to_string(),
        context_item,
        context_item_supplied,
        xpath_default_namespace: xpath_default_namespace.to_string(),
        default_collation: default_collation.to_string(),
        namespace_context,
        with_params,
        base_uri: base_uri.map(str::to_string),
    };

    evaluator
        .evaluate(&request, context, interpreter)
        .map_err(|error| normalize_xslt_evaluate_error(error.error))
}

fn normalize_xslt_evaluate_error(error: error::Error) -> error::Error {
    match error {
        error::Error::FODC0002 => error::Error::XTDE3160,
        error::Error::Application(application_error)
            if application_error.qname().local_name() == "FODC0002"
                && application_error.qname().namespace() == "http://www.w3.org/2005/xqt-errors" =>
        {
            error::Error::XTDE3160
        }
        other => other,
    }
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
    let analyze_results: Vec<AnalyzeEntry> = compiled_regex.analyze(input)?.collect();

    let match_function = match_fn.to_function()?;
    let non_match_function = non_match_fn.to_function()?;

    let mut result = Vec::new();
    let total_parts: IBig = analyze_results.len().into();
    for (index, entry) in analyze_results.into_iter().enumerate() {
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
        let position = IBig::from(index + 1);
        if is_match {
            if let AnalyzeEntry::Match(match_entries) = &entry {
                let groups = extract_regex_groups(match_entries);
                interpreter.push_regex_groups(groups);
            }
        }
        let items = interpreter.call_function_with_arguments(
            function,
            vec![
                arg,
                atomic::Atomic::from(position).into(),
                atomic::Atomic::from(total_parts.clone()).into(),
            ],
        )?;
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

#[xpath_fn("fn:xslt-message($content as item()*) as item()*")]
fn xslt_message(interpreter: &Interpreter, content: &sequence::Sequence) -> sequence::Sequence {
    let xot = interpreter.xot();
    let mut parts = Vec::new();
    for item in content.iter() {
        match item.string_value(xot) {
            Ok(s) => parts.push(s),
            Err(_) => parts.push("[error]".to_string()),
        }
    }
    eprintln!("{}", parts.join(""));
    sequence::Sequence::default()
}

#[xpath_fn("fn:xslt-message-terminate($content as item()*, $namespace as xs:string, $local_name as xs:string, $prefix as xs:string) as item()*")]
fn xslt_message_terminate(
    interpreter: &Interpreter,
    content: &sequence::Sequence,
    namespace: &str,
    local_name: &str,
    prefix: &str,
) -> error::Result<sequence::Sequence> {
    // Print the message body to stderr before terminating
    let xot = interpreter.xot();
    let mut parts = Vec::new();
    for item in content.iter() {
        match item.string_value(xot) {
            Ok(s) => parts.push(s),
            Err(_) => parts.push("[error]".to_string()),
        }
    }
    if !parts.is_empty() {
        eprintln!("{}", parts.join(""));
    }

    let qname = OwnedName::new(
        local_name.to_string(),
        namespace.to_string(),
        prefix.to_string(),
    );
    Err(error::Error::Application(Box::new(
        error::ApplicationError::new(qname, "Processing terminated by xsl:message".to_string()),
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
            xot::Value::Element(_) | xot::Value::Document => xot
                .children(node)
                .any(|child| is_item_populated(xot, sequence::Item::Node(child))),
            // PI, comment, attribute, namespace nodes count as populated
            _ => true,
        },
        // Atomic values and functions count as populated
        _ => true,
    }
}

#[xpath_fn("fn:xslt-sort($input as item()*, $collation as xs:string?, $key as function(item(), item(), item()) as xs:anyAtomicType*) as item()*")]
fn xslt_sort3(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    input: &sequence::Sequence,
    collation: Option<&str>,
    key: sequence::Item,
) -> error::Result<sequence::Sequence> {
    let collation = context.static_context().resolve_collation_str(collation)?;
    let function = key.to_function()?;
    input.sorted_by_key_indexed(
        context,
        collation,
        |item, position, last| {
            let pos_seq: sequence::Sequence = IBig::from(position).into();
            let last_seq: sequence::Sequence = IBig::from(last).into();
            let value = interpreter.call_function_with_arguments(
                &function,
                vec![item.clone().into(), pos_seq, last_seq],
            )?;
            Ok(value)
        },
        false,
    )
}

#[xpath_fn("fn:xslt-sort-descending($input as item()*, $collation as xs:string?) as item()*")]
fn xslt_sort_descending2(
    context: &crate::context::DynamicContext,
    interpreter: &Interpreter,
    input: &sequence::Sequence,
    collation: Option<&str>,
) -> error::Result<sequence::Sequence> {
    let collation = context.static_context().resolve_collation_str(collation)?;
    input.sorted_by_key_descending(context, collation, |item| {
        let seq: sequence::Sequence = item.into();
        seq.atomized(interpreter.xot())
            .collect::<error::Result<sequence::Sequence>>()
    })
}

#[xpath_fn("fn:xslt-sort-descending($input as item()*, $collation as xs:string?, $key as function(item(), item(), item()) as xs:anyAtomicType*) as item()*")]
fn xslt_sort_descending3(
    context: &crate::context::DynamicContext,
    interpreter: &mut Interpreter,
    input: &sequence::Sequence,
    collation: Option<&str>,
    key: sequence::Item,
) -> error::Result<sequence::Sequence> {
    let collation = context.static_context().resolve_collation_str(collation)?;
    let function = key.to_function()?;
    input.sorted_by_key_indexed(
        context,
        collation,
        |item, position, last| {
            let pos_seq: sequence::Sequence = IBig::from(position).into();
            let last_seq: sequence::Sequence = IBig::from(last).into();
            let value = interpreter.call_function_with_arguments(
                &function,
                vec![item.clone().into(), pos_seq, last_seq],
            )?;
            Ok(value)
        },
        true,
    )
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(simple_content),
        wrap_xpath_fn!(xslt_for_each_group_by),
        wrap_xpath_fn!(xslt_for_each_group_adjacent),
        wrap_xpath_fn!(xslt_for_each_group_starting_with),
        wrap_xpath_fn!(xslt_for_each_group_ending_with),
        wrap_xpath_fn!(current_group),
        wrap_xpath_fn!(current_grouping_key),
        wrap_xpath_fn!(copy_of),
        wrap_xpath_fn!(snapshot),
        wrap_xpath_fn!(strip_space_document),
        wrap_xpath_fn!(current_output_uri),
        wrap_xpath_fn!(unparsed_entity_uri),
        wrap_xpath_fn!(unparsed_entity_public_id),
        wrap_xpath_fn!(xslt_try),
        wrap_xpath_fn!(xslt_with_temporary_output_state),
        wrap_xpath_fn!(xslt_mark_temporary_tree),
        wrap_xpath_fn!(xslt_unsupported_merge),
        wrap_xpath_fn!(accumulator_before),
        wrap_xpath_fn!(accumulator_after),
        wrap_xpath_fn!(xslt_accumulator_before),
        wrap_xpath_fn!(xslt_accumulator_after),
        wrap_xpath_fn!(resolve_xslt_qname),
        wrap_xpath_fn!(format_number_lexical2),
        wrap_xpath_fn!(format_number_lexical3),
        wrap_xpath_fn!(xslt_evaluate_put_param),
        wrap_xpath_fn!(xslt_evaluate),
        wrap_xpath_fn!(store_result_document),
        wrap_xpath_fn!(store_principal_result_document),
        wrap_xpath_fn!(xslt_analyze_string),
        wrap_xpath_fn!(regex_group),
        wrap_xpath_fn!(xslt_message),
        wrap_xpath_fn!(xslt_message_terminate),
        wrap_xpath_fn!(fn_transform),
        wrap_xpath_fn!(xslt_where_populated),
        wrap_xpath_fn!(xslt_sort3),
        wrap_xpath_fn!(xslt_sort_descending2),
        wrap_xpath_fn!(xslt_sort_descending3),
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
