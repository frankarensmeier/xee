use std::collections::HashSet;
use std::fmt::Write;

use xee_schema_type::Xs;
use xee_xpath_macros::xpath_fn;
use xot::Xot;

use crate::{atomic, context, error, function, interpreter::Interpreter, sequence, wrap_xpath_fn};

use super::StaticFunctionDescription;

const FN_NS: &str = "http://www.w3.org/2005/xpath-functions";

#[xpath_fn("fn:parse-json($json_text as xs:string?) as item()?")]
fn parse_json1(json_text: Option<&str>) -> error::Result<Option<sequence::Item>> {
    if let Some(json_text) = json_text {
        let value = json::parse(json_text).map_err(|_| error::Error::FOJS0001)?;
        // the spec seems to imply escape should be true by default, but then
        // various tests fail (and escape false by default seems more
        // reasonable) See https://github.com/w3c/qt3tests/issues/65
        Ok(parse_json_value(&value, false)?)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:parse-json($json_text as xs:string?, $options as map(*)) as item()?")]
fn parse_json2(
    context: &context::DynamicContext,
    interpreter: &mut Interpreter,
    json_text: Option<&str>,
    options: function::Map,
) -> error::Result<Option<sequence::Item>> {
    let parameters =
        ParseJsonParameters::from_map(&options, context.static_context(), interpreter.xot())?;

    if let Some(json_text) = json_text {
        let value = json::parse(json_text).map_err(|_| error::Error::FOJS0001)?;
        Ok(parse_json_value(&value, parameters.escape)?)
    } else {
        Ok(None)
    }
}

enum Duplicates {
    Reject,
    UseFirst,
    UseLast,
}

struct ParseJsonParameters {
    // liberal is entirely ignored. we don't have a more liberal JSON parser
    liberal: bool,
    // We cannot actually handle duplicates, as the Rust json crate
    // does not report duplicate information and effectively implements
    // `use-last` semantics (most common according to the JSON RFC)
    duplicates: Duplicates,
    // I don't understand why escape=true even exists, as it imports JSON
    // escaping rules into XML land where they have no meaning? But it's the
    // default! we implement it by re-escaping...
    escape: bool,
    // TODO: fallback
}

impl ParseJsonParameters {
    fn from_map(
        map: &function::Map,
        static_context: &context::StaticContext,
        xot: &Xot,
    ) -> error::Result<Self> {
        let c = sequence::OptionParameterConverter::new(map, static_context, xot);

        let liberal = c
            .option_with_default("liberal", Xs::Boolean, false)
            .map_err(|_| error::Error::FOJS0005)?;
        let duplicates = c
            .option_with_default("duplicates", Xs::String, "use-first".to_string())
            .map_err(|_| error::Error::FOJS0005)?;
        let duplicates = match duplicates.as_str() {
            "reject" => Duplicates::Reject,
            "use-first" => Duplicates::UseFirst,
            "use-last" => Duplicates::UseLast,
            _ => return Err(error::Error::FOJS0005),
        };
        // the escape is true by default, weird but following the spec.
        // weirdly enough the default is `false` (contrary to the spec) in
        // case the map isn't passed at all.
        // See https://github.com/w3c/qt3tests/issues/65
        let escape = c
            .option_with_default("escape", Xs::Boolean, true)
            .map_err(|_| error::Error::FOJS0005)?;

        Ok(Self {
            liberal,
            duplicates,
            escape,
        })
    }
}

fn parse_json_value(
    value: &json::JsonValue,
    escape: bool,
) -> error::Result<Option<sequence::Item>> {
    match value {
        json::JsonValue::Null => Ok(None),
        json::JsonValue::Short(s) => Ok(Some(parse_json_string(s.to_string(), escape).into())),
        json::JsonValue::String(s) => Ok(Some(parse_json_string(s.to_string(), escape).into())),
        json::JsonValue::Number(n) => {
            let f: f64 = (*n).into();
            let atomic: atomic::Atomic = f.into();
            Ok(Some(atomic.into()))
        }
        json::JsonValue::Boolean(b) => {
            let atomic = atomic::Atomic::Boolean(*b);
            Ok(Some(atomic.into()))
        }
        json::JsonValue::Array(a) => {
            let mut entries = Vec::with_capacity(a.len());
            for value in a.iter() {
                let value = parse_json_value(value, escape)?;
                let sequence: sequence::Sequence = value.into();
                entries.push(sequence);
            }
            let array = function::Array::new(entries);
            let function = function::Function::Array(array);
            Ok(Some(function.into()))
        }
        json::JsonValue::Object(o) => {
            let mut entries = Vec::with_capacity(o.len());

            for (key, value) in o.iter() {
                let key = parse_json_string(key.to_string(), escape);
                let value = parse_json_value(value, escape)?;
                let sequence: sequence::Sequence = value.into();
                entries.push((key.clone(), sequence));
            }
            let map = function::Map::new(entries)?;
            let function = function::Function::Map(map);
            Ok(Some(function.into()))
        }
    }
}

fn parse_json_string(s: String, escape: bool) -> atomic::Atomic {
    let s = s.to_string();
    let s = if escape {
        v_jsonescape::escape(&s).to_string()
    } else {
        s
    };
    let atomic: atomic::Atomic = s.into();
    atomic
}

// --- xml-to-json ---

/// Cached Xot name IDs for the JSON XML representation elements and attributes
struct JsonXmlNames {
    fn_ns: xot::NamespaceId,
    null: xot::NameId,
    boolean: xot::NameId,
    number: xot::NameId,
    string: xot::NameId,
    array: xot::NameId,
    map: xot::NameId,
    key: xot::NameId,
    escaped: xot::NameId,
    escaped_key: xot::NameId,
}

impl JsonXmlNames {
    fn new(xot: &mut Xot) -> Self {
        let fn_ns = xot.add_namespace(FN_NS);
        Self {
            fn_ns,
            null: xot.add_name_ns("null", fn_ns),
            boolean: xot.add_name_ns("boolean", fn_ns),
            number: xot.add_name_ns("number", fn_ns),
            string: xot.add_name_ns("string", fn_ns),
            array: xot.add_name_ns("array", fn_ns),
            map: xot.add_name_ns("map", fn_ns),
            key: xot.add_name("key"),
            escaped: xot.add_name("escaped"),
            escaped_key: xot.add_name("escaped-key"),
        }
    }
}

#[xpath_fn("fn:xml-to-json($input as node()?) as xs:string?")]
fn xml_to_json1(
    interpreter: &mut Interpreter,
    input: Option<xot::Node>,
) -> error::Result<Option<String>> {
    let Some(input) = input else {
        return Ok(None);
    };
    let names = JsonXmlNames::new(interpreter.xot_mut());
    let xot = interpreter.xot();
    let element = find_json_element(xot, input)?;
    let mut output = String::new();
    xml_node_to_json(xot, &names, element, false, &mut output)?;
    Ok(Some(output))
}

#[xpath_fn("fn:xml-to-json($input as node()?, $options as map(*)) as xs:string?")]
fn xml_to_json2(
    context: &context::DynamicContext,
    interpreter: &mut Interpreter,
    input: Option<xot::Node>,
    options: function::Map,
) -> error::Result<Option<String>> {
    let Some(input) = input else {
        return Ok(None);
    };
    let params = XmlToJsonParameters::from_map(
        &options,
        context.static_context(),
        interpreter.xot(),
    )?;
    let names = JsonXmlNames::new(interpreter.xot_mut());
    let xot = interpreter.xot();
    let element = find_json_element(xot, input)?;
    let mut output = String::new();
    xml_node_to_json(xot, &names, element, params.indent, &mut output)?;
    Ok(Some(output))
}

struct XmlToJsonParameters {
    indent: bool,
}

impl XmlToJsonParameters {
    fn from_map(
        map: &function::Map,
        static_context: &context::StaticContext,
        xot: &Xot,
    ) -> error::Result<Self> {
        let c = sequence::OptionParameterConverter::new(map, static_context, xot);
        let indent = c
            .option_with_default("indent", Xs::Boolean, false)
            .map_err(|e| match e {
                error::Error::XPTY0004(_) | error::Error::FORG0001 => e,
                _ => error::Error::FOJS0005,
            })?;
        Ok(Self { indent })
    }
}

/// Navigate from a document/element node to the JSON root element
fn find_json_element(xot: &Xot, node: xot::Node) -> error::Result<xot::Node> {
    match xot.value(node) {
        xot::Value::Document => {
            // find the document element; there must be exactly one
            let mut found = None;
            for child in xot.children(node) {
                if xot.is_element(child) {
                    if found.is_some() {
                        return Err(error::Error::FOJS0006);
                    }
                    found = Some(child);
                }
            }
            found.ok_or(error::Error::FOJS0006)
        }
        xot::Value::Element(_) => Ok(node),
        _ => Err(error::Error::FOJS0006),
    }
}

/// Convert an XML element in the JSON XML representation to a JSON string
fn xml_node_to_json(
    xot: &Xot,
    names: &JsonXmlNames,
    node: xot::Node,
    indent: bool,
    output: &mut String,
) -> error::Result<()> {
    xml_node_to_json_inner(xot, names, node, indent, 0, output)
}

fn xml_node_to_json_inner(
    xot: &Xot,
    names: &JsonXmlNames,
    node: xot::Node,
    indent: bool,
    depth: usize,
    output: &mut String,
) -> error::Result<()> {
    let element = xot.element(node).ok_or(error::Error::FOJS0006)?;
    let name = element.name();

    // Validate: no attributes in the fn namespace are allowed on any element
    validate_no_fn_ns_attrs(xot, names, node)?;

    if name == names.null {
        // null must have no content (no text, no child elements)
        validate_no_content(xot, node)?;
        validate_attrs_only(xot, node, &[])?;
        output.push_str("null");
    } else if name == names.boolean {
        validate_no_child_elements(xot, node)?;
        validate_attrs_only(xot, node, &[])?;
        let text = element_text_content(xot, node);
        let text = text.trim();
        // xs:boolean accepts true/false/1/0
        match text {
            "true" | "1" => output.push_str("true"),
            "false" | "0" => output.push_str("false"),
            _ => return Err(error::Error::FOJS0006),
        }
    } else if name == names.number {
        validate_no_child_elements(xot, node)?;
        validate_attrs_only(xot, node, &[])?;
        let text = element_text_content(xot, node);
        let text = text.trim();
        // parse as xs:double, then format as JSON number
        let d: f64 = text.parse().map_err(|_| error::Error::FOJS0006)?;
        if d.is_nan() || d.is_infinite() {
            return Err(error::Error::FOJS0006);
        }
        write_json_number(d, output);
    } else if name == names.string {
        validate_no_child_elements(xot, node)?;
        validate_attrs_only(xot, node, &[names.escaped])?;
        let text = element_text_content(xot, node);
        let escaped = parse_boolean_attr(xot, names, node, names.escaped)?;
        output.push('"');
        if escaped {
            // escaped mode: validate and copy escape sequences
            write_json_string_escaped(&text, output)?;
        } else {
            // unescaped mode: escape special chars
            write_json_string_unescaped(&text, output);
        }
        output.push('"');
    } else if name == names.array {
        validate_attrs_only(xot, node, &[])?;
        output.push('[');
        let children = json_child_elements(xot, node)?;
        for (i, child) in children.iter().enumerate() {
            if i > 0 {
                output.push(',');
            }
            if indent {
                output.push('\n');
                write_indent(depth + 1, output);
            }
            xml_node_to_json_inner(xot, names, *child, indent, depth + 1, output)?;
        }
        if indent && !children.is_empty() {
            output.push('\n');
            write_indent(depth, output);
        }
        output.push(']');
    } else if name == names.map {
        validate_attrs_only(xot, node, &[])?;
        output.push('{');
        let children = json_child_elements(xot, node)?;
        // check for duplicate normalized keys
        let mut seen_keys = HashSet::new();
        let mut entries = Vec::with_capacity(children.len());
        for child in &children {
            let key_raw = xot
                .attributes(*child)
                .get(names.key)
                .ok_or(error::Error::FOJS0006)?;
            let escaped_key = parse_boolean_attr(xot, names, *child, names.escaped_key)?;
            let normalized_key = if escaped_key {
                unescape_json_string(key_raw)?
            } else {
                key_raw.to_string()
            };
            if !seen_keys.insert(normalized_key) {
                return Err(error::Error::FOJS0006);
            }
            entries.push((*child, key_raw.to_string(), escaped_key));
        }
        for (i, (child, key_raw, escaped_key)) in entries.iter().enumerate() {
            if i > 0 {
                output.push(',');
            }
            if indent {
                output.push('\n');
                write_indent(depth + 1, output);
            }
            // write the key
            output.push('"');
            if *escaped_key {
                write_json_string_escaped(&key_raw, output)?;
            } else {
                write_json_string_unescaped(&key_raw, output);
            }
            output.push('"');
            output.push(':');
            // write the value
            xml_node_to_json_inner(xot, names, *child, indent, depth + 1, output)?;
        }
        if indent && !children.is_empty() {
            output.push('\n');
            write_indent(depth, output);
        }
        output.push('}');
    } else {
        // unknown element in the fn namespace or wrong namespace
        return Err(error::Error::FOJS0006);
    }
    Ok(())
}

/// Validate that an element has no content (no text, no child elements)
fn validate_no_content(xot: &Xot, node: xot::Node) -> error::Result<()> {
    for child in xot.children(node) {
        match xot.value(child) {
            xot::Value::Element(_) => return Err(error::Error::FOJS0006),
            xot::Value::Text(text) => {
                if !text.get().trim().is_empty() {
                    return Err(error::Error::FOJS0006);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate that an element has no child elements
fn validate_no_child_elements(xot: &Xot, node: xot::Node) -> error::Result<()> {
    for child in xot.children(node) {
        if xot.is_element(child) {
            return Err(error::Error::FOJS0006);
        }
    }
    Ok(())
}

/// Validate that an element only has the specified allowed no-namespace
/// attributes plus `key` and `escaped-key` (which are allowed on all elements).
/// Attributes in other namespaces (non-fn) are silently ignored per spec.
fn validate_attrs_only(
    xot: &Xot,
    node: xot::Node,
    allowed: &[xot::NameId],
) -> error::Result<()> {
    for attr_name in xot.attributes(node).keys() {
        let (local, ns) = xot.name_ns_str(attr_name);
        if ns.is_empty() {
            // no-namespace: only key, escaped-key, and explicitly allowed
            if local == "key" || local == "escaped-key" {
                continue;
            }
            if allowed.contains(&attr_name) {
                continue;
            }
            return Err(error::Error::FOJS0006);
        }
        // namespaced attributes: fn namespace rejected by validate_no_fn_ns_attrs,
        // other namespaces silently ignored
    }
    Ok(())
}

/// Validate that no attributes in the fn namespace appear on an element
fn validate_no_fn_ns_attrs(xot: &Xot, _names: &JsonXmlNames, node: xot::Node) -> error::Result<()> {
    for attr_name in xot.attributes(node).keys() {
        let (_, ns) = xot.name_ns_str(attr_name);
        if ns == FN_NS {
            return Err(error::Error::FOJS0006);
        }
    }
    Ok(())
}

/// Collect text content from an element, ignoring comments and PIs
fn element_text_content(xot: &Xot, node: xot::Node) -> String {
    let mut result = String::new();
    for child in xot.children(node) {
        if let xot::Value::Text(text) = xot.value(child) {
            result.push_str(text.get());
        }
        // comments and PIs are ignored per spec
    }
    result
}

/// Get child elements, skipping whitespace-only text nodes, comments, and PIs.
/// Returns FOJS0006 if non-whitespace text content is found.
fn json_child_elements(xot: &Xot, node: xot::Node) -> error::Result<Vec<xot::Node>> {
    let mut children = Vec::new();
    for child in xot.children(node) {
        match xot.value(child) {
            xot::Value::Element(_) => children.push(child),
            xot::Value::Text(text) => {
                if !text.get().trim().is_empty() {
                    return Err(error::Error::FOJS0006);
                }
            }
            // comments and PIs are ignored
            _ => {}
        }
    }
    Ok(children)
}

/// Parse an xs:boolean attribute value with whitespace tolerance
fn parse_boolean_attr(
    xot: &Xot,
    _names: &JsonXmlNames,
    node: xot::Node,
    attr_name: xot::NameId,
) -> error::Result<bool> {
    let Some(val) = xot.attributes(node).get(attr_name) else {
        return Ok(false); // default is false
    };
    let val = val.trim();
    match val {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(error::Error::FOJS0006),
    }
}

/// Write a JSON number from an f64, using xs:double canonical formatting.
fn write_json_number(d: f64, output: &mut String) {
    output.push_str(&crate::atomic::Atomic::canonical_float(d));
}

/// Write a JSON string in unescaped mode (content is literal, we must escape)
fn write_json_string_unescaped(s: &str, output: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '/' => output.push_str("\\/"),
            '\u{0008}' => output.push_str("\\b"),
            '\u{000C}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c < '\u{0020}' || ('\u{007F}'..='\u{009F}').contains(&c) => {
                // control characters → \uHHHH
                write!(output, "\\u{:04X}", c as u32).unwrap();
            }
            c => output.push(c),
        }
    }
}

/// Write a JSON string in escaped mode (backslash sequences are pre-escaped,
/// copy them through but still escape chars that require it)
fn write_json_string_escaped(s: &str, output: &mut String) -> error::Result<()> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            // validate and copy the escape sequence
            if i + 1 >= chars.len() {
                return Err(error::Error::FOJS0007);
            }
            let next = chars[i + 1];
            match next {
                '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                    output.push('\\');
                    output.push(next);
                    i += 2;
                }
                'u' => {
                    // \uHHHH — need 4 hex digits
                    if i + 5 >= chars.len() {
                        return Err(error::Error::FOJS0007);
                    }
                    let hex: String = chars[i + 2..i + 6].iter().collect();
                    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(error::Error::FOJS0007);
                    }
                    output.push_str("\\u");
                    output.push_str(&hex);
                    i += 6;
                }
                _ => return Err(error::Error::FOJS0007),
            }
        } else {
            // non-backslash chars: still need to escape control chars
            // and quotes that aren't part of escape sequences
            match ch {
                '"' => output.push_str("\\\""),
                '/' => output.push_str("\\/"),
                '\u{0008}' => output.push_str("\\b"),
                '\u{000C}' => output.push_str("\\f"),
                '\n' => output.push_str("\\n"),
                '\r' => output.push_str("\\r"),
                '\t' => output.push_str("\\t"),
                c if c < '\u{0020}' || ('\u{007F}'..='\u{009F}').contains(&c) => {
                    write!(output, "\\u{:04X}", c as u32).unwrap();
                }
                c => output.push(c),
            }
            i += 1;
        }
    }
    Ok(())
}

