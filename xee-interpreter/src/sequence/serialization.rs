use ahash::HashMap;
use rust_decimal::Decimal;
use xot::{xmlname::OwnedName, Xot};

use xee_schema_type::Xs;

use crate::{
    atomic, context, error,
    function::{self, Map},
};

use super::{
    core::Sequence,
    item::Item,
    opc::{OptionParameterConverter, QNameOrString},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializationParameters {
    pub allow_duplicate_names: bool,
    pub byte_order_mark: bool,
    pub cdata_section_elements: Vec<OwnedName>,
    pub doctype_public: Option<String>,
    pub doctype_system: Option<String>,
    pub encoding: String,
    pub escape_uri_attributes: bool,
    pub html_version: Decimal,
    pub explicit_html_version: bool,
    pub include_content_type: bool,
    pub indent: bool,
    pub item_separator: String,
    pub json_node_output_method: QNameOrString,
    pub media_type: Option<String>,
    pub method: QNameOrString,
    pub normalization_form: Option<String>,
    pub omit_xml_declaration: bool,
    pub standalone: Option<bool>,
    pub suppress_indentation: Vec<OwnedName>,
    pub undeclare_prefixes: bool,
    pub use_character_maps: HashMap<char, String>,
    pub version: String,
}

impl SerializationParameters {
    // default values are as used in XSLT 3.0
    pub fn new() -> Self {
        Self {
            allow_duplicate_names: false,
            byte_order_mark: false,
            cdata_section_elements: Vec::new(),
            doctype_public: None,
            doctype_system: None,
            encoding: "UTF-8".to_string(),
            escape_uri_attributes: true,
            html_version: Decimal::from_str_exact("5.0").unwrap(),
            explicit_html_version: false,
            include_content_type: true,
            indent: false,
            item_separator: " ".to_string(),
            json_node_output_method: QNameOrString::String("xml".to_string()),
            media_type: Some("text/xml".to_string()),
            method: QNameOrString::String("xml".to_string()),
            normalization_form: None,
            omit_xml_declaration: false,
            standalone: None,
            suppress_indentation: Vec::new(),
            undeclare_prefixes: false,
            use_character_maps: HashMap::default(),
            version: "1.0".to_string(),
        }
    }

    pub(crate) fn from_map(
        map: Map,
        static_context: &context::StaticContext,
        xot: &Xot,
    ) -> error::Result<Self> {
        let c = OptionParameterConverter::new(&map, static_context, xot);
        let allow_duplicate_names =
            c.option_with_default("allow-duplicate-names", Xs::Boolean, false)?;

        let byte_order_mark = c.option_with_default("byte-order-mark", Xs::Boolean, false)?;

        let cdata_section_elements = c.many("cdata-section-elements", Xs::QName)?;

        let doctype_public = c.option("doctype-public", Xs::String)?;

        let doctype_system = c.option("doctype-system", Xs::String)?;

        let encoding = c.option_with_default("encoding", Xs::String, "utf-8".to_string())?;

        let escape_uri_attributes =
            c.option_with_default("escape-uri-attributes", Xs::Boolean, true)?;

        let html_version = c.option_with_default(
            "html-version",
            Xs::Decimal,
            Decimal::from_str_exact("5.0").unwrap(),
        )?;

        let include_content_type =
            c.option_with_default("include-content-type", Xs::Boolean, true)?;

        let indent = c.option_with_default("indent", Xs::Boolean, false)?;

        let item_separator =
            c.option_with_default("item-separator", Xs::String, " ".to_string())?;

        let json_node_output_method = c.qname_or_string(
            "json-node-output-method",
            QNameOrString::String("xml".to_string()),
        )?;

        let media_type = c.option("media-type", Xs::String)?;

        let method = c.qname_or_string("method", QNameOrString::String("xml".to_string()))?;

        let normalization_form = c.option("normalization-form", Xs::String)?;

        let omit_xml_declaration =
            c.option_with_default("omit-xml-declaration", Xs::Boolean, true)?;

        let standalone = c.option("standalone", Xs::Boolean)?;

        let suppress_indentation = c.many("suppress-indentation", Xs::QName)?;

        let undeclare_prefixes = c.option_with_default("undeclare-prefixes", Xs::Boolean, false)?;

        // TODO: use-character-maps

        let version = c.option_with_default("version", Xs::String, "1.0".to_string())?;

        Ok(Self {
            allow_duplicate_names,
            byte_order_mark,
            cdata_section_elements,
            doctype_public,
            doctype_system,
            encoding,
            escape_uri_attributes,
            html_version,
            explicit_html_version: false,
            include_content_type,
            indent,
            item_separator,
            json_node_output_method,
            media_type,
            method,
            normalization_form,
            omit_xml_declaration,
            standalone,
            suppress_indentation,
            undeclare_prefixes,
            use_character_maps: HashMap::default(),
            version,
        })
    }

    pub(crate) fn xml_in_json_serialization(method: &QNameOrString) -> Self {
        Self {
            // use the method given
            method: method.clone(),
            // the only thing set according to the specification
            omit_xml_declaration: true,
            // keep this around just in case, though I don't think we
            // can end up in json output from XML output
            json_node_output_method: method.clone(),
            allow_duplicate_names: false,
            byte_order_mark: false,
            cdata_section_elements: Vec::new(),
            doctype_public: None,
            doctype_system: None,
            encoding: "UTF-8".to_string(),
            escape_uri_attributes: false,
            html_version: Decimal::from_str_exact("5.0").unwrap(),
            explicit_html_version: false,
            include_content_type: false,
            indent: false,
            item_separator: " ".to_string(),
            media_type: None,
            normalization_form: None,
            standalone: None,
            suppress_indentation: Vec::new(),
            undeclare_prefixes: false,
            use_character_maps: HashMap::default(),
            version: "1.0".to_string(),
        }
    }
}

impl Default for SerializationParameters {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn serialize_sequence(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> error::Result<String> {
    if let Some(local_name) = parameters.method.local_name() {
        match local_name {
            "adaptive" => serialize_adaptive(arg, parameters, xot),
            "xml" => serialize_xml(arg, parameters, xot),
            "html" => serialize_html(arg, parameters, xot),
            "xhtml" => serialize_xhtml(arg, parameters, xot),
            "json" => serialize_json(arg, parameters, xot),
            "text" => serialize_text(arg, parameters, xot),
            _ => Err(error::Error::SEPM0016),
        }
    } else {
        Err(error::Error::SEPM0016)
    }
}

fn serialize_adaptive(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let serialized = serialize_adaptive_items(arg.iter(), &parameters.item_separator, &parameters, xot)?;
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_adaptive_items<'a>(
    items: impl Iterator<Item = Item>,
    separator: &str,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let mut serialized = Vec::new();
    for item in items {
        serialized.push(serialize_adaptive_item(item, parameters, xot)?);
    }
    Ok(serialized.join(separator))
}

fn serialize_adaptive_item(
    item: Item,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match item {
        Item::Atomic(atomic) => Ok(atomic.xpath_representation()),
        Item::Node(node) => serialize_adaptive_node(node, parameters, xot),
        Item::Function(function) => serialize_adaptive_function(&function, parameters, xot),
    }
}

fn serialize_adaptive_node(
    node: xot::Node,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match xot.value(node) {
        xot::Value::Attribute(attribute) => {
            let (local_name, namespace) = xot.name_ns_str(attribute.name());
            let name = adaptive_node_name(local_name, namespace);
            Ok(format!(
                "{}=\"{}\"",
                name,
                escape_attribute_value(attribute.value())
            ))
        }
        xot::Value::Namespace(namespace) => {
            let prefix = xot.prefix_str(namespace.prefix());
            let name = if prefix.is_empty() {
                "xmlns".to_string()
            } else {
                format!("xmlns:{}", prefix)
            };
            Ok(format!(
                "{}=\"{}\"",
                name,
                escape_attribute_value(xot.namespace_str(namespace.namespace()))
            ))
        }
        _ => {
            let mut xml_parameters = parameters.clone();
            xml_parameters.method = QNameOrString::String("xml".to_string());
            xml_parameters.omit_xml_declaration = true;
            let sequence: Sequence = vec![node].into();
            serialize_xml(&sequence, xml_parameters, xot)
        }
    }
}

fn serialize_adaptive_function(
    function: &function::Function,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match function {
        function::Function::Array(array) => serialize_adaptive_array(array, parameters, xot),
        function::Function::Map(map) => serialize_adaptive_map(map, parameters, xot),
        _ => Ok("function(*)".to_string()),
    }
}

fn serialize_adaptive_array(
    array: &function::Array,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let mut members = Vec::with_capacity(array.len());
    for member in array.iter() {
        members.push(serialize_adaptive_member_sequence(member, parameters, xot)?);
    }
    Ok(format!("[{}]", members.join(",")))
}

fn serialize_adaptive_map(
    map: &function::Map,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let mut entries = Vec::with_capacity(map.len());
    for (key, value) in map.entries() {
        entries.push(format!(
            "{}:{}",
            key.xpath_representation(),
            serialize_adaptive_member_sequence(value, parameters, xot)?
        ));
    }
    entries.sort();
    Ok(format!("map{{{}}}", entries.join(",")))
}

fn serialize_adaptive_member_sequence(
    sequence: &Sequence,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match sequence.len() {
        0 => Ok("()".to_string()),
        1 => serialize_adaptive_item(sequence.iter().next().unwrap(), parameters, xot),
        _ => Ok(format!(
            "({})",
            serialize_adaptive_items(sequence.iter(), ", ", parameters, xot)?
        )),
    }
}

fn adaptive_node_name(local_name: &str, namespace: &str) -> String {
    if namespace.is_empty() {
        local_name.to_string()
    } else {
        format!("Q{{{}}}{}", namespace, local_name)
    }
}

fn escape_attribute_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            '\t' => escaped.push_str("&#x9;"),
            '\n' => escaped.push_str("&#xA;"),
            '\r' => escaped.push_str("&#xD;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn serialize_text(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let node = arg.normalize(&parameters.item_separator, xot)?;
    let serialized = apply_character_maps(&xot.string_value(node), &parameters.use_character_maps);
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_xml(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let node = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    let indentation = xot_indentation(&parameters, xot);
    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    let declaration = if !parameters.omit_xml_declaration {
        Some(xot::output::xml::Declaration {
            encoding: Some(parameters.encoding.to_string()),
            standalone: parameters.standalone,
        })
    } else {
        None
    };
    let doctype = match (
        parameters.doctype_public.clone(),
        parameters.doctype_system.clone(),
    ) {
        (Some(public), Some(system)) => Some(xot::output::xml::DocType::Public { public, system }),
        (None, Some(system)) => Some(xot::output::xml::DocType::System { system }),
        // TODO: this should really not happen?
        (Some(public), None) => Some(xot::output::xml::DocType::Public {
            public,
            system: "".to_string(),
        }),
        (None, None) => None,
    };
    let output_parameters = xot::output::xml::Parameters {
        indentation,
        cdata_section_elements,
        declaration,
        doctype,
        ..Default::default()
    };

    Ok(apply_byte_order_mark(
        xot.serialize_xml_string(output_parameters, node)?,
        &parameters,
    ))
}

fn serialize_html(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let node = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    if xot.document_element(node).is_err() {
        return Ok(xot.string_value(node));
    }
    // TODO: no check yet for html version rejecting versions that aren't 5
    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    let indentation = xot_indentation(&parameters, xot);
    let html5 = xot.html5();
    let output_parameters = xot::output::html5::Parameters {
        indentation,
        cdata_section_elements,
    };
    let mut serialized = html5.serialize_string(output_parameters, node)?;

    if parameters.include_content_type {
        inject_content_type_meta(&mut serialized, &parameters, false);
    }

    if parameters.explicit_html_version {
        if parameters.html_version >= Decimal::from_str_exact("5.0").unwrap() {
            ensure_html5_doctype(&mut serialized);
        } else {
            remove_html5_doctype(&mut serialized);
        }
    }

    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_xhtml(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let node = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    if xot.document_element(node).is_err() {
        return Ok(xot.string_value(node));
    }

    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    let indentation = xot_indentation(&parameters, xot);
    let html5 = xot.html5();
    let output_parameters = xot::output::html5::Parameters {
        indentation,
        cdata_section_elements,
    };
    let mut serialized = html5.serialize_string(output_parameters, node)?;

    if parameters.include_content_type {
        inject_content_type_meta(&mut serialized, &parameters, true);
    }

    if parameters.explicit_html_version {
        if parameters.html_version >= Decimal::from_str_exact("5.0").unwrap() {
            ensure_html5_doctype(&mut serialized);
        } else {
            remove_html5_doctype(&mut serialized);
        }
    }

    remove_redundant_default_namespace(&mut serialized, " xmlns=\"http://www.w3.org/1999/xhtml\"");

    if !parameters.omit_xml_declaration {
        serialized = format!(
            "<?xml version=\"1.0\" encoding=\"{}\"?>\n{}",
            parameters.encoding, serialized
        );
    }

    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn inject_content_type_meta(
    serialized: &mut String,
    parameters: &SerializationParameters,
    self_closing: bool,
) {
    let media_type = parameters
        .media_type
        .clone()
        .unwrap_or_else(|| "text/html".to_string());
    let closing = if self_closing { " />" } else { ">" };
    let meta = format!(
        "<meta http-equiv=\"Content-Type\" content=\"{}; charset={}\"{}",
        media_type, parameters.encoding, closing
    );
    if let Some(position) = serialized.find("</head>") {
        serialized.insert_str(position, &meta);
    }
}

fn remove_redundant_default_namespace(serialized: &mut String, namespace_attr: &str) {
    let Some(first_position) = serialized.find(namespace_attr) else {
        return;
    };
    let mut search_start = first_position + namespace_attr.len();
    while let Some(relative_position) = serialized[search_start..].find(namespace_attr) {
        let position = search_start + relative_position;
        serialized.replace_range(position..position + namespace_attr.len(), "");
        search_start = position;
    }
}

fn ensure_html5_doctype(serialized: &mut String) {
    if !serialized.trim_start().starts_with("<!DOCTYPE html>") {
        *serialized = format!("<!DOCTYPE html>\n{serialized}");
    }
}

fn remove_html5_doctype(serialized: &mut String) {
    let trimmed = serialized.trim_start();
    let Some(rest) = trimmed.strip_prefix("<!DOCTYPE html>") else {
        return;
    };
    *serialized = rest.trim_start_matches('\n').to_string();
}

fn map_serialization_node(
    node: xot::Node,
    character_maps: &HashMap<char, String>,
    xot: &mut Xot,
) -> xot::Node {
    let mapped_root = xot.clone_node(node);
    normalize_default_namespace_nodes(mapped_root, xot);

    if character_maps.is_empty() {
        return mapped_root;
    }

    let mut text_nodes = Vec::new();
    if matches!(xot.value(mapped_root), xot::Value::Text(_)) {
        text_nodes.push(mapped_root);
    }
    text_nodes.extend(
        xot.descendants(mapped_root)
            .filter(|descendant| matches!(xot.value(*descendant), xot::Value::Text(_))),
    );

    for text_node in text_nodes {
        if let Some(text) = xot.text_mut(text_node) {
            let current = text.get().to_string();
            let mapped = apply_character_maps(&current, character_maps);
            if mapped != current {
                text.set(mapped);
            }
        }
    }

    mapped_root
}

fn normalize_default_namespace_nodes(root: xot::Node, xot: &mut Xot) {
    let mut elements = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    for element in elements {
        let parent_default_namespace = xot
            .parent(element)
            .filter(|parent| xot.is_element(*parent))
            .and_then(|parent| {
                xot.namespaces_in_scope(parent)
                    .find(|(prefix, _)| xot.prefix_str(*prefix).is_empty())
                    .map(|(_, namespace)| xot.namespace_str(namespace).to_string())
            })
            .unwrap_or_default();

        let element_namespace = xot
            .node_name(element)
            .map(|name| xot.namespace_str(xot.namespace_for_name(name)).to_string())
            .unwrap_or_default();

        let explicit_default_namespace =
            xot.children(element)
                .find_map(|child| match xot.value(child) {
                    xot::Value::Namespace(namespace)
                        if xot.prefix_str(namespace.prefix()).is_empty() =>
                    {
                        Some((child, xot.namespace_str(namespace.namespace()).to_string()))
                    }
                    _ => None,
                });

        if element_namespace.is_empty() && !parent_default_namespace.is_empty() {
            if explicit_default_namespace
                .as_ref()
                .map(|(_, uri)| uri.as_str())
                != Some("")
            {
                if let Some((child, _)) = explicit_default_namespace {
                    let _ = xot.remove(child);
                }
                let prefix = xot.add_prefix("");
                let namespace = xot.add_namespace("");
                let node = xot.new_namespace_node(prefix, namespace);
                xot.any_append(element, node).unwrap();
            }
        } else if explicit_default_namespace
            .as_ref()
            .map(|(_, uri)| uri.as_str())
            == Some(parent_default_namespace.as_str())
        {
            if let Some((child, _)) = explicit_default_namespace {
                let _ = xot.remove(child);
            }
        }
    }
}

fn apply_character_maps(value: &str, character_maps: &HashMap<char, String>) -> String {
    if character_maps.is_empty() {
        return value.to_string();
    }

    let mut mapped = String::new();
    for character in value.chars() {
        if let Some(replacement) = character_maps.get(&character) {
            mapped.push_str(replacement);
        } else {
            mapped.push(character);
        }
    }
    mapped
}

fn apply_byte_order_mark(mut serialized: String, parameters: &SerializationParameters) -> String {
    if parameters.byte_order_mark {
        serialized.insert(0, '\u{FEFF}');
    }
    serialized
}

fn serialize_json(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let r = serialize_json_sequence(arg, &parameters, xot)?;
    Ok(r.dump())
}

fn serialize_json_sequence(
    arg: &Sequence,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    match arg {
        Sequence::One(item) => serialize_json_item(item.item(), parameters, xot),
        Sequence::Empty(_) => Ok(json::JsonValue::Null),
        Sequence::Many(_) | Sequence::Range(_) => Err(error::Error::SERE0023),
    }
}

fn serialize_json_item(
    item: &Item,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    match item {
        Item::Atomic(atomic) => serialize_json_atomic(atomic, parameters),
        Item::Node(node) => serialize_json_node(*node, parameters, xot),
        Item::Function(function) => serialize_json_function(function, parameters, xot),
    }
}

fn serialize_json_atomic(
    atomic: &atomic::Atomic,
    parameters: &SerializationParameters,
) -> Result<json::JsonValue, error::Error> {
    match atomic {
        atomic::Atomic::Float(float) => {
            let f = float.into_inner();
            if f.is_infinite() || f.is_nan() {
                return Err(error::Error::SERE0020);
            }
            Ok(json::JsonValue::Number(f.into()))
        }
        atomic::Atomic::Double(double) => {
            let d = double.into_inner();
            if d.is_infinite() || d.is_nan() {
                return Err(error::Error::SERE0020);
            }
            Ok(json::JsonValue::Number(d.into()))
        }
        atomic::Atomic::Decimal(decimal) => {
            let d: f64 = (*decimal.as_ref())
                .try_into()
                .map_err(|_| error::Error::SERE0020)?;
            Ok(json::JsonValue::Number(d.into()))
        }
        atomic::Atomic::Integer(_t, integer) => {
            let i: f64 = integer.to_f64();
            Ok(json::JsonValue::Number(i.into()))
        }
        atomic::Atomic::Boolean(b) => Ok(json::JsonValue::Boolean(*b)),
        _ => {
            let s = atomic.string_value();
            Ok(serialize_json_string(s, parameters))
        }
    }
}

fn serialize_json_string(s: String, _parameters: &SerializationParameters) -> json::JsonValue {
    // TODO: normalization-form

    // NOTE: tests serialize-json-127 and serialize-json-128 fail because
    // the forward slash (solidus) character is not escaped. This is because
    // the json crate does not do so. This is consistent with the JSON RFC
    // https://softwareengineering.stackexchange.com/questions/444480/json-rfc8259-escape-forward-slash-or-not
    // but not consistent with the serialization spec which wrongfully manadates
    // it anyway.
    // https://www.w3.org/TR/xslt-xquery-serialization-31/#json-output
    json::JsonValue::String(s)
}

fn serialize_json_node(
    node: xot::Node,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    match parameters.json_node_output_method.local_name() {
        Some("xml") | Some("html") => {
            let xml_parameters = SerializationParameters::xml_in_json_serialization(
                &parameters.json_node_output_method,
            );
            let sequence: Sequence = vec![node].into();
            let s = serialize_sequence(&sequence, xml_parameters, xot)?;
            Ok(serialize_json_string(s, parameters))
        }
        _ => todo!(),
    }
}

fn serialize_json_function(
    function: &function::Function,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    match function {
        function::Function::Array(array) => serialize_json_array(array, parameters, xot),
        function::Function::Map(map) => serialize_json_map(map, parameters, xot),
        _ => Err(error::Error::SERE0021),
    }
}

fn serialize_json_array(
    array: &function::Array,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    let mut result = Vec::with_capacity(array.len());
    for entry in array.iter() {
        let serialized = serialize_json_sequence(entry, parameters, xot)?;
        result.push(serialized);
    }
    Ok(json::JsonValue::Array(result))
}

fn serialize_json_map(
    map: &function::Map,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<json::JsonValue, error::Error> {
    let mut result = json::object::Object::new();
    for key in map.keys() {
        let key_s = key.string_value();
        let value = map.get(key).unwrap();
        let value = serialize_json_sequence(value, parameters, xot)?;
        result.insert(&key_s, value);
    }
    Ok(json::JsonValue::Object(result))
}

fn xot_indentation(
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Option<xot::output::Indentation> {
    if !parameters.indent {
        return None;
    }
    let suppress = xot_names(&parameters.suppress_indentation, xot);
    Some(xot::output::Indentation { suppress })
}

fn xot_names(names: &[xot::xmlname::OwnedName], xot: &mut Xot) -> Vec<xot::NameId> {
    names
        .iter()
        .map(|owned_name| owned_name.to_ref(xot).name_id())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{atomic, sequence};

    use super::*;

    #[test]
    fn test_allow_duplicate_names_true() {
        let map = Map::new(vec![(
            "allow-duplicate-names".to_string().into(),
            sequence::Sequence::from(vec![atomic::Atomic::Boolean(true)]),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert!(params.allow_duplicate_names);
    }

    #[test]
    fn test_allow_duplicate_names_false() {
        let map = Map::new(vec![(
            "allow-duplicate-names".to_string().into(),
            sequence::Sequence::from(vec![atomic::Atomic::Boolean(false)]),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert!(!params.allow_duplicate_names);
    }

    #[test]
    fn test_allow_duplicate_names_default_empty_sequence() {
        let map = Map::new(vec![(
            "allow-duplicate-names".to_string().into(),
            sequence::Sequence::default(),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert!(!params.allow_duplicate_names);
    }

    #[test]
    fn test_allow_duplicate_names_missing() {
        let map = Map::new(vec![]).unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert!(!params.allow_duplicate_names);
    }

    #[test]
    fn test_cdata_section_elements() {
        let html = OwnedName::new("html".to_string(), "".to_string(), "".to_string());
        let script = OwnedName::new("script".to_string(), "".to_string(), "".to_string());
        let map = Map::new(vec![(
            "cdata-section-elements".to_string().into(),
            sequence::Sequence::from(vec![
                atomic::Atomic::QName(html.clone().into()),
                atomic::Atomic::QName(script.clone().into()),
            ]),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert_eq!(params.cdata_section_elements.len(), 2);
        assert_eq!(params.cdata_section_elements[0], html);
        assert_eq!(params.cdata_section_elements[1], script);
    }

    #[test]
    fn test_qname_or_string_string() {
        let json: atomic::Atomic = "json".to_string().into();
        let map = Map::new(vec![(
            "json-node-output-method".to_string().into(),
            sequence::Sequence::from(vec![json]),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert_eq!(
            params.json_node_output_method,
            QNameOrString::String("json".to_string())
        );
    }

    #[test]
    fn test_qname_or_string_qname() {
        let owned_name = OwnedName::new("json".to_string(), "".to_string(), "".to_string());
        let json: atomic::Atomic = owned_name.clone().into();
        let map = Map::new(vec![(
            "json-node-output-method".to_string().into(),
            sequence::Sequence::from(vec![json]),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert_eq!(
            params.json_node_output_method,
            QNameOrString::QName(owned_name)
        );
    }

    #[test]
    fn test_qname_or_string_default_empty_sequence() {
        let map = Map::new(vec![(
            "json-node-output-method".to_string().into(),
            sequence::Sequence::default(),
        )])
        .unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert_eq!(
            params.json_node_output_method,
            QNameOrString::String("xml".to_string())
        );
    }

    #[test]
    fn test_qname_or_string_default_missing() {
        let map = Map::new(vec![]).unwrap();
        let static_context = context::StaticContext::default();
        let xot = Xot::new();
        let params = SerializationParameters::from_map(map, &static_context, &xot).unwrap();
        assert_eq!(
            params.json_node_output_method,
            QNameOrString::String("xml".to_string())
        );
    }

    #[test]
    fn test_serialize_adaptive_map_element_and_attribute_sequence() {
        let mut xot = Xot::new();
        let map = Map::new(vec![(
            "a".to_string().into(),
            sequence::Sequence::from(vec![atomic::Atomic::from(22)]),
        )])
        .unwrap();
        let elem_name = xot.add_name("elem");
        let element = xot.new_element(elem_name);
        let attr_name = xot.add_name("a");
        let attribute = xot.new_attribute_node(attr_name, "5".to_string());

        let sequence = Sequence::from(vec![
            Item::from(function::Function::Map(map)),
            Item::from(element),
            Item::from(attribute),
        ]);
        let mut params = SerializationParameters::new();
        params.method = QNameOrString::String("adaptive".to_string());
        params.item_separator = "|".to_string();

        assert_eq!(
            serialize_sequence(&sequence, params, &mut xot).unwrap(),
            "map{\"a\":22}|<elem/>|a=\"5\""
        );
    }
}
