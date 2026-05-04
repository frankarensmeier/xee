// xsl:number runtime implementation: counting and formatting.

use ahash::HashMapExt;
use ibig::IBig;
use rust_decimal::RoundingStrategy;
use xee_xpath_ast::Pattern;
use xee_xpath_macros::xpath_fn;
use xot::Xot;

use crate::atomic;
use crate::error;
use crate::function;
use crate::function::StaticFunctionDescription;
use crate::interpreter::Interpreter;
use crate::library::number_count_cache::{CacheKey, CountPatternKey, NumberCountEntry};
use crate::pattern::PredicateMatcher;
use crate::sequence;
use crate::wrap_xpath_fn;

#[xpath_fn("fn:xslt-number-value($value as item()*, $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string")]
fn xslt_number_value(
    interpreter: &Interpreter,
    value: &sequence::Sequence,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa_values = parse_start_at_values(start_at);
    let atomized = value.atomized(interpreter.xot());

    // Convert each item to an integer per XSLT spec (round to nearest)
    let mut numbers: Vec<i64> = Vec::new();
    for item in atomized {
        let atomic = item?;
        let number = atomic_to_number_value(&atomic)?;
        numbers.push(number);
    }

    if numbers.is_empty() {
        // Empty sequence: format with empty list
        return format_xslt_number_values(&[], format.unwrap_or("1"), gs, gsz);
    }

    // Apply per-position start-at adjustments
    apply_start_at_multiple(&mut numbers, &sa_values);
    format_xslt_number_values(&numbers, format.unwrap_or("1"), gs, gsz)
}

/// Convert an atomic value to an i64 for xsl:number formatting.
/// NaN and infinity produce XTDE0980.
/// Negative values produce XTDE0980.
fn atomic_to_number_value(atomic: &atomic::Atomic) -> error::Result<i64> {
    let number = match atomic {
        atomic::Atomic::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                return Err(error::Error::XTDE0980);
            }
            f.round() as i64
        }
        atomic::Atomic::Double(d) => {
            if d.is_nan() || d.is_infinite() {
                return Err(error::Error::XTDE0980);
            }
            d.round() as i64
        }
        atomic::Atomic::Decimal(d) => {
            // XSLT spec: round towards positive infinity (half-up)
            let rounded = d.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
            i64::try_from(rounded).map_err(|_| error::Error::XTDE0980)?
        }
        atomic::Atomic::Integer(_, i) => {
            i64::try_from(i.as_ref()).map_err(|_| error::Error::XTDE0980)?
        }
        _ => {
            // Try to parse as a number (for string/untyped values)
            match atomic.string_value().parse::<f64>() {
                Ok(d) => {
                    if d.is_nan() || d.is_infinite() {
                        return Err(error::Error::XTDE0980);
                    }
                    d.round() as i64
                }
                Err(_) => return Err(error::Error::XTDE0980),
            }
        }
    };
    if number < 0 {
        return Err(error::Error::XTDE0980);
    }
    Ok(number)
}