/// Unescape a JSON escape sequence to get the normalized key value
fn unescape_json_string(s: &str) -> error::Result<String> {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            if i + 1 >= chars.len() {
                return Err(error::Error::FOJS0007);
            }
            match chars[i + 1] {
                '"' => { result.push('"'); i += 2; }
                '\\' => { result.push('\\'); i += 2; }
                '/' => { result.push('/'); i += 2; }
                'b' => { result.push('\u{0008}'); i += 2; }
                'f' => { result.push('\u{000C}'); i += 2; }
                'n' => { result.push('\n'); i += 2; }
                'r' => { result.push('\r'); i += 2; }
                't' => { result.push('\t'); i += 2; }
                'u' => {
                    if i + 5 >= chars.len() {
                        return Err(error::Error::FOJS0007);
                    }
                    let hex: String = chars[i + 2..i + 6].iter().collect();
                    let cp = u32::from_str_radix(&hex, 16)
                        .map_err(|_| error::Error::FOJS0007)?;
                    // handle surrogate pairs
                    if (0xD800..=0xDBFF).contains(&cp) {
                        // high surrogate — expect \uDCxx low surrogate
                        if i + 11 >= chars.len() || chars[i + 6] != '\\' || chars[i + 7] != 'u' {
                            return Err(error::Error::FOJS0007);
                        }
                        let hex2: String = chars[i + 8..i + 12].iter().collect();
                        let cp2 = u32::from_str_radix(&hex2, 16)
                            .map_err(|_| error::Error::FOJS0007)?;
                        if !(0xDC00..=0xDFFF).contains(&cp2) {
                            return Err(error::Error::FOJS0007);
                        }
                        let combined = 0x10000 + (cp - 0xD800) * 0x400 + (cp2 - 0xDC00);
                        let c = char::from_u32(combined).ok_or(error::Error::FOJS0007)?;
                        result.push(c);
                        i += 12;
                    } else if let Some(c) = char::from_u32(cp) {
                        result.push(c);
                        i += 6;
                    } else {
                        return Err(error::Error::FOJS0007);
                    }
                }
                _ => return Err(error::Error::FOJS0007),
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn write_indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

// --- json-to-xml ---

/// Cached Xot name IDs for building the JSON XML output tree
struct JsonXmlBuilder {
    fn_ns: xot::NamespaceId,
    j_prefix: xot::PrefixId,
    null: xot::NameId,
    boolean: xot::NameId,
    number: xot::NameId,
    string: xot::NameId,
    array: xot::NameId,
    map: xot::NameId,
    key: xot::NameId,
    escaped: xot::NameId,
    escaped_key: xot::NameId,
}

impl JsonXmlBuilder {
    fn new(xot: &mut Xot) -> Self {
        let fn_ns = xot.add_namespace(FN_NS);
        Self {
            fn_ns,
            j_prefix: xot.add_prefix("j"),
            null: xot.add_name_ns("null", fn_ns),
            boolean: xot.add_name_ns("boolean", fn_ns),
            number: xot.add_name_ns("number", fn_ns),
            string: xot.add_name_ns("string", fn_ns),
            array: xot.add_name_ns("array", fn_ns),
            map: xot.add_name_ns("map", fn_ns),
            key: xot.add_name("key"),
            escaped: xot.add_name("escaped"),
            escaped_key: xot.add_name("escaped-key"),
        }
    }

    /// Build a JSON XML element, attaching the fn namespace declaration
    fn new_element(&self, xot: &mut Xot, name: xot::NameId) -> xot::Node {
        let el = xot.new_element(name);
        let mut ns = xot.namespaces_mut(el);
        ns.insert(self.j_prefix, self.fn_ns);
        el
    }

    /// Build a JSON XML element with a key attribute (for map children)
    fn new_keyed_element(
        &self,
        xot: &mut Xot,
        name: xot::NameId,
        key: &str,
        escape: bool,
    ) -> xot::Node {
        let el = self.new_element(xot, name);
        let mut attrs = xot.attributes_mut(el);
        if escape {
            attrs.insert(self.key, v_jsonescape::escape(key).to_string());
            attrs.insert(self.escaped_key, "true".to_string());
        } else {
            attrs.insert(self.key, key.to_string());
        }
        el
    }
}

struct JsonToXmlParameters {
    escape: bool,
    duplicates: Duplicates,
}

impl JsonToXmlParameters {
    fn from_map(
        map: &function::Map,
        static_context: &context::StaticContext,
        xot: &Xot,
    ) -> error::Result<Self> {
        let c = sequence::OptionParameterConverter::new(map, static_context, xot);
        let escape = c
            .option_with_default("escape", Xs::Boolean, false)
            .map_err(|_| error::Error::FOJS0005)?;
        let duplicates = c
            .option_with_default("duplicates", Xs::String, "retain".to_string())
            .map_err(|_| error::Error::FOJS0005)?;
        let duplicates = match duplicates.as_str() {
            "reject" => Duplicates::Reject,
            "use-first" => Duplicates::UseFirst,
            "use-last" => Duplicates::UseLast,
            "retain" => Duplicates::UseLast, // retain ≈ keep all, but we can't distinguish
            _ => return Err(error::Error::FOJS0005),
        };
        // liberal is ignored, validate → FOJS0004
        let validate: bool = c
            .option_with_default("validate", Xs::Boolean, false)
            .map_err(|_| error::Error::FOJS0005)?;
        if validate {
            return Err(error::Error::FOJS0004);
        }
        Ok(Self { escape, duplicates })
    }
}

#[xpath_fn("fn:json-to-xml($json_text as xs:string?) as document-node()?")]
fn json_to_xml1(
    interpreter: &mut Interpreter,
    json_text: Option<&str>,
) -> error::Result<Option<sequence::Item>> {
    let Some(json_text) = json_text else {
        return Ok(None);
    };
    let value = json::parse(json_text).map_err(|_| error::Error::FOJS0001)?;
    let builder = JsonXmlBuilder::new(interpreter.xot_mut());
    let xot = interpreter.state.xot_mut();
    let doc = xot.new_document();
    let root = json_value_to_xml(xot, &builder, &value, None, false)?;
    xot.append(doc, root).unwrap();
    Ok(Some(doc.into()))
}

#[xpath_fn("fn:json-to-xml($json_text as xs:string?, $options as map(*)) as document-node()?")]
fn json_to_xml2(
    context: &context::DynamicContext,
    interpreter: &mut Interpreter,
    json_text: Option<&str>,
    options: function::Map,
) -> error::Result<Option<sequence::Item>> {
    let Some(json_text) = json_text else {
        return Ok(None);
    };
    let params = JsonToXmlParameters::from_map(
        &options,
        context.static_context(),
        interpreter.xot(),
    )?;
    let value = json::parse(json_text).map_err(|_| error::Error::FOJS0001)?;
    let builder = JsonXmlBuilder::new(interpreter.xot_mut());
    let xot = interpreter.state.xot_mut();
    let doc = xot.new_document();
    let root = json_value_to_xml(xot, &builder, &value, None, params.escape)?;
    xot.append(doc, root).unwrap();
    Ok(Some(doc.into()))
}

/// Recursively build an XML element from a JSON value
fn json_value_to_xml(
    xot: &mut Xot,
    builder: &JsonXmlBuilder,
    value: &json::JsonValue,
    key: Option<&str>,
    escape: bool,
) -> error::Result<xot::Node> {
    let node = match value {
        json::JsonValue::Null => {
            make_json_element(xot, builder, builder.null, key, escape)
        }
        json::JsonValue::Boolean(b) => {
            let el = make_json_element(xot, builder, builder.boolean, key, escape);
            let text = xot.new_text(if *b { "true" } else { "false" });
            xot.append(el, text).unwrap();
            el
        }
        json::JsonValue::Number(n) => {
            let el = make_json_element(xot, builder, builder.number, key, escape);
            let f: f64 = (*n).into();
            let mut s = String::new();
            write_json_number(f, &mut s);
            let text = xot.new_text(&s);
            xot.append(el, text).unwrap();
            el
        }
        json::JsonValue::Short(s) => {
            let el = make_json_element(xot, builder, builder.string, key, escape);
            let s_str = s.as_str();
            if escape {
                let escaped_s = v_jsonescape::escape(s_str).to_string();
                let text = xot.new_text(&escaped_s);
                xot.append(el, text).unwrap();
                let mut attrs = xot.attributes_mut(el);
                attrs.insert(builder.escaped, "true".to_string());
            } else {
                let text = xot.new_text(s_str);
                xot.append(el, text).unwrap();
            }
            el
        }
        json::JsonValue::String(s) => {
            let el = make_json_element(xot, builder, builder.string, key, escape);
            if escape {
                let escaped_s = v_jsonescape::escape(s).to_string();
                let text = xot.new_text(&escaped_s);
                xot.append(el, text).unwrap();
                let mut attrs = xot.attributes_mut(el);
                attrs.insert(builder.escaped, "true".to_string());
            } else {
                let text = xot.new_text(s);
                xot.append(el, text).unwrap();
            }
            el
        }
        json::JsonValue::Array(arr) => {
            let el = make_json_element(xot, builder, builder.array, key, escape);
            for item in arr.iter() {
                let child = json_value_to_xml(xot, builder, item, None, escape)?;
                xot.append(el, child).unwrap();
            }
            el
        }
        json::JsonValue::Object(obj) => {
            let el = make_json_element(xot, builder, builder.map, key, escape);
            for (k, v) in obj.iter() {
                let child = json_value_to_xml(xot, builder, v, Some(k), escape)?;
                xot.append(el, child).unwrap();
            }
            el
        }
    };
    Ok(node)
}

/// Create a JSON XML element with optional key attribute
fn make_json_element(
    xot: &mut Xot,
    builder: &JsonXmlBuilder,
    name: xot::NameId,
    key: Option<&str>,
    escape: bool,
) -> xot::Node {
    match key {
        Some(k) => builder.new_keyed_element(xot, name, k, escape),
        None => builder.new_element(xot, name),
    }
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(parse_json1),
        wrap_xpath_fn!(parse_json2),
        wrap_xpath_fn!(xml_to_json1),
        wrap_xpath_fn!(xml_to_json2),
        wrap_xpath_fn!(json_to_xml1),
        wrap_xpath_fn!(json_to_xml2),
    ]
}
