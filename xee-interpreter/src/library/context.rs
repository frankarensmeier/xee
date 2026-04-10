// https://www.w3.org/TR/2017/REC-xpath-functions-31-20170321/#context

use ibig::IBig;
use xee_name::{Name, Namespaces, FN_NAMESPACE};
use xee_xpath_ast::parse_name;
use xee_xpath_ast::ast;
use xee_xpath_macros::xpath_fn;
use xot::xmlname::NameStrInfo;

use crate::atomic;
use crate::atomic::NaiveDateWithOffset;
use crate::atomic::NaiveTimeWithOffset;
use crate::context::DynamicContext;
use crate::error;
use crate::function::FunctionKind;
use crate::function::StaticFunctionDescription;
use crate::interpreter;
use crate::sequence;
use crate::wrap_xpath_fn;

use super::datetime::offset_to_duration;

const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";

const XSLT_ELEMENT_NAMES: &[&str] = &[
    "accept",
    "accumulator",
    "accumulator-rule",
    "analyze-string",
    "apply-imports",
    "apply-templates",
    "assert",
    "attribute",
    "attribute-set",
    "break",
    "call-template",
    "catch",
    "character-map",
    "choose",
    "comment",
    "context-item",
    "copy",
    "copy-of",
    "decimal-format",
    "document",
    "element",
    "evaluate",
    "expose",
    "fallback",
    "for-each",
    "for-each-group",
    "fork",
    "function",
    "global-context-item",
    "if",
    "import",
    "import-schema",
    "include",
    "iterate",
    "key",
    "map",
    "map-entry",
    "matching-substring",
    "merge",
    "merge-action",
    "merge-key",
    "merge-source",
    "message",
    "mode",
    "namespace",
    "namespace-alias",
    "next-iteration",
    "next-match",
    "non-matching-substring",
    "number",
    "on-completion",
    "on-empty",
    "on-non-empty",
    "otherwise",
    "output",
    "output-character",
    "override",
    "package",
    "param",
    "perform-sort",
    "preserve-space",
    "processing-instruction",
    "result-document",
    "sequence",
    "sort",
    "source-document",
    "strip-space",
    "stylesheet",
    "template",
    "text",
    "transform",
    "try",
    "use-package",
    "value-of",
    "variable",
    "when",
    "where-populated",
    "with-param",
];

fn bound_position(
    _context: &DynamicContext,
    _interpreter: &mut interpreter::Interpreter,
    arguments: &[sequence::Sequence],
) -> error::Result<sequence::Sequence> {
    // position should be the context value
    Ok(arguments[0].clone())
}

fn bound_last(
    _context: &DynamicContext,
    _interpreter: &mut interpreter::Interpreter,
    arguments: &[sequence::Sequence],
) -> error::Result<sequence::Sequence> {
    // size should be the context value
    Ok(arguments[0].clone())
}

#[xpath_fn("fn:current-dateTime() as xs:dateTimeStamp")]
fn current_date_time(context: &DynamicContext) -> chrono::DateTime<chrono::offset::FixedOffset> {
    context.current_datetime()
}

#[xpath_fn("fn:current-date() as xs:date")]
fn current_date(context: &DynamicContext) -> NaiveDateWithOffset {
    NaiveDateWithOffset {
        date: context.current_datetime().naive_local().date(),
        offset: Some(context.implicit_timezone()),
    }
}

#[xpath_fn("fn:current-time() as xs:time")]
fn current_time(context: &DynamicContext) -> NaiveTimeWithOffset {
    NaiveTimeWithOffset {
        time: context.current_datetime().time(),
        offset: Some(context.implicit_timezone()),
    }
}

#[xpath_fn("fn:implicit-timezone() as xs:dayTimeDuration")]
fn implicit_timezone(context: &DynamicContext) -> chrono::Duration {
    offset_to_duration(context.implicit_timezone())
}

#[xpath_fn("fn:default-collation() as xs:string")]
fn default_collation(context: &DynamicContext) -> String {
    context.static_context().default_collation_uri().to_string()
}

#[xpath_fn("fn:static-base-uri() as xs:anyURI?")]
fn static_base_uri(context: &DynamicContext) -> Option<atomic::Atomic> {
    context
        .static_context()
        .static_base_uri()
        .map(|uri| atomic::Atomic::String(atomic::StringType::AnyURI, uri.to_string().into()))
}

#[xpath_fn("fn:system-property($property_name as xs:string) as xs:string")]
fn system_property(context: &DynamicContext, property_name: &str) -> String {
    resolve_system_property(context, property_name).unwrap_or_default()
}

#[xpath_fn("fn:function-available($function_name as xs:string) as xs:boolean")]
fn function_available(context: &DynamicContext, function_name: &str) -> bool {
    let Some(name) = resolve_function_name(context, function_name) else {
        return false;
    };
    is_function_available(context, &name, None)
}

#[xpath_fn("fn:function-available($function_name as xs:string, $arity as xs:integer) as xs:boolean")]
fn function_available_with_arity(
    context: &DynamicContext,
    function_name: &str,
    arity: IBig,
) -> bool {
    let Ok(arity) = u8::try_from(arity) else {
        return false;
    };
    let Some(name) = resolve_function_name(context, function_name) else {
        return false;
    };
    is_function_available(context, &name, Some(arity))
}