// xsl:number level="single" with default count pattern (no explicit count/from).
// Counts 1 + preceding siblings that match the same node kind and expanded-QName.
#[xpath_fn("fn:xslt-number-count-single($node as node(), $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string")]
fn xslt_number_count_single(
    interpreter: &Interpreter,
    node: xot::Node,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa = parse_start_at(start_at);
    let xot = interpreter.xot();
    let count = count_single_level(xot, node);
    if count == 0 {
        return format_xslt_number_values(&[], format.unwrap_or("1"), gs, gsz);
    }
    let adjusted = count + sa - 1;
    format_xslt_number_value(adjusted, format.unwrap_or("1"), gs, gsz)
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
#[xpath_fn("fn:xslt-number-count-any($node as node(), $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string")]
fn xslt_number_count_any(
    interpreter: &mut Interpreter,
    node: xot::Node,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa = parse_start_at(start_at);
    let count = count_any_level(interpreter, node);
    if count == 0 {
        return format_xslt_number_values(&[], format.unwrap_or("1"), gs, gsz);
    }
    let adjusted = count + sa - 1;
    format_xslt_number_value(adjusted, format.unwrap_or("1"), gs, gsz)
}

/// For level="any" with default count pattern: count all nodes preceding
/// (or equal to) the current node in document order that match the same
/// node kind and expanded-QName. Uses prefix-sum cache for O(1) lookups.
fn count_any_level(interpreter: &mut Interpreter, node: xot::Node) -> i64 {
    // Attribute nodes are not included in descendants(), so fall back to the
    // old reverse_document_order algorithm for them.
    if interpreter.xot().value_type(node) == xot::ValueType::Attribute {
        return count_any_level_attribute(interpreter.xot(), node);
    }

    let xot = interpreter.xot();
    let target_value_type = xot.value_type(node);
    let target_name = match xot.value(node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };
    let doc_root = xot.root(node);

    let count_key = CountPatternKey::Default(target_value_type, target_name);
    let cache_key = CacheKey {
        doc_root,
        count_key: count_key.clone(),
        from_index: -1,
    };

    // Check cache first
    if let Some(entry) = interpreter.number_count_cache.get(&cache_key) {
        return entry.lookup(node);
    }

    // Build cache: walk entire document forward
    // Note: descendants() includes the root node itself.
    let all_nodes: Vec<_> = interpreter.xot().descendants(doc_root).collect();

    let mut node_to_pos = ahash::HashMap::with_capacity(all_nodes.len());
    let mut prefix_sum = Vec::with_capacity(all_nodes.len());
    let mut running_count: i64 = 0;

    for (pos, &n) in all_nodes.iter().enumerate() {
        node_to_pos.insert(n, pos);
        if node_matches_default_count(interpreter.xot(), n, target_value_type, target_name) {
            running_count += 1;
        }
        prefix_sum.push(running_count);
    }

    let entry = NumberCountEntry::new(node_to_pos, prefix_sum, Vec::new());
    interpreter.number_count_cache.insert(cache_key.clone(), entry);
    interpreter.number_count_cache.get(&cache_key).unwrap().lookup(node)
}

/// Fallback for attribute context nodes: walk backward via reverse_document_order.
/// Attribute nodes are not included in descendants() so we can't use the cache.
fn count_any_level_attribute(xot: &Xot, node: xot::Node) -> i64 {
    let target_value_type = xot.value_type(node);
    let target_name = match xot.value(node) {
        xot::Value::Element(el) => Some(el.name()),
        xot::Value::Attribute(attr) => Some(attr.name()),
        _ => None,
    };

    let mut count: i64 = if node_matches_default_count(xot, node, target_value_type, target_name) {
        1
    } else {
        0
    };

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
    "fn:xslt-number-count-single-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string"
)]
fn xslt_number_count_single_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa = parse_start_at(start_at);
    let count_index: i64 = (&count_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let from_index: i64 = (&from_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let count = count_single_level_pattern(interpreter, node, count_index, from_index);
    if count == 0 {
        return format_xslt_number_values(&[], format.unwrap_or("1"), gs, gsz);
    }
    let adjusted = count + sa - 1;
    format_xslt_number_value(adjusted, format.unwrap_or("1"), gs, gsz)
}

fn count_single_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    let use_default_count = count_index < 0;
    let count_pattern = clone_number_pattern(interpreter, count_index);
    let from_pattern = clone_number_pattern(interpreter, from_index);

    // Collect ancestor-or-self first to avoid borrow conflicts
    let ancestor_or_self: Vec<_> = std::iter::once(node)
        .chain(interpreter.xot().ancestors(node))
        .collect();

    // Walk ancestor-or-self to find the first node matching the count pattern.
    // If from is specified, stop at the first ancestor matching from (but only
    // if it doesn't also match the count pattern — a node can match both).
    let count_node = {
        let mut found = None;
        for n in ancestor_or_self {
            // Check count pattern first: if this node matches count, use it
            if use_default_count {
                if node_matches_default_count_for(interpreter.xot(), node, n) {
                    found = Some(n);
                    break;
                }
            } else if let Some(ref count_pattern) = count_pattern {
                if node_matches_pattern(interpreter, n, count_pattern) {
                    found = Some(n);
                    break;
                }
            }
            // Then check from: if this ancestor matches from, stop searching
            if let Some(ref from_pattern) = from_pattern {
                if node_matches_pattern(interpreter, n, from_pattern) {
                    break;
                }
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
        } else if let Some(ref count_pattern) = count_pattern {
            if node_matches_pattern(interpreter, sibling, count_pattern) {
                count += 1;
            }
        }
    }
    count
}

// xsl:number level="any" with compiled count/from patterns.
#[xpath_fn(
    "fn:xslt-number-count-any-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string"
)]
fn xslt_number_count_any_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa = parse_start_at(start_at);
    let count_index: i64 = (&count_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let from_index: i64 = (&from_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let count = count_any_level_pattern(interpreter, node, count_index, from_index);
    if count == 0 {
        return format_xslt_number_values(&[], format.unwrap_or("1"), gs, gsz);
    }
    let adjusted = count + sa - 1;
    format_xslt_number_value(adjusted, format.unwrap_or("1"), gs, gsz)
}

fn count_any_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    // Attribute nodes are not included in descendants(), so fall back to the
    // old reverse_document_order algorithm for them.
    if interpreter.xot().value_type(node) == xot::ValueType::Attribute {
        return count_any_level_pattern_attribute(interpreter, node, count_index, from_index);
    }

    let use_default_count = count_index < 0;
    let doc_root = interpreter.xot().root(node);

    // Build the cache key
    let count_key = if use_default_count {
        let xot = interpreter.xot();
        let target_value_type = xot.value_type(node);
        let target_name = match xot.value(node) {
            xot::Value::Element(el) => Some(el.name()),
            xot::Value::Attribute(attr) => Some(attr.name()),
            _ => None,
        };
        CountPatternKey::Default(target_value_type, target_name)
    } else {
        CountPatternKey::PatternIndex(count_index)
    };

    let cache_key = CacheKey {
        doc_root,
        count_key: count_key.clone(),
        from_index,
    };

    // Check cache first
    if let Some(entry) = interpreter.number_count_cache.get(&cache_key) {
        return entry.lookup(node);
    }

    // Build cache: walk entire document forward, evaluate patterns once per node
    let count_pattern = clone_number_pattern(interpreter, count_index);
    let from_pattern = clone_number_pattern(interpreter, from_index);

    // Note: descendants() includes the root node itself.
    let all_nodes: Vec<_> = interpreter.xot().descendants(doc_root).collect();

    let mut node_to_pos = ahash::HashMap::with_capacity(all_nodes.len());
    let mut prefix_sum = Vec::with_capacity(all_nodes.len());
    let mut from_positions = Vec::new();
    let mut running_count: i64 = 0;

    for (pos, &n) in all_nodes.iter().enumerate() {
        node_to_pos.insert(n, pos);

        // Check from pattern
        if let Some(ref from_pattern) = from_pattern {
            if node_matches_pattern(interpreter, n, from_pattern) {
                from_positions.push(pos);
            }
        }

        // Check count pattern
        let matches_count = if use_default_count {
            node_matches_default_count_for(interpreter.xot(), node, n)
        } else if let Some(ref count_pattern) = count_pattern {
            node_matches_pattern(interpreter, n, count_pattern)
        } else {
            false
        };

        if matches_count {
            running_count += 1;
        }
        prefix_sum.push(running_count);
    }

    let entry = NumberCountEntry::new(node_to_pos, prefix_sum, from_positions);
    interpreter.number_count_cache.insert(cache_key.clone(), entry);
    interpreter.number_count_cache.get(&cache_key).unwrap().lookup(node)
}

