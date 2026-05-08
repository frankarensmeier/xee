use ahash::{HashMap, HashSet, HashSetExt};
use rust_decimal::Decimal;
use unicode_normalization::UnicodeNormalization;
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    pub explicit_method: bool,
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
            explicit_method: false,
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
            explicit_method: false,
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
        let html_in_json = matches!(method.local_name(), Some("html"));
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
            explicit_method: false,
            include_content_type: html_in_json,
            indent: false,
            item_separator: " ".to_string(),
            media_type: if html_in_json {
                Some("text/html".to_string())
            } else {
                None
            },
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
    let serialized = apply_normalization_form(serialized, &parameters)?;
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_xml(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let (node, placeholders, reverse_placeholders) = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    let indentation = xot_indentation(&parameters, xot);
    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    // Per spec, character maps do not apply to text in CDATA section elements.
    revert_placeholders_in_cdata_elements(node, &cdata_section_elements, &reverse_placeholders, xot);
    let declaration = if !parameters.omit_xml_declaration {
        Some(xot::output::xml::Declaration {
            encoding: Some(parameters.encoding.to_string()),
            standalone: parameters.standalone,
        })
    } else {
        None
    };
    let doctype = build_xml_doctype(&parameters);
    let output_parameters = xot::output::xml::Parameters {
        indentation,
        cdata_section_elements,
        declaration,
        doctype,
        ..Default::default()
    };

    let serialized = xot.serialize_xml_string(output_parameters, node)?;
    let serialized = apply_character_map_placeholders(serialized, &placeholders);

    // xot unconditionally adds a newline after the XML declaration (?>).
    // When indent="no", the spec requires no whitespace between the
    // declaration and the document element, so strip it.
    let serialized = if !parameters.indent && !parameters.omit_xml_declaration {
        strip_post_declaration_newline(&serialized)
    } else {
        serialized
    };

    // Per spec, the DOCTYPE must appear immediately before the first element.
    // xot places it after the XML declaration but before comments/PIs.
    let serialized = reposition_doctype_before_first_element(serialized);

    let serialized = apply_normalization_form(serialized, &parameters)?;
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_html(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let (node, placeholders, reverse_placeholders) = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    if xot.document_element(node).is_err() {
        return Ok(xot.string_value(node));
    }
    materialize_html_foreign_attribute_namespaces(node, xot);
    if parameters.escape_uri_attributes {
        revert_placeholders_in_uri_attributes(node, &reverse_placeholders, xot);
        escape_uri_attributes(node, xot);
    }
    if parameters.include_content_type {
        inject_content_type_meta(node, &parameters, xot);
    }
    // TODO: no check yet for html version rejecting versions that aren't 5
    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    let indentation = xot_indentation(&parameters, xot);
    let html5 = xot.html5();
    let output_parameters = xot::output::html5::Parameters {
        indentation,
        cdata_section_elements,
    };
    let serialized = html5.serialize_string(output_parameters, node)?;
    let mut serialized = apply_character_map_placeholders(serialized, &placeholders);

    if parameters.explicit_html_version {
        if parameters.html_version >= Decimal::from_str_exact("5.0").unwrap() {
            // Remove xot's DOCTYPE (always at start), re-add at correct position
            remove_html5_doctype(&mut serialized);
            ensure_html5_doctype(&mut serialized);
        } else {
            remove_html5_doctype(&mut serialized);
        }
    } else {
        remove_html5_doctype(&mut serialized);
    }

    let serialized = apply_normalization_form(serialized, &parameters)?;
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn serialize_xhtml(
    arg: &Sequence,
    parameters: SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let (node, placeholders, reverse_placeholders) = map_serialization_node(
        arg.normalize(&parameters.item_separator, xot)?,
        &parameters.use_character_maps,
        xot,
    );
    if xot.document_element(node).is_err() {
        return Ok(xot.string_value(node));
    }

    materialize_xhtml_nonvoid_empty_elements(node, xot);

    let html_root = is_html_root_element(node, xot);
    let xhtml5 = parameters.explicit_html_version
        && parameters.html_version >= Decimal::from_str_exact("5.0").unwrap();
    if xhtml5 {
        strip_xhtml5_namespace_prefixes(node, xot);
    }
    if parameters.escape_uri_attributes {
        escape_uri_attributes(node, xot);
    }
    if parameters.include_content_type && html_root {
        inject_content_type_meta(node, &parameters, xot);
    }

    // XHTML uses the XML output method for serialization (well-formed XML)
    let indentation = xot_indentation(&parameters, xot);
    let cdata_section_elements = xot_names(&parameters.cdata_section_elements, xot);
    // Per spec, character maps do not apply to text in CDATA section elements.
    revert_placeholders_in_cdata_elements(node, &cdata_section_elements, &reverse_placeholders, xot);
    let declaration = if !parameters.omit_xml_declaration {
        Some(xot::output::xml::Declaration {
            encoding: Some(parameters.encoding.to_string()),
            standalone: parameters.standalone,
        })
    } else {
        None
    };

    // DOCTYPE from explicit doctype-public/doctype-system
    let doctype = build_xml_doctype(&parameters);

    let output_parameters = xot::output::xml::Parameters {
        indentation,
        cdata_section_elements,
        declaration,
        doctype,
        ..Default::default()
    };

    let serialized = xot.serialize_xml_string(output_parameters, node)?;
    let mut serialized = apply_character_map_placeholders(serialized, &placeholders);

    // XHTML compatibility: add space before /> in empty elements
    serialized = add_xhtml_space_before_self_closing(&serialized);

    // XHTML compatibility: replace &apos; with literal apostrophe.
    // Some browsers don't support &apos; and in XML with double-quoted
    // attributes, apostrophes don't need escaping.
    serialized = serialized.replace("&apos;", "'");

    // XHTML: escape C1 control characters as numeric character references
    // for browser compatibility.
    serialized = escape_c1_control_characters(&serialized);

    // Strip post-declaration newline when indent="no"
    if !parameters.indent && !parameters.omit_xml_declaration {
        serialized = strip_post_declaration_newline(&serialized);
    }

    // For XHTML 5 with html root, add <!DOCTYPE html> when no effective doctype.
    // Per spec, doctype-public without doctype-system is treated as absent.
    let has_effective_doctype = build_xml_doctype(&parameters).is_some();
    if html_root && xhtml5 && !has_effective_doctype {
        ensure_html5_doctype(&mut serialized);
    }

    let serialized = apply_normalization_form(serialized, &parameters)?;
    Ok(apply_byte_order_mark(serialized, &parameters))
}

fn inject_content_type_meta(
    root: xot::Node,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) {
    let Ok(document_element) = xot.document_element(root) else {
        return;
    };
    let Some(head) = html_head_node(document_element, xot) else {
        return;
    };

    let media_type = parameters
        .media_type
        .clone()
        .unwrap_or_else(|| "text/html".to_string());
    let http_equiv_name = xot.add_name("http-equiv");
    let content_name = xot.add_name("content");
    if let Some(existing_meta) = head_content_type_meta(head, http_equiv_name, xot) {
        let http_equiv_attr = xot.new_attribute_node(http_equiv_name, "Content-Type".to_string());
        let content_attr = xot.new_attribute_node(
            content_name,
            format!("{}; charset={}", media_type, parameters.encoding),
        );
        xot.append_attribute_node(existing_meta, http_equiv_attr).unwrap();
        xot.append_attribute_node(existing_meta, content_attr).unwrap();
        return;
    }
    let namespace_id = xot
        .node_name(head)
        .map(|name_id| xot.namespace_for_name(name_id))
        .unwrap();
    let meta_name = if xot.namespace_str(namespace_id).is_empty() {
        xot.add_name("meta")
    } else {
        xot.add_name_ns("meta", namespace_id)
    };
    let meta = xot.new_element(meta_name);
    let http_equiv_attr = xot.new_attribute_node(http_equiv_name, "Content-Type".to_string());
    let content_attr = xot.new_attribute_node(
        content_name,
        format!("{}; charset={}", media_type, parameters.encoding),
    );
    xot.append_attribute_node(meta, http_equiv_attr).unwrap();
    xot.append_attribute_node(meta, content_attr).unwrap();
    xot.prepend(head, meta).unwrap();
}

fn html_head_node(document_element: xot::Node, xot: &Xot) -> Option<xot::Node> {
    let Some(name_id) = xot.node_name(document_element) else {
        return None;
    };
    let (local_name, namespace) = xot.name_ns_str(name_id);
    let html_namespace = namespace.is_empty() || namespace == "http://www.w3.org/1999/xhtml";
    if !html_namespace {
        return None;
    }
    if local_name.eq_ignore_ascii_case("head") {
        return Some(document_element);
    }
    if !local_name.eq_ignore_ascii_case("html") {
        return None;
    }
    xot.children(document_element).find(|child| {
        if !xot.is_element(*child) {
            return false;
        }
        let Some(name_id) = xot.node_name(*child) else {
            return false;
        };
        let (local_name, namespace) = xot.name_ns_str(name_id);
        local_name.eq_ignore_ascii_case("head")
            && (namespace.is_empty() || namespace == "http://www.w3.org/1999/xhtml")
    })
}

fn head_content_type_meta(head: xot::Node, _http_equiv_name: xot::NameId, xot: &Xot) -> Option<xot::Node> {
    xot.children(head).find(|child| {
        if !xot.is_element(*child) {
            return false;
        }
        let Some(name_id) = xot.node_name(*child) else {
            return false;
        };
        let (local_name, namespace) = xot.name_ns_str(name_id);
        if !local_name.eq_ignore_ascii_case("meta")
            || !(namespace.is_empty() || namespace == "http://www.w3.org/1999/xhtml")
        {
            return false;
        }
        // Check for http-equiv="Content-Type" case-insensitively,
        // because HTML attribute names may be uppercase in the source
        xot.attributes(*child)
            .iter()
            .any(|(attr_name, value)| {
                let (attr_local, _) = xot.name_ns_str(attr_name);
                attr_local.eq_ignore_ascii_case("http-equiv")
                    && value.eq_ignore_ascii_case("Content-Type")
            })
    })
}

fn ensure_html5_doctype(serialized: &mut String) {
    if serialized.contains("<!DOCTYPE ") {
        return;
    }
    // Extract the root element name to match its case in the DOCTYPE.
    // Only insert DOCTYPE if the root element is "html" (case-insensitive).
    if let Some(pos) = find_first_element_position(serialized) {
        let rest = &serialized[pos + 1..];
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if name.eq_ignore_ascii_case("html") {
            let doctype = format!("<!DOCTYPE {name}>");
            serialized.insert_str(pos, &doctype);
        }
    }
}

/// Find the byte position of the first element start tag in the serialized output.
/// Skips XML declaration, comments, processing instructions, and whitespace.
fn find_first_element_position(s: &str) -> Option<usize> {
    let mut pos = 0;
    let bytes = s.as_bytes();
    while pos < bytes.len() {
        match bytes[pos] {
            b' ' | b'\t' | b'\n' | b'\r' => pos += 1,
            b'<' => {
                if s[pos..].starts_with("<?") {
                    // Skip PI
                    if let Some(end) = s[pos..].find("?>") {
                        pos += end + 2;
                    } else {
                        return Some(pos);
                    }
                } else if s[pos..].starts_with("<!--") {
                    // Skip comment
                    if let Some(end) = s[pos..].find("-->") {
                        pos += end + 3;
                    } else {
                        return Some(pos);
                    }
                } else if s[pos..].starts_with("<!DOCTYPE") {
                    // Skip existing DOCTYPE
                    if let Some(end) = s[pos..].find('>') {
                        pos += end + 1;
                    } else {
                        return Some(pos);
                    }
                } else {
                    // This is an element start tag
                    return Some(pos);
                }
            }
            _ => return Some(pos),
        }
    }
    None
}

fn is_html_root_element(node: xot::Node, xot: &Xot) -> bool {
    let Ok(doc_el) = xot.document_element(node) else {
        return false;
    };
    let Some(name_id) = xot.node_name(doc_el) else {
        return false;
    };
    let (local, ns) = xot.name_ns_str(name_id);
    local.eq_ignore_ascii_case("html") && (ns.is_empty() || ns == "http://www.w3.org/1999/xhtml")
}

/// HTML attributes whose values are URIs and should be percent-encoded
/// when escape-uri-attributes="yes".
/// See XSLT 3.0 spec §20.1 and HTML spec.
const URI_ATTRIBUTES: &[&str] = &[
    "action",
    "archive",
    "background",
    "cite",
    "classid",
    "codebase",
    "data",
    "datasrc",
    "dynsrc",
    "formaction",
    "href",
    "icon",
    "longdesc",
    "lowsrc",
    "manifest",
    "pluginspage",
    "poster",
    "profile",
    "src",
    "srcset",
    "usemap",
];

/// Revert PUA character map placeholders in URI attributes.
/// Per spec, character maps are not applied to HTML attributes where
/// URI-escaping has been applied (see Bug 2459 resolution).
fn revert_placeholders_in_uri_attributes(
    root: xot::Node,
    reverse_placeholders: &ReversePlaceholderMap,
    xot: &mut Xot,
) {
    if reverse_placeholders.is_empty() {
        return;
    }
    let mut elements: Vec<xot::Node> = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    for element in elements {
        let Some(name_id) = xot.node_name(element) else {
            continue;
        };
        let namespace = xot.namespace_str(xot.namespace_for_name(name_id));
        if !namespace.is_empty() && namespace != "http://www.w3.org/1999/xhtml" {
            continue;
        }

        let mut updates: Vec<(xot::NameId, String)> = Vec::new();
        for (attr_name, attr_value) in xot.attributes(element).iter() {
            let (local, attr_ns) = xot.name_ns_str(attr_name);
            if !attr_ns.is_empty() {
                continue;
            }
            let local_lower = local.to_ascii_lowercase();
            if URI_ATTRIBUTES.contains(&local_lower.as_str()) {
                let reverted: String = attr_value
                    .chars()
                    .map(|c| *reverse_placeholders.get(&c).unwrap_or(&c))
                    .collect();
                if reverted != *attr_value {
                    updates.push((attr_name, reverted));
                }
            }
        }

        for (attr_name, new_value) in updates {
            xot.attributes_mut(element).insert(attr_name, new_value);
        }
    }
}

/// Revert PUA character map placeholders in text nodes that are children of
/// CDATA section elements. Per spec, character maps do not apply to text
/// that will be serialized as CDATA sections.
fn revert_placeholders_in_cdata_elements(
    root: xot::Node,
    cdata_section_elements: &[xot::NameId],
    reverse_placeholders: &ReversePlaceholderMap,
    xot: &mut Xot,
) {
    if reverse_placeholders.is_empty() || cdata_section_elements.is_empty() {
        return;
    }

    let mut text_nodes_to_revert = Vec::new();

    // Find all text nodes whose parent is a CDATA section element
    let all_nodes: Vec<_> = std::iter::once(root)
        .chain(xot.descendants(root))
        .collect();

    for node in all_nodes {
        if let Some(_text) = xot.text(node) {
            if let Some(parent) = xot.parent(node) {
                if let Some(parent_name) = xot.node_name(parent) {
                    if cdata_section_elements.contains(&parent_name) {
                        text_nodes_to_revert.push(node);
                    }
                }
            }
        }
    }

    for text_node in text_nodes_to_revert {
        if let Some(text) = xot.text_mut(text_node) {
            let current = text.get().to_string();
            let reverted: String = current
                .chars()
                .map(|c| *reverse_placeholders.get(&c).unwrap_or(&c))
                .collect();
            if reverted != current {
                text.set(reverted);
            }
        }
    }
}

/// The three namespaces that should use default namespace declarations
/// (no prefix) in XHTML5 output.
const XHTML5_DEFAULT_NS: &[&str] = &[
    "http://www.w3.org/1999/xhtml",
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1998/Math/MathML",
];

/// Strip namespace prefixes from elements in XHTML, SVG, and MathML namespaces
/// for XHTML5 output. These namespaces should use default namespace declarations.
fn strip_xhtml5_namespace_prefixes(root: xot::Node, xot: &mut Xot) {
    let elements: Vec<xot::Node> = std::iter::once(root)
        .chain(xot.descendants(root))
        .filter(|n| xot.is_element(*n))
        .collect();

    let empty_prefix = xot.empty_prefix();

    // Collect namespace changes: (element, prefix_to_remove, namespace_id)
    let mut changes: Vec<(xot::Node, xot::PrefixId, xot::NamespaceId)> = Vec::new();
    for element in &elements {
        // Iterate namespace declarations on this element
        for (prefix_id, ns_id) in xot.namespaces(*element).iter() {
            if prefix_id == empty_prefix {
                continue; // Already default namespace
            }
            let ns = xot.namespace_str(*ns_id);
            if XHTML5_DEFAULT_NS.contains(&ns) {
                changes.push((*element, prefix_id, *ns_id));
            }
        }
    }

    for (element, prefix_to_remove, ns_id) in changes {
        // Remove the prefixed declaration and add default namespace
        xot.remove_namespace(element, prefix_to_remove);
        xot.set_namespace(element, empty_prefix, ns_id);
    }
}

/// Percent-encode non-ASCII characters in URI-type attributes of HTML/XHTML elements.
fn escape_uri_attributes(root: xot::Node, xot: &mut Xot) {
    let mut elements: Vec<xot::Node> = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    for element in elements {
        let Some(name_id) = xot.node_name(element) else {
            continue;
        };
        let namespace = xot.namespace_str(xot.namespace_for_name(name_id));
        // Only apply to HTML/XHTML elements (empty namespace or XHTML namespace)
        if !namespace.is_empty() && namespace != "http://www.w3.org/1999/xhtml" {
            continue;
        }

        let mut updates: Vec<(xot::NameId, String)> = Vec::new();
        for (attr_name, attr_value) in xot.attributes(element).iter() {
            let (local, attr_ns) = xot.name_ns_str(attr_name);
            // Only URI-escape attributes in no namespace (HTML attributes)
            if !attr_ns.is_empty() {
                continue;
            }
            let local_lower = local.to_ascii_lowercase();
            if URI_ATTRIBUTES.contains(&local_lower.as_str()) {
                let escaped = percent_encode_non_ascii(attr_value);
                if escaped != *attr_value {
                    updates.push((attr_name, escaped));
                }
            }
        }

        for (attr_name, new_value) in updates {
            xot.attributes_mut(element)
                .insert(attr_name, new_value);
        }
    }
}

/// Percent-encode non-ASCII bytes in a URI string.
/// ASCII characters are left as-is; non-ASCII characters are NFC-normalized,
/// UTF-8 encoded, and each byte is percent-encoded per IRI-to-URI conversion.
fn percent_encode_non_ascii(s: &str) -> String {
    let normalized: String = s.nfc().collect();
    let mut result = String::with_capacity(normalized.len());
    for byte in normalized.bytes() {
        if byte > 0x7E {
            result.push('%');
            result.push(char::from(HEX_UPPER[(byte >> 4) as usize]));
            result.push(char::from(HEX_UPPER[(byte & 0x0F) as usize]));
        } else {
            result.push(byte as char);
        }
    }
    result
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// XHTML void elements that may use self-closing syntax.
/// All other elements must use explicit close tags: `<element></element>`, not `<element/>`.
const XHTML_VOID_ELEMENTS: &[&str] = &[
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "isindex",
    "link", "meta", "param", "source", "track", "wbr",
];

fn materialize_xhtml_nonvoid_empty_elements(root: xot::Node, xot: &mut Xot) {
    let mut elements = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    for element in elements {
        if !needs_explicit_xhtml_end_tag(element, xot) {
            continue;
        }
        let empty = xot.new_text("");
        xot.append(element, empty).unwrap();
    }
}

fn materialize_html_foreign_attribute_namespaces(root: xot::Node, xot: &mut Xot) {
    let mut elements = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    for element in elements {
        let Some(name_id) = xot.node_name(element) else {
            continue;
        };
        let namespace = xot.namespace_str(xot.namespace_for_name(name_id));
        if namespace != "http://www.w3.org/2000/svg"
            && namespace != "http://www.w3.org/1998/Math/MathML"
        {
            continue;
        }

        let mut required = Vec::new();
        for attribute_name in xot.attributes(element).keys() {
            let attribute_namespace = xot.namespace_str(xot.namespace_for_name(attribute_name));
            if attribute_namespace.is_empty() {
                continue;
            }

            let Some(prefix) = in_scope_prefix_for_namespace(element, attribute_namespace, xot)
            else {
                continue;
            };

            if has_explicit_namespace_node(element, &prefix, attribute_namespace, xot) {
                continue;
            }

            if !required
                .iter()
                .any(|(current_prefix, current_namespace)| {
                    current_prefix == &prefix && current_namespace == attribute_namespace
                })
            {
                required.push((prefix, attribute_namespace.to_string()));
            }
        }

        for (prefix, namespace) in required {
            let prefix_id = xot.add_prefix(&prefix);
            let namespace_id = xot.add_namespace(&namespace);
            let namespace_node = xot.new_namespace_node(prefix_id, namespace_id);
            xot.any_append(element, namespace_node).unwrap();
        }
    }
}

fn in_scope_prefix_for_namespace(
    element: xot::Node,
    namespace: &str,
    xot: &Xot,
) -> Option<String> {
    xot.namespaces_in_scope(element).find_map(|(prefix, current_namespace)| {
        let prefix = xot.prefix_str(prefix);
        if prefix.is_empty() || xot.namespace_str(current_namespace) != namespace {
            return None;
        }
        Some(prefix.to_string())
    })
}

fn has_explicit_namespace_node(
    element: xot::Node,
    prefix: &str,
    namespace: &str,
    xot: &Xot,
) -> bool {
    xot.children(element).any(|child| match xot.value(child) {
        xot::Value::Namespace(node) => {
            xot.prefix_str(node.prefix()) == prefix && xot.namespace_str(node.namespace()) == namespace
        }
        _ => false,
    })
}

fn needs_explicit_xhtml_end_tag(element: xot::Node, xot: &Xot) -> bool {
    let Some(name_id) = xot.node_name(element) else {
        return false;
    };
    let (local_name, namespace) = xot.name_ns_str(name_id);
    let is_xhtml_element = namespace.is_empty() || namespace == "http://www.w3.org/1999/xhtml";
    if !is_xhtml_element || XHTML_VOID_ELEMENTS.contains(&local_name) {
        return false;
    }
    !xot
        .children(element)
        .any(|child| !matches!(xot.value(child), xot::Value::Namespace(_)))
}

fn remove_html5_doctype(serialized: &mut String) {
    if let Some(pos) = serialized.find("<!DOCTYPE html>") {
        let end = pos + "<!DOCTYPE html>".len();
        // Also remove a trailing newline if present
        let end = if serialized[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        serialized.replace_range(pos..end, "");
    }
}

/// A mapping from PUA placeholder characters to their replacement strings,
/// used to defer character map application until after XML serialization.
type PlaceholderMap = HashMap<char, String>;

/// Prepare a node for serialization by cloning it, fixing namespace
/// declarations, and replacing character-mapped characters with PUA
/// placeholders.  Returns the cloned node and a map from placeholder
/// codepoints to the intended replacement strings.
///
/// After serialization, call `apply_character_map_placeholders` on the
/// serialized string to replace the placeholders with the actual replacement
/// strings (which may contain XML markup characters like `&` or `<`).
/// Reverse placeholder map: PUA codepoint → original character.
/// Used to revert character map placeholders in URI attributes (HTML output).
type ReversePlaceholderMap = HashMap<char, char>;

fn map_serialization_node(
    node: xot::Node,
    character_maps: &HashMap<char, String>,
    xot: &mut Xot,
) -> (xot::Node, PlaceholderMap, ReversePlaceholderMap) {
    // The node from normalize() is already a fresh clone, so we can
    // apply namespace fixup directly without another clone.
    ensure_element_namespace_declarations(node, xot);
    normalize_namespace_declarations(node, xot);

    if character_maps.is_empty() {
        return (node, HashMap::default(), HashMap::default());
    }

    // Build a mapping: source_char → PUA codepoint, and PUA codepoint → replacement string
    let mut char_to_placeholder: HashMap<char, char> = HashMap::default();
    let mut placeholder_to_replacement: PlaceholderMap = HashMap::default();
    let mut placeholder_to_original: ReversePlaceholderMap = HashMap::default();
    // Use Unicode Private Use Area starting at U+E000
    let mut pua_offset = 0u32;
    for (source_char, replacement) in character_maps {
        let Some(placeholder) = char::from_u32(0xE000 + pua_offset) else {
            // PUA range exhausted; skip remaining character map entries
            break;
        };
        char_to_placeholder.insert(*source_char, placeholder);
        placeholder_to_replacement.insert(placeholder, replacement.clone());
        placeholder_to_original.insert(placeholder, *source_char);
        pua_offset += 1;
    }

    let mut text_nodes = Vec::new();
    if matches!(xot.value(node), xot::Value::Text(_)) {
        text_nodes.push(node);
    }
    text_nodes.extend(
        xot.descendants(node)
            .filter(|descendant| matches!(xot.value(*descendant), xot::Value::Text(_))),
    );

    for text_node in text_nodes {
        if let Some(text) = xot.text_mut(text_node) {
            let current = text.get().to_string();
            let mapped: String = current
                .chars()
                .map(|c| *char_to_placeholder.get(&c).unwrap_or(&c))
                .collect();
            if mapped != current {
                text.set(mapped);
            }
        }
    }

    // Also apply character maps to attribute values
    let elements: Vec<_> = if xot.is_element(node) {
        std::iter::once(node)
            .chain(xot.descendants(node))
            .filter(|n| xot.is_element(*n))
            .collect()
    } else {
        xot.descendants(node)
            .filter(|n| xot.is_element(*n))
            .collect()
    };

    for element in elements {
        let attr_updates: Vec<(xot::NameId, String)> = xot
            .attributes(element)
            .iter()
            .filter_map(|(name_id, value)| {
                let mapped: String = value
                    .chars()
                    .map(|c| *char_to_placeholder.get(&c).unwrap_or(&c))
                    .collect();
                if mapped != *value {
                    Some((name_id, mapped))
                } else {
                    None
                }
            })
            .collect();

        for (attr_name, mapped_value) in attr_updates {
            xot.set_attribute(element, attr_name, mapped_value);
        }
    }

    (node, placeholder_to_replacement, placeholder_to_original)
}

/// Replace PUA placeholder characters in the serialized output with the
/// actual character map replacement strings.
fn apply_character_map_placeholders(serialized: String, placeholders: &PlaceholderMap) -> String {
    if placeholders.is_empty() {
        return serialized;
    }
    let mut result = String::with_capacity(serialized.len());
    for c in serialized.chars() {
        if let Some(replacement) = placeholders.get(&c) {
            result.push_str(replacement);
        } else {
            result.push(c);
        }
    }
    result
}

/// Ensure that every element in the tree has a namespace declaration for its
/// own namespace.  Dynamically-created elements (e.g. `xsl:element` with an
/// AVT name) may lack a declaration because the namespace is only known at
/// runtime.  Without this fixup the serializer would fail with
/// `MissingPrefix`.
///
/// Uses the default namespace (`xmlns="..."`) for the declaration.
fn ensure_element_namespace_declarations(root: xot::Node, xot: &mut Xot) {
    let elements: Vec<_> = if xot.is_element(root) {
        std::iter::once(root)
            .chain(xot.descendants(root))
            .filter(|n| xot.is_element(*n))
            .collect()
    } else {
        xot.descendants(root)
            .filter(|n| xot.is_element(*n))
            .collect()
    };

    let empty_prefix = xot.empty_prefix();
    let no_namespace = xot.no_namespace();

    for element in elements {
        let Some(name_id) = xot.node_name(element) else {
            continue;
        };
        let ns_id = xot.namespace_for_name(name_id);
        if ns_id == no_namespace {
            continue;
        }
        // Element already has an in-scope prefix for this namespace
        // (including inherited declarations).
        if xot.prefix_for_namespace(element, ns_id).is_some() {
            continue;
        }
        // Add a default-namespace declaration: xmlns="..."
        xot.set_namespace(element, empty_prefix, ns_id);
    }
}

/// Normalize namespace declarations in the result tree before serialization.
///
/// Phase 1 – default namespace:
///   • Elements in no-namespace under a default namespace get an explicit
///     undeclaration (`xmlns=""`).
///   • Default namespace declarations that duplicate the parent's are removed.
///
/// Phase 2 – prefixed namespaces:
///   • Any `xmlns:prefix="uri"` declaration already inherited from an ancestor
///     is removed.  This eliminates the redundant declarations that arise when
///     e.g. `xsl:element` children each repeat the parent's namespace binding.
fn normalize_namespace_declarations(root: xot::Node, xot: &mut Xot) {
    let mut elements = Vec::new();
    if matches!(xot.value(root), xot::Value::Element(_)) {
        elements.push(root);
    }
    elements.extend(xot.descendants(root).filter(|node| xot.is_element(*node)));

    let empty_prefix = xot.add_prefix("");

    for element in elements {
        let parent = xot.parent(element).filter(|p| xot.is_element(*p));

        // What the parent's default namespace resolves to (walks ancestors).
        // None means no default namespace in scope (or undeclared via xmlns="").
        let parent_default_ns = parent.and_then(|p| xot.namespace_for_prefix(p, empty_prefix));

        let element_ns = xot
            .node_name(element)
            .map(|name| xot.namespace_for_name(name))
            .unwrap_or(xot.no_namespace());

        // Does this element carry its own xmlns="..." declaration?
        let explicit_default_ns = xot
            .namespace_declarations(element)
            .into_iter()
            .find(|(prefix, _)| *prefix == empty_prefix)
            .map(|(_, ns)| ns);

        // -- Phase 1: default namespace normalization --

        if element_ns == xot.no_namespace() && parent_default_ns.is_some() {
            // Element in no-namespace under a default namespace: must
            // undeclare with xmlns="" so it doesn't inherit the parent's.
            if explicit_default_ns != Some(xot.no_namespace()) {
                // Replace any existing default declaration with xmlns=""
                xot.set_namespace(element, empty_prefix, xot.no_namespace());
            }
        } else if let Some(ns) = explicit_default_ns {
            // Explicit default namespace identical to parent's: redundant.
            if Some(ns) == parent_default_ns
                || (ns == xot.no_namespace() && parent_default_ns.is_none())
            {
                xot.remove_namespace(element, empty_prefix);
            }
        }

        // -- Phase 2: prefixed namespace deduplication --
        // A declaration is redundant when an ancestor already binds the
        // same prefix to the same namespace URI.
        if let Some(parent) = parent {
            let redundant: Vec<_> = xot
                .namespace_declarations(element)
                .into_iter()
                .filter(|(prefix, ns)| {
                    *prefix != empty_prefix
                        && xot.namespace_for_prefix(parent, *prefix) == Some(*ns)
                })
                .map(|(prefix, _)| prefix)
                .collect();

            for prefix in redundant {
                xot.remove_namespace(element, prefix);
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

fn apply_normalization_form(
    serialized: String,
    parameters: &SerializationParameters,
) -> error::Result<String> {
    match parameters.normalization_form.as_deref() {
        Some("NFC") => Ok(serialized.nfc().collect()),
        Some("NFD") => Ok(serialized.nfd().collect()),
        Some("NFKC") => Ok(serialized.nfkc().collect()),
        Some("NFKD") => Ok(serialized.nfkd().collect()),
        Some("none") => Ok(serialized),
        Some("fully-normalized") => Err(error::Error::SESU0011),
        Some(_) => Err(error::Error::SESU0011),
        None => Ok(serialized),
    }
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
    serialize_json_sequence(arg, &parameters, xot)
}

fn serialize_json_sequence(
    arg: &Sequence,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match arg {
        Sequence::One(item) => serialize_json_item(item.item(), parameters, xot),
        Sequence::Empty(_) => Ok("null".to_string()),
        Sequence::Many(_) | Sequence::Range(_) => Err(error::Error::SERE0023),
    }
}

fn serialize_json_item(
    item: &Item,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    match item {
        Item::Atomic(atomic) => serialize_json_atomic(atomic, parameters),
        Item::Node(node) => serialize_json_node(*node, parameters, xot),
        Item::Function(function) => serialize_json_function(function, parameters, xot),
    }
}

fn serialize_json_atomic(
    atomic: &atomic::Atomic,
    parameters: &SerializationParameters,
) -> Result<String, error::Error> {
    match atomic {
        atomic::Atomic::Float(float) => {
            let f = float.into_inner();
            if f.is_infinite() || f.is_nan() {
                return Err(error::Error::SERE0020);
            }
            Ok(json::JsonValue::Number(f.into()).dump())
        }
        atomic::Atomic::Double(double) => {
            let d = double.into_inner();
            if d.is_infinite() || d.is_nan() {
                return Err(error::Error::SERE0020);
            }
            Ok(json::JsonValue::Number(d.into()).dump())
        }
        atomic::Atomic::Decimal(decimal) => {
            let d: f64 = (*decimal.as_ref())
                .try_into()
                .map_err(|_| error::Error::SERE0020)?;
            Ok(json::JsonValue::Number(d.into()).dump())
        }
        atomic::Atomic::Integer(_t, integer) => {
            let i: f64 = integer.to_f64();
            Ok(json::JsonValue::Number(i.into()).dump())
        }
        atomic::Atomic::Boolean(b) => Ok(json::JsonValue::Boolean(*b).dump()),
        _ => {
            let s = atomic.string_value();
            Ok(serialize_json_string(s, parameters))
        }
    }
}

fn serialize_json_string(s: String, parameters: &SerializationParameters) -> String {
    // TODO: normalization-form

    escape_json_string(&s, &parameters.use_character_maps)
}

fn escape_json_string(s: &str, character_maps: &HashMap<char, String>) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('"');
    for ch in s.chars() {
        if let Some(replacement) = character_maps.get(&ch) {
            escaped.push_str(replacement);
            continue;
        }

        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '/' => escaped.push_str("\\/"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch <= '\u{1F}' => {
                use std::fmt::Write;

                write!(&mut escaped, "\\u{:04X}", ch as u32).unwrap();
            }
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

fn serialize_json_node(
    node: xot::Node,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
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
) -> Result<String, error::Error> {
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
) -> Result<String, error::Error> {
    let mut result = Vec::with_capacity(array.len());
    for entry in array.iter() {
        let serialized = serialize_json_sequence(entry, parameters, xot)?;
        result.push(serialized);
    }
    Ok(format!("[{}]", result.join(",")))
}

fn serialize_json_map(
    map: &function::Map,
    parameters: &SerializationParameters,
    xot: &mut Xot,
) -> Result<String, error::Error> {
    let mut entries = Vec::with_capacity(map.len());
    let mut seen_names = HashSet::new();

    for (_map_key, (key, value)) in map.full_entries() {
        let key_s = key.string_value();
        if !parameters.allow_duplicate_names && !seen_names.insert(key_s.clone()) {
            return Err(error::Error::SERE0022);
        }

        let value = serialize_json_sequence(value, parameters, xot)?;
        entries.push((key_s, value));
    }

    entries.sort_by(|(left, _), (right, _)| left.cmp(right));

    Ok(format!(
        "{{{}}}",
        entries
            .into_iter()
            .map(|(key, value)| format!("{}:{}", serialize_json_string(key, parameters), value))
            .collect::<Vec<_>>()
            .join(",")
    ))
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

/// Build DocType from serialization parameters.
/// Per XSLT spec erratum E31, empty doctype-system and doctype-public suppress DOCTYPE.
/// Per XSLT spec: if doctype-system is absent, doctype-public is also treated as absent.
fn build_xml_doctype(
    parameters: &SerializationParameters,
) -> Option<xot::output::xml::DocType> {
    match (
        parameters.doctype_public.as_deref(),
        parameters.doctype_system.as_deref(),
    ) {
        // Both empty — suppress DOCTYPE entirely (erratum E31)
        (Some(""), Some("")) => None,
        (Some(public), Some(system)) => Some(xot::output::xml::DocType::Public {
            public: public.to_string(),
            system: system.to_string(),
        }),
        (None, Some(system)) => Some(xot::output::xml::DocType::System {
            system: system.to_string(),
        }),
        // doctype-public without doctype-system: treat public as absent
        (Some(_), None) | (None, None) => None,
    }
}

/// Reposition DOCTYPE declaration to immediately before the first element.
/// Per spec, the DOCTYPE must appear immediately before the first element in the
/// serialized output, not immediately after the XML declaration.
fn reposition_doctype_before_first_element(serialized: String) -> String {
    // Find DOCTYPE in the output
    let Some(doctype_start) = serialized.find("<!DOCTYPE") else {
        return serialized;
    };
    let Some(doctype_end_rel) = serialized[doctype_start..].find('>') else {
        return serialized;
    };
    let doctype_end = doctype_start + doctype_end_rel + 1;

    // Find first element position
    let Some(element_pos) = find_first_element_position(&serialized) else {
        return serialized;
    };

    // If DOCTYPE is already immediately before the first element, nothing to do
    if doctype_end == element_pos || serialized[doctype_end..element_pos].trim().is_empty() {
        return serialized;
    }

    // Extract the DOCTYPE string (include trailing newline if present)
    let mut doctype_str = serialized[doctype_start..doctype_end].to_string();
    let after_doctype = if serialized[doctype_end..].starts_with('\n') {
        doctype_end + 1
    } else {
        doctype_end
    };

    // Build new string: everything before DOCTYPE + everything between DOCTYPE and element + DOCTYPE + element onwards
    let mut result = String::with_capacity(serialized.len());
    result.push_str(&serialized[..doctype_start]);
    result.push_str(&serialized[after_doctype..element_pos]);
    doctype_str.push('\n');
    result.push_str(&doctype_str);
    result.push_str(&serialized[element_pos..]);
    result
}

fn strip_post_declaration_newline(s: &str) -> String {
    if let Some(pos) = s.find("?>") {
        let after = pos + 2;
        if s[after..].starts_with('\n') {
            let mut result = String::with_capacity(s.len() - 1);
            result.push_str(&s[..after]);
            result.push_str(&s[after + 1..]);
            return result;
        }
    }
    s.to_string()
}

/// XHTML compatibility: insert a space before `/>` in self-closing elements.
/// E.g. `<br/>` becomes `<br />`, `<img src="x"/>` becomes `<img src="x" />`.
fn add_xhtml_space_before_self_closing(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + 32);
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
            result.push(bytes[i]);
            i += 1;
        } else if !in_quotes && bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'>'
        {
            if i > 0 && bytes[i - 1] != b' ' {
                result.push(b' ');
            }
            result.push(b'/');
            result.push(b'>');
            i += 2;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    // Safe: input was valid UTF-8 and we only inserted ASCII bytes
    String::from_utf8(result).expect("add_xhtml_space_before_self_closing: invalid UTF-8")
}

/// Escape C1 control characters (U+0080..U+009F) as decimal numeric character
/// references (e.g. `&#150;`). These codepoints are not representable in HTML
/// and must be escaped in XHTML for interoperability.
fn escape_c1_control_characters(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ('\u{0080}'..='\u{009F}').contains(&ch) {
            result.push_str(&format!("&#{};", ch as u32));
        } else {
            result.push(ch);
        }
    }
    result
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
    fn test_serialize_json_string_escapes_solidus() {
        let params = SerializationParameters::new();
        assert_eq!(serialize_json_string("a/b".to_string(), &params), "\"a\\/b\"");
    }

    #[test]
    fn test_serialize_json_string_honors_character_maps() {
        let params = SerializationParameters {
            use_character_maps: HashMap::from_iter([('/' , "/".to_string())]),
            ..SerializationParameters::new()
        };
        assert_eq!(serialize_json_string("a/b".to_string(), &params), "\"a/b\"");
    }

    #[test]
    fn test_serialize_json_map_duplicate_names_allowed() {
        let map = Map::new(vec![
            (
                atomic::Atomic::Time(
                    atomic::NaiveTimeWithOffset::new(
                        chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
                        Some(chrono::FixedOffset::east_opt(0).unwrap()),
                    )
                    .into(),
                ),
                sequence::Sequence::from(vec![atomic::Atomic::from("alpha")]),
            ),
            (
                atomic::Atomic::from("23:00:00Z"),
                sequence::Sequence::from(vec![atomic::Atomic::from("beta")]),
            ),
        ])
        .unwrap();
        let mut xot = Xot::new();
        let params = SerializationParameters {
            method: QNameOrString::String("json".to_string()),
            allow_duplicate_names: true,
            ..SerializationParameters::new()
        };

        let serialized = serialize_json_map(&map, &params, &mut xot).unwrap();
        assert!(
            serialized == "{\"23:00:00Z\":\"alpha\",\"23:00:00Z\":\"beta\"}"
                || serialized
                    == "{\"23:00:00Z\":\"beta\",\"23:00:00Z\":\"alpha\"}"
        );
    }

    #[test]
    fn test_serialize_json_map_duplicate_names_forbidden() {
        let map = Map::new(vec![
            (
                atomic::Atomic::Time(
                    atomic::NaiveTimeWithOffset::new(
                        chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
                        Some(chrono::FixedOffset::east_opt(0).unwrap()),
                    )
                    .into(),
                ),
                sequence::Sequence::from(vec![atomic::Atomic::from("alpha")]),
            ),
            (
                atomic::Atomic::from("23:00:00Z"),
                sequence::Sequence::from(vec![atomic::Atomic::from("beta")]),
            ),
        ])
        .unwrap();
        let mut xot = Xot::new();
        let params = SerializationParameters {
            method: QNameOrString::String("json".to_string()),
            allow_duplicate_names: false,
            ..SerializationParameters::new()
        };

        assert_eq!(
            serialize_json_map(&map, &params, &mut xot),
            Err(error::Error::SERE0022)
        );
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

    #[test]
    fn test_materialize_xhtml_nonvoid_empty_elements_adds_empty_text() {
        let mut xot = Xot::new();
        let xhtml_ns = xot.add_namespace("http://www.w3.org/1999/xhtml");
        let html_name = xot.add_name_ns("html", xhtml_ns);
        let title_name = xot.add_name_ns("title", xhtml_ns);
        let html = xot.new_element(html_name);
        let title = xot.new_element(title_name);
        xot.append(html, title).unwrap();

        materialize_xhtml_nonvoid_empty_elements(html, &mut xot);

        let children: Vec<_> = xot.children(title).collect();
        assert_eq!(children.len(), 1);
        assert!(matches!(xot.value(children[0]), xot::Value::Text(_)));
    }

    #[test]
    fn test_materialize_xhtml_nonvoid_empty_elements_keeps_void_empty() {
        let mut xot = Xot::new();
        let html_name = xot.add_name("html");
        let br_name = xot.add_name("br");
        let html = xot.new_element(html_name);
        let br = xot.new_element(br_name);
        xot.append(html, br).unwrap();

        materialize_xhtml_nonvoid_empty_elements(html, &mut xot);

        assert_eq!(xot.children(br).count(), 0);
    }
}
