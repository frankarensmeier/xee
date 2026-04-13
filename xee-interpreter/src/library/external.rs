use std::fs;

use iri_string::types::{IriReferenceStr, IriString};
use xee_xpath_macros::xpath_fn;
use xot::xmlname::OwnedName;

use crate::{
    context::DynamicContext, error, function::StaticFunctionDescription, interpreter::Interpreter,
    sequence::Sequence, wrap_xpath_fn,
};

#[xpath_fn("fn:doc($uri as xs:string?) as document-node()?")]
fn doc(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    uri: Option<&str>,
) -> error::Result<Option<xot::Node>> {
    if let Some(uri) = uri {
        document_node(context, interpreter, uri)
    } else {
        Ok(None)
    }
}

#[xpath_fn("fn:document($uri as xs:string?) as document-node()?")]
fn document(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    uri: Option<&str>,
) -> error::Result<Option<xot::Node>> {
    doc(context, interpreter, uri)
}

#[xpath_fn("fn:doc-available($uri as xs:string?) as xs:boolean")]
fn doc_available(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    uri: Option<&str>,
) -> bool {
    if let Some(uri) = uri {
        document_node(context, interpreter, uri).is_ok()
    } else {
        false
    }
}

fn document_node(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    uri: &str,
) -> error::Result<Option<xot::Node>> {
    let iri_reference: &IriReferenceStr = uri.try_into().map_err(|_| error::Error::FODC0005)?;
    let uri = absolute_uri(context, iri_reference)?;

    // first check whether a document is there at all, if so, return it
    let documents = context.documents();
    if let Some(document) = documents.borrow().get_by_uri(&uri) {
        return Ok(Some(document.root()));
    }

    load_document(context, interpreter, &uri)
}

fn load_document(
    context: &DynamicContext,
    interpreter: &mut Interpreter,
    uri: &IriString,
) -> error::Result<Option<xot::Node>> {
    let url = url::Url::parse(uri.as_str()).map_err(|_| error::Error::FODC0005)?;
    let path = url
        .to_file_path()
        .map_err(|_| resource_error(uri.as_str().to_string()))?;
    let xml = fs::read_to_string(&path).map_err(|_| resource_error(uri.as_str().to_string()))?;

    let documents = context.documents();
    let handle = documents
        .borrow_mut()
        .add_string(interpreter.xot_mut(), Some(uri.as_ref()), &xml)
        .map_err(|_| resource_error(uri.as_str().to_string()))?;
    let document = documents
        .borrow()
        .get_node_by_handle(handle)
        .ok_or_else(|| resource_error(uri.as_str().to_string()))?;
    Ok(Some(document))
}

fn resource_error(uri: String) -> error::Error {
    error::Error::Application(Box::new(error::ApplicationError::new(
        OwnedName::new(
            "FODC0002".to_string(),
            "http://www.w3.org/2005/xqt-errors".to_string(),
            "err".to_string(),
        ),
        format!("Error retrieving resource: {uri}"),
    )))
}

#[xpath_fn("fn:collection() as item()*")]
fn collection(context: &DynamicContext) -> error::Result<Sequence> {
    if let Some(collection) = context.default_collection() {
        Ok(collection.clone())
    } else {
        Err(error::Error::FODC0002)
    }
}

#[xpath_fn("fn:collection($uri as xs:string?) as item()*")]
fn collection_by_uri(context: &DynamicContext, uri: Option<&str>) -> error::Result<Sequence> {
    if let Some(uri) = uri {
        let iri_reference: &IriReferenceStr = uri.try_into().map_err(|_| error::Error::FODC0004)?;
        let uri = absolute_uri(context, iri_reference)?;
        if let Some(collection) = context.collection(&uri) {
            Ok(collection.clone())
        } else {
            Err(error::Error::FODC0002)
        }
    } else if let Some(collection) = context.default_collection() {
        Ok(collection.clone())
    } else {
        Err(error::Error::FODC0002)
    }
}

#[xpath_fn("fn:uri-collection() as xs:anyURI*")]
fn uri_collection(context: &DynamicContext) -> error::Result<Sequence> {
    if let Some(collection) = context.default_uri_collection() {
        Ok(collection.clone())
    } else {
        Err(error::Error::FODC0002)
    }
}

#[xpath_fn("fn:uri-collection($uri as xs:string?) as xs:anyURI*")]
fn uri_collection_by_uri(context: &DynamicContext, uri: Option<&str>) -> error::Result<Sequence> {
    if let Some(uri) = uri {
        let iri_reference: &IriReferenceStr = uri.try_into().map_err(|_| error::Error::FODC0004)?;
        let uri = absolute_uri(context, iri_reference)?;
        if let Some(collection) = context.uri_collection(&uri) {
            Ok(collection.clone())
        } else {
            Err(error::Error::FODC0002)
        }
    } else if let Some(collection) = context.default_uri_collection() {
        Ok(collection.clone())
    } else {
        Err(error::Error::FODC0002)
    }
}

fn absolute_uri(context: &DynamicContext, uri: &IriReferenceStr) -> error::Result<IriString> {
    let uri: IriString = match uri.to_iri() {
        Ok(iri) => iri.into(),
        Err(relative_iri) => {
            let base = context.static_context().static_base_uri();
            if let Some(base) = base {
                relative_iri.resolve_against(base).into()
            } else {
                return Err(error::Error::FODC0002);
            }
        }
    };
    Ok(uri)
}