/// Fallback for attribute context nodes: walk backward via reverse_document_order.
/// Attribute nodes are not included in descendants() so we can't use the cache.
fn count_any_level_pattern_attribute(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    let use_default_count = count_index < 0;
    let count_pattern = clone_number_pattern(interpreter, count_index);
    let from_pattern = clone_number_pattern(interpreter, from_index);

    let mut count: i64 = if use_default_count {
        if node_matches_default_count_for(interpreter.xot(), node, node) {
            1
        } else {
            0
        }
    } else if let Some(ref count_pattern) = count_pattern {
        if node_matches_pattern(interpreter, node, count_pattern) {
            1
        } else {
            0
        }
    } else {
        0
    };

    let preceding: Vec<_> = reverse_document_order(interpreter.xot(), node).collect();

    for n in preceding {
        if let Some(ref from_pattern) = from_pattern {
            if node_matches_pattern(interpreter, n, from_pattern) {
                break;
            }
        }
        if use_default_count {
            if node_matches_default_count_for(interpreter.xot(), node, n) {
                count += 1;
            }
        } else if let Some(ref count_pattern) = count_pattern {
            if node_matches_pattern(interpreter, n, count_pattern) {
                count += 1;
            }
        }
    }

    count
}

// xsl:number level="multiple" with default count pattern (no explicit count/from).
// For each ancestor-or-self matching the default count, count 1 + preceding siblings matching.
// Returns the formatted multi-value string.
#[xpath_fn("fn:xslt-number-count-multiple($node as node(), $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string")]
fn xslt_number_count_multiple(
    interpreter: &Interpreter,
    node: xot::Node,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa_values = parse_start_at_values(start_at);
    let xot = interpreter.xot();
    let mut numbers = count_multiple_level(xot, node);
    apply_start_at_multiple(&mut numbers, &sa_values);
    format_xslt_number_values(&numbers, format.unwrap_or("1"), gs, gsz)
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
    "fn:xslt-number-count-multiple-pattern($node as node(), $count_index as xs:integer, $from_index as xs:integer, $format as xs:string?, $grouping_separator as xs:string?, $grouping_size as xs:string?, $start_at as xs:string?) as xs:string"
)]
fn xslt_number_count_multiple_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: IBig,
    from_index: IBig,
    format: Option<&str>,
    grouping_separator: Option<&str>,
    grouping_size: Option<&str>,
    start_at: Option<&str>,
) -> error::Result<String> {
    let gs = parse_grouping_separator(grouping_separator);
    let gsz = parse_grouping_size(grouping_size);
    let sa_values = parse_start_at_values(start_at);
    let count_index: i64 = (&count_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let from_index: i64 = (&from_index)
        .try_into()
        .map_err(|_| error::Error::XPTY0004(None))?;
    let mut numbers = count_multiple_level_pattern(interpreter, node, count_index, from_index);
    apply_start_at_multiple(&mut numbers, &sa_values);
    format_xslt_number_values(&numbers, format.unwrap_or("1"), gs, gsz)
}

fn count_multiple_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> Vec<i64> {
    let use_default_count = count_index < 0;
    let count_pattern = clone_number_pattern(interpreter, count_index);
    let from_pattern = clone_number_pattern(interpreter, from_index);

    // Collect ancestor-or-self first to avoid borrow conflicts
    // xot.ancestors() includes self (indextree semantics)
    let ancestor_or_self: Vec<_> = interpreter.xot().ancestors(node).collect();

    let mut numbers = Vec::new();
    for n in &ancestor_or_self {
        let n = *n;
        // If we hit a from-boundary ancestor, stop
        if let Some(ref from_pattern) = from_pattern {
            if node_matches_pattern(interpreter, n, from_pattern) {
                break;
            }
        }
        let matches_count = if use_default_count {
            node_matches_default_count_for(interpreter.xot(), node, n)
        } else if let Some(ref count_pattern) = count_pattern {
            node_matches_pattern(interpreter, n, count_pattern)
        } else {
            false
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
                } else if let Some(ref count_pattern) = count_pattern {
                    node_matches_pattern(interpreter, sibling, count_pattern)
                } else {
                    false
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
fn clone_number_pattern(
    interpreter: &Interpreter,
    pattern_index: i64,
) -> Option<Pattern<function::InlineFunctionId>> {
    if pattern_index < 0 {
        None
    } else {
        Some(
            interpreter
                .runnable()
                .program()
                .declarations
                .number_pattern(pattern_index as usize)
                .pattern
                .clone(),
        )
    }
}

fn node_matches_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    pattern: &Pattern<function::InlineFunctionId>,
) -> bool {
    let item = sequence::Item::from(node);
    interpreter.matches(pattern, &item)
}

/// Test if a candidate node matches the default count pattern for the reference node.
/// Default count matches nodes with the same node kind and expanded-QName.
fn node_matches_default_count_for(
    xot: &Xot,
    reference_node: xot::Node,
    candidate: xot::Node,
) -> bool {
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

/// Parse grouping-separator attribute: must be a single character.
fn parse_grouping_separator(s: Option<&str>) -> Option<char> {
    s.and_then(|s| {
        let mut chars = s.chars();
        let c = chars.next()?;
        if chars.next().is_none() {
            Some(c)
        } else {
            None
        }
    })
}

/// Parse grouping-size attribute: must be a positive integer.
fn parse_grouping_size(s: Option<&str>) -> Option<usize> {
    s.and_then(|s| s.parse::<usize>().ok()).filter(|&n| n > 0)
}

/// Parse start-at attribute for single/any level: returns the first integer value (default 1).
fn parse_start_at(s: Option<&str>) -> i64 {
    s.and_then(|s| {
        s.split_whitespace()
            .next()
            .and_then(|v| v.parse::<i64>().ok())
    })
    .unwrap_or(1)
}

/// Parse start-at attribute for multiple level: returns a Vec of integer values.
fn parse_start_at_values(s: Option<&str>) -> Vec<i64> {
    match s {
        Some(s) => s
            .split_whitespace()
            .filter_map(|v| v.parse::<i64>().ok())
            .collect(),
        None => Vec::new(),
    }
}

/// Apply start-at adjustments to a vector of numbers for level="multiple".
/// Each position gets its corresponding start-at value; if there are fewer
/// start-at values than numbers, the last start-at value is reused per spec.
fn apply_start_at_multiple(numbers: &mut [i64], start_at_values: &[i64]) {
    for (i, num) in numbers.iter_mut().enumerate() {
        let sa = if start_at_values.is_empty() {
            1
        } else if i < start_at_values.len() {
            start_at_values[i]
        } else {
            start_at_values[start_at_values.len() - 1]
        };
        *num = *num + sa - 1;
    }
}

fn format_xslt_number_value(
    number: i64,
    picture: &str,
    grouping_separator: Option<char>,
    grouping_size: Option<usize>,
) -> error::Result<String> {
    format_xslt_number_values(&[number], picture, grouping_separator, grouping_size)
}

/// Format a sequence of numbers according to the XSLT format picture.
/// For level="single"/"any" this is a single number; for level="multiple" it may be several.
fn format_xslt_number_values(
    numbers: &[i64],
    picture: &str,
    grouping_separator: Option<char>,
    grouping_size: Option<usize>,
) -> error::Result<String> {
    if picture.is_empty() {
        // Empty format picture: per XSLT spec, use the default format "1"
        return format_xslt_number_values(numbers, "1", grouping_separator, grouping_size);
    }

    if numbers.is_empty() {
        // Empty list (e.g. no matching nodes for level="single"/"any"):
        // per XSLT spec, the formatted numbers part is empty but the prefix
        // and suffix from the format picture are still produced.
        let chars: Vec<char> = picture.chars().collect();
        let first_alnum = chars.iter().position(|c| c.is_alphanumeric());
        let last_alnum = chars.iter().rposition(|c| c.is_alphanumeric());
        return match (first_alnum, last_alnum) {
            (Some(first), Some(last)) => {
                let prefix: String = chars[..first].iter().collect();
                let suffix: String = chars[last + 1..].iter().collect();
                Ok(format!("{}{}", prefix, suffix))
            }
            _ => Ok(String::new()),
        };
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
        // No format tokens at all — per XSLT spec, the picture is used as
        // both prefix and suffix, with default format token "1".
        let mut result = picture.to_string();
        for (i, &num) in numbers.iter().enumerate() {
            if i > 0 {
                result.push('.');
            }
            result.push_str(&format_number_token(num, "1", grouping_separator, grouping_size)?);
        }
        result.push_str(picture);
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
                if s.is_empty() {
                    ".".to_string()
                } else {
                    s
                }
            } else if tokens.len() >= 2 {
                let prev_end = tokens[tokens.len() - 2].1;
                let curr_start = tokens[tokens.len() - 1].0;
                let s: String = chars[prev_end..curr_start].iter().collect();
                if s.is_empty() {
                    ".".to_string()
                } else {
                    s
                }
            } else {
                ".".to_string()
            };
            result.push_str(&sep);
        }

        // Use token[i] if it exists, otherwise use the last token
        let token_idx = if i < tokens.len() {
            i
        } else {
            tokens.len() - 1
        };
        let (t_start, t_end) = tokens[token_idx];
        let token: String = chars[t_start..t_end].iter().collect();
        result.push_str(&format_number_token(num, &token, grouping_separator, grouping_size)?);
    }

    result.push_str(&suffix);
    Ok(result)
}

fn format_number_token(
    number: i64,
    token: &str,
    grouping_separator: Option<char>,
    grouping_size: Option<usize>,
) -> error::Result<String> {
    if token.is_empty() {
        return Ok(number.to_string());
    }

    // Handle decimal format pictures: "1", "0", "01", "001", etc.
    // Any token consisting entirely of ASCII digits 0 and 1 is a decimal picture.
    let chars: Vec<char> = token.chars().collect();
    if chars.iter().all(|c| *c == '0' || *c == '1') {
        let min_width = chars.len();
        let formatted = format!("{:0>width$}", number, width = min_width);
        return Ok(apply_grouping(&formatted, grouping_separator, grouping_size));
    }

    if chars.len() > 1 {
        return Err(error::Error::Unsupported(format!(
            "xsl:number value formatting token not supported yet: {token}"
        )));
    }

    match chars[0] {
        '1' => {
            let formatted = number.to_string();
            Ok(apply_grouping(&formatted, grouping_separator, grouping_size))
        }
        'a' => format_alphabetic_number(number, false),
        'A' => format_alphabetic_number(number, true),
        'i' => format_roman_number(number, false),
        'I' => format_roman_number(number, true),
        _ => Err(error::Error::Unsupported(format!(
            "xsl:number value formatting token not supported yet: {token}"
        ))),
    }
}

/// Apply digit grouping to a formatted number string.
/// E.g. "1000000" with separator=',' and size=3 becomes "1,000,000".
fn apply_grouping(
    formatted: &str,
    separator: Option<char>,
    size: Option<usize>,
) -> String {
    let (Some(sep), Some(sz)) = (separator, size) else {
        return formatted.to_string();
    };
    if sz == 0 {
        return formatted.to_string();
    }
    // Only group the digit portion (skip any leading minus sign)
    let (prefix, digits) = if formatted.starts_with('-') {
        ("-", &formatted[1..])
    } else {
        ("", formatted.as_ref())
    };
    if digits.len() <= sz {
        return formatted.to_string();
    }
    let mut result = String::with_capacity(formatted.len() + digits.len() / sz);
    result.push_str(prefix);
    let remainder = digits.len() % sz;
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && i >= remainder && (i - remainder) % sz == 0 {
            result.push(sep);
        }
        result.push(ch);
    }
    result
}

fn format_alphabetic_number(number: i64, uppercase: bool) -> error::Result<String> {
    if number <= 0 {
        // Spec: fall back to decimal for values outside the representable range
        return Ok(number.to_string());
    }

    let mut value = u64::try_from(number).map_err(|_| error::Error::XPTY0004(None))?;
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
    if number <= 0 || number > 3999 {
        // Spec: if the number is outside the range for the numbering scheme,
        // fall back to the default format token "1" (decimal).
        return Ok(number.to_string());
    }

    let mut value = number as u64;
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

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(xslt_number_value),
        wrap_xpath_fn!(xslt_number_count_single),
        wrap_xpath_fn!(xslt_number_count_any),
        wrap_xpath_fn!(xslt_number_count_single_pattern),
        wrap_xpath_fn!(xslt_number_count_any_pattern),
        wrap_xpath_fn!(xslt_number_count_multiple),
        wrap_xpath_fn!(xslt_number_count_multiple_pattern),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;
    use rust_decimal::Decimal;
    use std::rc::Rc;

    // --- parse_grouping_separator ---

    #[test]
    fn grouping_separator_none() {
        assert_eq!(parse_grouping_separator(None), None);
    }

    #[test]
    fn grouping_separator_single_char() {
        assert_eq!(parse_grouping_separator(Some(",")), Some(','));
        assert_eq!(parse_grouping_separator(Some(".")), Some('.'));
        assert_eq!(parse_grouping_separator(Some(" ")), Some(' '));
    }

    #[test]
    fn grouping_separator_multi_char_rejected() {
        assert_eq!(parse_grouping_separator(Some(",,")), None);
        assert_eq!(parse_grouping_separator(Some("ab")), None);
    }

    #[test]
    fn grouping_separator_empty_string() {
        assert_eq!(parse_grouping_separator(Some("")), None);
    }

    // --- parse_grouping_size ---

    #[test]
    fn grouping_size_none() {
        assert_eq!(parse_grouping_size(None), None);
    }

    #[test]
    fn grouping_size_valid() {
        assert_eq!(parse_grouping_size(Some("3")), Some(3));
        assert_eq!(parse_grouping_size(Some("1")), Some(1));
    }

    #[test]
    fn grouping_size_zero_rejected() {
        assert_eq!(parse_grouping_size(Some("0")), None);
    }

    #[test]
    fn grouping_size_non_numeric() {
        assert_eq!(parse_grouping_size(Some("abc")), None);
    }

    // --- parse_start_at ---

    #[test]
    fn start_at_default() {
        assert_eq!(parse_start_at(None), 1);
    }

    #[test]
    fn start_at_single_value() {
        assert_eq!(parse_start_at(Some("3")), 3);
        assert_eq!(parse_start_at(Some("0")), 0);
        assert_eq!(parse_start_at(Some("-2")), -2);
    }

    #[test]
    fn start_at_multiple_values_takes_first() {
        assert_eq!(parse_start_at(Some("3 5 7")), 3);
    }

    // --- parse_start_at_values ---

    #[test]
    fn start_at_values_none() {
        assert_eq!(parse_start_at_values(None), Vec::<i64>::new());
    }

    #[test]
    fn start_at_values_multiple() {
        assert_eq!(parse_start_at_values(Some("3 5 7")), vec![3, 5, 7]);
    }

    #[test]
    fn start_at_values_with_negatives() {
        assert_eq!(parse_start_at_values(Some("-0 1 -2 3")), vec![0, 1, -2, 3]);
    }

    // --- apply_start_at_multiple ---

    #[test]
    fn start_at_multiple_empty_sa() {
        let mut nums = vec![1, 2, 3];
        apply_start_at_multiple(&mut nums, &[]);
        // default sa=1: num + 1 - 1 = num
        assert_eq!(nums, vec![1, 2, 3]);
    }

    #[test]
    fn start_at_multiple_single_sa_reused() {
        // Per XSLT spec: last start-at value is reused for remaining positions
        let mut nums = vec![1, 2, 3];
        apply_start_at_multiple(&mut nums, &[3]);
        // Each: num + 3 - 1 = num + 2
        assert_eq!(nums, vec![3, 4, 5]);
    }

    #[test]
    fn start_at_multiple_per_position() {
        let mut nums = vec![1, 4, 5];
        apply_start_at_multiple(&mut nums, &[0, 1, 2]);
        // 1+(0-1)=0, 4+(1-1)=4, 5+(2-1)=6
        assert_eq!(nums, vec![0, 4, 6]);
    }

    #[test]
    fn start_at_multiple_fewer_sa_than_numbers() {
        let mut nums = vec![1, 2, 3, 4];
        apply_start_at_multiple(&mut nums, &[5, 10]);
        // 1+4=5, 2+9=11, 3+9=12, 4+9=13 (last sa=10 reused)
        assert_eq!(nums, vec![5, 11, 12, 13]);
    }

    // --- apply_grouping ---

    #[test]
    fn grouping_none() {
        assert_eq!(apply_grouping("1234", None, None), "1234");
    }

    #[test]
    fn grouping_thousands() {
        assert_eq!(apply_grouping("1000000", Some(','), Some(3)), "1,000,000");
    }

    #[test]
    fn grouping_small_number() {
        assert_eq!(apply_grouping("999", Some(','), Some(3)), "999");
    }

    #[test]
    fn grouping_exact_boundary() {
        assert_eq!(apply_grouping("1000", Some(','), Some(3)), "1,000");
    }

    #[test]
    fn grouping_size_zero() {
        assert_eq!(apply_grouping("1000", Some(','), Some(0)), "1000");
    }

    // --- format_number_token ---

    #[test]
    fn format_decimal_1() {
        assert_eq!(format_number_token(42, "1", None, None).unwrap(), "42");
    }

    #[test]
    fn format_decimal_0() {
        assert_eq!(format_number_token(7, "0", None, None).unwrap(), "7");
    }

    #[test]
    fn format_zero_padded_01() {
        assert_eq!(format_number_token(5, "01", None, None).unwrap(), "05");
        assert_eq!(format_number_token(42, "01", None, None).unwrap(), "42");
    }

    #[test]
    fn format_zero_padded_001() {
        assert_eq!(format_number_token(5, "001", None, None).unwrap(), "005");
        assert_eq!(format_number_token(42, "001", None, None).unwrap(), "042");
        assert_eq!(format_number_token(999, "001", None, None).unwrap(), "999");
        assert_eq!(format_number_token(1000, "001", None, None).unwrap(), "1000");
    }

    #[test]
    fn format_alpha_lower() {
        assert_eq!(format_number_token(1, "a", None, None).unwrap(), "a");
        assert_eq!(format_number_token(26, "a", None, None).unwrap(), "z");
        assert_eq!(format_number_token(27, "a", None, None).unwrap(), "aa");
    }

    #[test]
    fn format_alpha_upper() {
        assert_eq!(format_number_token(1, "A", None, None).unwrap(), "A");
        assert_eq!(format_number_token(26, "A", None, None).unwrap(), "Z");
        assert_eq!(format_number_token(27, "A", None, None).unwrap(), "AA");
    }

    #[test]
    fn format_roman_lower() {
        assert_eq!(format_number_token(1, "i", None, None).unwrap(), "i");
        assert_eq!(format_number_token(4, "i", None, None).unwrap(), "iv");
        assert_eq!(format_number_token(9, "i", None, None).unwrap(), "ix");
        assert_eq!(format_number_token(42, "i", None, None).unwrap(), "xlii");
        assert_eq!(
            format_number_token(3999, "i", None, None).unwrap(),
            "mmmcmxcix"
        );
    }

    #[test]
    fn format_roman_upper() {
        assert_eq!(format_number_token(1, "I", None, None).unwrap(), "I");
        assert_eq!(format_number_token(14, "I", None, None).unwrap(), "XIV");
    }

    #[test]
    fn format_roman_out_of_range() {
        // Spec: fall back to decimal for values outside roman range
        assert_eq!(format_number_token(0, "i", None, None).unwrap(), "0");
        assert_eq!(format_number_token(4000, "I", None, None).unwrap(), "4000");
    }

    #[test]
    fn format_alpha_zero_fallback() {
        // Spec: fall back to decimal for 0
        assert_eq!(format_number_token(0, "a", None, None).unwrap(), "0");
    }

    #[test]
    fn format_with_grouping() {
        assert_eq!(
            format_number_token(1000000, "1", Some(','), Some(3)).unwrap(),
            "1,000,000"
        );
    }

    // --- format_xslt_number_values ---

    #[test]
    fn format_single_number() {
        assert_eq!(
            format_xslt_number_values(&[42], "1", None, None).unwrap(),
            "42"
        );
    }

    #[test]
    fn format_multiple_numbers_default_separator() {
        assert_eq!(
            format_xslt_number_values(&[1, 2, 3], "1", None, None).unwrap(),
            "1.2.3"
        );
    }

    #[test]
    fn format_multiple_numbers_custom_separator() {
        // format="1,1" → token1="1", separator=",", token2="1"
        assert_eq!(
            format_xslt_number_values(&[10, 11, 12], "1,1", None, None).unwrap(),
            "10,11,12"
        );
    }

    #[test]
    fn format_with_prefix_suffix() {
        // format="(1)" → prefix="(", token="1", suffix=")"
        assert_eq!(
            format_xslt_number_values(&[5], "(1)", None, None).unwrap(),
            "(5)"
        );
    }

    #[test]
    fn format_prefix_suffix_multiple() {
        // format="(1)" with multiple numbers → (5.6.7.8)
        assert_eq!(
            format_xslt_number_values(&[5, 6, 7, 8], "(1)", None, None).unwrap(),
            "(5.6.7.8)"
        );
    }

    #[test]
    fn format_prefix_only_with_separator() {
        // format="1;1)" → no prefix, separator=";", suffix=")"
        assert_eq!(
            format_xslt_number_values(&[5, 6, 7, 8], "1;1)", None, None).unwrap(),
            "5;6;7;8)"
        );
    }

    #[test]
    fn format_no_tokens_picture_is_prefix_and_suffix() {
        // format="*" → no format tokens → "*" is both prefix and suffix
        assert_eq!(
            format_xslt_number_values(&[1], "*", None, None).unwrap(),
            "*1*"
        );
    }

    #[test]
    fn format_empty_picture_defaults_to_1() {
        assert_eq!(
            format_xslt_number_values(&[42], "", None, None).unwrap(),
            "42"
        );
    }

    #[test]
    fn format_empty_numbers() {
        // Empty number list with format "(1) " → prefix "(" + suffix ") "
        assert_eq!(
            format_xslt_number_values(&[], "(1) ", None, None).unwrap(),
            "() "
        );
    }

    #[test]
    fn format_empty_numbers_no_tokens() {
        // Empty number list with format "*" → no format tokens, returns empty
        assert_eq!(
            format_xslt_number_values(&[], "*", None, None).unwrap(),
            ""
        );
    }

    #[test]
    fn format_multi_level_alpha() {
        // format="a.a.a" with three levels
        assert_eq!(
            format_xslt_number_values(&[3, 3, 4], "a.a.a", None, None).unwrap(),
            "c.c.d"
        );
    }

    // --- atomic_to_number_value ---

    #[test]
    fn atomic_integer_positive() {
        let a = atomic::Atomic::from(IBig::from(42));
        assert_eq!(atomic_to_number_value(&a).unwrap(), 42);
    }

    #[test]
    fn atomic_integer_zero() {
        let a = atomic::Atomic::from(IBig::from(0));
        assert_eq!(atomic_to_number_value(&a).unwrap(), 0);
    }

    #[test]
    fn atomic_integer_negative_error() {
        let a = atomic::Atomic::from(IBig::from(-5));
        assert!(matches!(
            atomic_to_number_value(&a),
            Err(error::Error::XTDE0980)
        ));
    }

    #[test]
    fn atomic_double_rounds() {
        let a = atomic::Atomic::Double(OrderedFloat(6.51));
        assert_eq!(atomic_to_number_value(&a).unwrap(), 7);
    }

    #[test]
    fn atomic_double_nan_error() {
        let a = atomic::Atomic::Double(OrderedFloat(f64::NAN));
        assert!(matches!(
            atomic_to_number_value(&a),
            Err(error::Error::XTDE0980)
        ));
    }

    #[test]
    fn atomic_double_infinity_error() {
        let a = atomic::Atomic::Double(OrderedFloat(f64::INFINITY));
        assert!(matches!(
            atomic_to_number_value(&a),
            Err(error::Error::XTDE0980)
        ));
    }

    #[test]
    fn atomic_double_negative_error() {
        let a = atomic::Atomic::Double(OrderedFloat(-1.0));
        assert!(matches!(
            atomic_to_number_value(&a),
            Err(error::Error::XTDE0980)
        ));
    }

    #[test]
    fn atomic_float_rounds() {
        let a = atomic::Atomic::Float(OrderedFloat(2.7f32));
        assert_eq!(atomic_to_number_value(&a).unwrap(), 3);
    }

    #[test]
    fn atomic_decimal_half_rounds_away_from_zero() {
        // 6.5 should round to 7, not 6 (banker's rounding)
        let d = Decimal::new(65, 1); // 6.5
        let a = atomic::Atomic::Decimal(d.into());
        assert_eq!(atomic_to_number_value(&a).unwrap(), 7);
    }

    #[test]
    fn atomic_decimal_rounds_down() {
        let d = Decimal::new(64, 1); // 6.4
        let a = atomic::Atomic::Decimal(d.into());
        assert_eq!(atomic_to_number_value(&a).unwrap(), 6);
    }

    #[test]
    fn atomic_string_non_numeric_error() {
        let a = atomic::Atomic::Untyped("fizz".into());
        assert!(matches!(
            atomic_to_number_value(&a),
            Err(error::Error::XTDE0980)
        ));
    }

    // --- format_alphabetic_number ---

    #[test]
    fn alphabetic_boundary_values() {
        assert_eq!(format_alphabetic_number(1, false).unwrap(), "a");
        assert_eq!(format_alphabetic_number(26, false).unwrap(), "z");
        assert_eq!(format_alphabetic_number(27, false).unwrap(), "aa");
        assert_eq!(format_alphabetic_number(52, false).unwrap(), "az");
        assert_eq!(format_alphabetic_number(53, false).unwrap(), "ba");
        assert_eq!(format_alphabetic_number(702, false).unwrap(), "zz");
        assert_eq!(format_alphabetic_number(703, false).unwrap(), "aaa");
    }

    // --- format_roman_number ---

    #[test]
    fn roman_standard_values() {
        assert_eq!(format_roman_number(1, true).unwrap(), "I");
        assert_eq!(format_roman_number(4, true).unwrap(), "IV");
        assert_eq!(format_roman_number(9, true).unwrap(), "IX");
        assert_eq!(format_roman_number(40, true).unwrap(), "XL");
        assert_eq!(format_roman_number(90, true).unwrap(), "XC");
        assert_eq!(format_roman_number(400, true).unwrap(), "CD");
        assert_eq!(format_roman_number(900, true).unwrap(), "CM");
        assert_eq!(format_roman_number(1994, true).unwrap(), "MCMXCIV");
        assert_eq!(format_roman_number(3999, true).unwrap(), "MMMCMXCIX");
    }

    #[test]
    fn roman_lowercase() {
        assert_eq!(format_roman_number(7, false).unwrap(), "vii");
        assert_eq!(format_roman_number(99, false).unwrap(), "xcix");
    }
}
