// xsl:number runtime implementation: counting and formatting.

use ahash::HashMapExt;
use ibig::IBig;
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
    let sa = parse_start_at(start_at);
    let atomic = sequence::one(value.atomized(interpreter.xot()))??;
    // Per XSLT spec, the value is rounded to the nearest integer
    let number = match &atomic {
        atomic::Atomic::Float(f) => f.round() as i64,
        atomic::Atomic::Double(d) => d.round() as i64,
        atomic::Atomic::Decimal(d) => {
            let rounded = d.round();
            i64::try_from(rounded).map_err(|_| error::Error::XPTY0004(None))?
        }
        atomic::Atomic::Integer(_, i) => {
            i64::try_from(i.as_ref()).map_err(|_| error::Error::XPTY0004(None))?
        }
        _ => atomic
            .cast_to_integer_value::<i64>()
            .map_err(|_| error::Error::XPTY0004(None))?,
    };
    if number < 0 {
        return Err(error::Error::Unsupported(
            "xsl:number value must be non-negative".to_string(),
        ));
    }

    // Apply start-at adjustment: displayed = number + start_at - 1
    let adjusted = number + sa - 1;
    format_xslt_number_value(adjusted, format.unwrap_or("1"), gs, gsz)
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
/// Each position gets its corresponding start-at value (default 1).
fn apply_start_at_multiple(numbers: &mut [i64], start_at_values: &[i64]) {
    for (i, num) in numbers.iter_mut().enumerate() {
        let sa = start_at_values.get(i).copied().unwrap_or(1);
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
        return Err(error::Error::FODF1310);
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
        // No format tokens at all — use default "1"
        let mut result = picture.to_string();
        for (i, &num) in numbers.iter().enumerate() {
            if i > 0 {
                result.push('.');
            }
            result.push_str(&format_number_token(num, "1", grouping_separator, grouping_size)?);
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

    // Handle zero-padded decimal pictures like "01", "001"
    let chars: Vec<char> = token.chars().collect();
    if chars.iter().all(|c| *c == '0' || *c == '1') && chars.last() == Some(&'1') && chars.len() > 1
    {
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