#[xpath_fn("fn:environment-variable($name as xs:string) as xs:string?")]
fn environment_variable(context: &DynamicContext, name: &str) -> Option<String> {
    context.environment_variable(name).map(|s| s.to_string())
}

#[xpath_fn("fn:available-environment-variables() as xs:string*")]
fn available_environment_variables(context: &DynamicContext) -> Vec<String> {
    context
        .environment_variable_names()
        .map(|s| s.to_string())
        .collect()
}

fn read_text_resource(
    context: &DynamicContext,
    href: &str,
    encoding: Option<&str>,
) -> error::Result<String> {
    // Fragment identifiers are not allowed
    if href.contains('#') {
        return Err(error::Error::FOUT1170);
    }

    let iri_reference: &IriReferenceStr = href.try_into().map_err(|_| error::Error::FOUT1170)?;
    let uri = absolute_uri(context, iri_reference).map_err(|_| error::Error::FOUT1170)?;
    let url = url::Url::parse(uri.as_str()).map_err(|_| error::Error::FOUT1170)?;
    let path = url.to_file_path().map_err(|_| error::Error::FOUT1170)?;
    let bytes = fs::read(&path).map_err(|_| error::Error::FOUT1170)?;

    let encoding_label = encoding.unwrap_or("utf-8");
    let enc = encoding_rs::Encoding::for_label(encoding_label.as_bytes())
        .ok_or(error::Error::FOUT1190)?;

    if enc == encoding_rs::UTF_8 {
        String::from_utf8(bytes).map_err(|_| error::Error::FOUT1190)
    } else {
        let (result, _, had_errors) = enc.decode(&bytes);
        if had_errors {
            return Err(error::Error::FOUT1190);
        }
        Ok(result.into_owned())
    }
}

#[xpath_fn("fn:unparsed-text($href as xs:string?) as xs:string?")]
fn unparsed_text1(context: &DynamicContext, href: Option<&str>) -> error::Result<Option<String>> {
    match href {
        Some(href) => read_text_resource(context, href, None).map(Some),
        None => Ok(None),
    }
}

#[xpath_fn("fn:unparsed-text($href as xs:string?, $encoding as xs:string) as xs:string?")]
fn unparsed_text2(
    context: &DynamicContext,
    href: Option<&str>,
    encoding: &str,
) -> error::Result<Option<String>> {
    match href {
        Some(href) => read_text_resource(context, href, Some(encoding)).map(Some),
        None => Ok(None),
    }
}

#[xpath_fn("fn:unparsed-text-available($href as xs:string?) as xs:boolean")]
fn unparsed_text_available1(context: &DynamicContext, href: Option<&str>) -> bool {
    match href {
        Some(href) => read_text_resource(context, href, None).is_ok(),
        None => false,
    }
}

#[xpath_fn("fn:unparsed-text-available($href as xs:string?, $encoding as xs:string) as xs:boolean")]
fn unparsed_text_available2(context: &DynamicContext, href: Option<&str>, encoding: &str) -> bool {
    match href {
        Some(href) => read_text_resource(context, href, Some(encoding)).is_ok(),
        None => false,
    }
}

#[xpath_fn("fn:unparsed-text-lines($href as xs:string?) as xs:string*")]
fn unparsed_text_lines1(
    context: &DynamicContext,
    href: Option<&str>,
) -> error::Result<Vec<String>> {
    match href {
        Some(href) => {
            let text = read_text_resource(context, href, None)?;
            Ok(text.lines().map(|s| s.to_string()).collect())
        }
        None => Ok(vec![]),
    }
}

#[xpath_fn("fn:unparsed-text-lines($href as xs:string?, $encoding as xs:string) as xs:string*")]
fn unparsed_text_lines2(
    context: &DynamicContext,
    href: Option<&str>,
    encoding: &str,
) -> error::Result<Vec<String>> {
    match href {
        Some(href) => {
            let text = read_text_resource(context, href, Some(encoding))?;
            Ok(text.lines().map(|s| s.to_string()).collect())
        }
        None => Ok(vec![]),
    }
}

// https://www.w3.org/TR/xpath-functions-31/#fns-on-docs
pub(crate) fn static_function_descriptions() -> Vec<StaticFunctionDescription> {
    vec![
        wrap_xpath_fn!(doc),
        wrap_xpath_fn!(document),
        wrap_xpath_fn!(doc_available),
        wrap_xpath_fn!(collection),
        wrap_xpath_fn!(collection_by_uri),
        wrap_xpath_fn!(uri_collection),
        wrap_xpath_fn!(uri_collection_by_uri),
        wrap_xpath_fn!(environment_variable),
        wrap_xpath_fn!(available_environment_variables),
        wrap_xpath_fn!(unparsed_text1),
        wrap_xpath_fn!(unparsed_text2),
        wrap_xpath_fn!(unparsed_text_available1),
        wrap_xpath_fn!(unparsed_text_available2),
        wrap_xpath_fn!(unparsed_text_lines1),
        wrap_xpath_fn!(unparsed_text_lines2),
    ]
}