#[xpath_fn("fn:element-available($element_name as xs:string) as xs:boolean")]
fn element_available(context: &DynamicContext, element_name: &str) -> bool {
    let Some(name) = resolve_element_name(context, element_name) else {
        return false;
    };
    name.namespace() == XSLT_NAMESPACE && XSLT_ELEMENT_NAMES.contains(&name.local_name())
}

fn resolve_function_name(context: &DynamicContext, lexical_name: &str) -> Option<Name> {
    if lexical_name.is_empty() {
        return None;
    }

    if lexical_name.contains(':') || lexical_name.starts_with("Q{") {
        parse_name(lexical_name, context.static_context().namespaces())
            .ok()
            .map(|name| {
                Name::new(
                    name.value.local_name().to_string(),
                    name.value.namespace().to_string(),
                    String::new(),
                )
            })
    } else {
        Some(Name::new(
            lexical_name.to_string(),
            context
                .static_context()
                .namespaces()
                .default_function_namespace
                .to_string(),
            String::new(),
        ))
    }
}

fn resolve_element_name(context: &DynamicContext, lexical_name: &str) -> Option<Name> {
    if lexical_name.is_empty() {
        return None;
    }

    parse_name(lexical_name, context.static_context().namespaces())
        .ok()
        .map(|name| {
            Name::new(
                name.value.local_name().to_string(),
                name.value.namespace().to_string(),
                String::new(),
            )
        })
}

fn is_function_available(
    context: &DynamicContext,
    name: &Name,
    arity: Option<u8>,
) -> bool {
    if context.static_context().is_function_disabled(name) {
        return false;
    }

    match arity {
        Some(arity) => {
            context.static_context().function_id_by_name(name, arity).is_some()
                || is_manual_xslt_function_available(name, arity)
        }
        None => {
            context.static_context().has_function_name(name)
                || [0_u8, 1, 2, 3]
                    .into_iter()
                    .any(|arity| is_manual_xslt_function_available(name, arity))
        }
    }
}

fn is_manual_xslt_function_available(name: &Name, arity: u8) -> bool {
    if name.namespace() != FN_NAMESPACE {
        return false;
    }

    match name.local_name() {
        "current" => arity == 0,
        "key" => matches!(arity, 2 | 3),
        "unparsed-entity-uri" | "unparsed-entity-public-id" => arity == 1,
        _ => false,
    }
}

fn resolve_system_property(context: &DynamicContext, property_name: &str) -> Option<String> {
    if !property_name.contains(':') && !property_name.starts_with("Q{") {
        return None;
    }

    let name = parse_name(property_name, context.static_context().namespaces())
        .ok()?
        .value;

    if name.namespace() != XSLT_NAMESPACE {
        return None;
    }

    let xslt_version = format!(
        "{}.0",
        context
            .static_context()
            .processor_xslt_version()
            .or(context.static_context().stylesheet_xslt_version())
            .unwrap_or(3)
    );
    let xpath_version = match context.static_context().processor_xpath_version().unwrap_or(31) {
        20 => "2.0",
        30 => "3.0",
        31 => "3.1",
        version => return Some(format!("{version}.0")),
    };

    Some(match name.local_name() {
        "version" => xslt_version,
        "vendor" => "Xee".to_string(),
        "vendor-url" => "https://github.com/frankarensmeier/xee".to_string(),
        "product-name" => "Xee".to_string(),
        "product-version" => xslt_version,
        "is-schema-aware" => "no".to_string(),
        "supports-serialization" => "yes".to_string(),
        "supports-backwards-compatibility" => "yes".to_string(),
        "supports-dynamic-evaluation" => "no".to_string(),
        "supports-streaming" => "no".to_string(),
        "supports-higher-order-functions" => "yes".to_string(),
        "xpath-version" => xpath_version.to_string(),
        "xsd-version" => "1.1".to_string(),
        _ => return None,
    })
}

pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        StaticFunctionDescription {
            name: Name::new(
                "position".to_string(),
                FN_NAMESPACE.to_string(),
                String::new(),
            ),
            signature: ast::Signature::parse("fn:position() as xs:integer", &Namespaces::default())
                .unwrap()
                .into(),
            function_kind: Some(FunctionKind::Position),
            func: bound_position,
        },
        StaticFunctionDescription {
            name: Name::new("last".to_string(), FN_NAMESPACE.to_string(), String::new()),
            signature: ast::Signature::parse("fn:last() as xs:integer", &Namespaces::default())
                .unwrap()
                .into(),
            function_kind: Some(FunctionKind::Size),
            func: bound_last,
        },
        wrap_xpath_fn!(current_date_time),
        wrap_xpath_fn!(current_date),
        wrap_xpath_fn!(current_time),
        wrap_xpath_fn!(implicit_timezone),
        wrap_xpath_fn!(default_collation),
        wrap_xpath_fn!(static_base_uri),
        wrap_xpath_fn!(system_property),
        wrap_xpath_fn!(function_available),
        wrap_xpath_fn!(function_available_with_arity),
        wrap_xpath_fn!(element_available),
    ]
}
