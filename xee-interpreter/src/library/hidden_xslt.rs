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
use crate::function::StaticFunctionDescription;
use crate::interpreter::Interpreter;
use crate::sequence;
use crate::wrap_xpath_fn;

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
            .map_err(|_| error::Error::FOXT0002)?;
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

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(simple_content),
        wrap_xpath_fn!(group_by_first),
        wrap_xpath_fn!(resolve_xslt_qname),
        wrap_xpath_fn!(store_result_document),
        wrap_xpath_fn!(store_principal_result_document),
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
