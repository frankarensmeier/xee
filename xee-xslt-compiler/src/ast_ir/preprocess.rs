//! Stylesheet preprocessing: loading, import/include resolution, and namespace collection.
//!
//! This module handles the first phase of XSLT compilation: parsing the entry stylesheet,
//! recursively loading imported/included modules, flattening them into a precedence-ordered
//! list of declarations, and collecting namespace bindings for QName resolution.

use std::path::PathBuf;
use xee_interpreter::{
    context::{StaticContext, Variables as StaticVariables},
    error,
};
use xee_xslt_ast::{
    ast,
    error::{AttributeError, ElementError},
    parse_transform_with_static_variables_and_base_dir,
    parse_transform_with_static_variables_and_location,
};
use xee_name::Namespaces;
use xot::Xot;
use xot::xmlname::OwnedName;


use super::{
    PreprocessedDeclaration, PreprocessedModule,
};

/// Parse the entry stylesheet, load imported/included modules, flatten them
/// into a precedence-ordered declaration list, and resolve the initial mode.
pub(super) fn preprocess_stylesheet(
    static_context: StaticContext,
    xslt: &str,
    base_dir: Option<std::path::PathBuf>,
    initial_mode: Option<String>,
) -> error::SpannedResult<(
    Vec<PreprocessedDeclaration>,
    StaticContext,
    ast::ApplyTemplatesModeValue,
)> {
    let mut static_context =
        augment_static_context_with_stylesheet_namespaces(static_context, xslt);
    let stylesheet_version = detect_stylesheet_version(xslt);
    static_context.set_stylesheet_xslt_version(stylesheet_version);
    if static_context.processor_xslt_version().is_none() {
        static_context.set_processor_xslt_version(Some(3));
    }
    if static_context.processor_xpath_version().is_none() {
        static_context.set_processor_xpath_version(Some(31));
    }
    let transform = if static_context.static_base_uri().is_some() {
        parse_transform_with_static_variables_and_location(
            xslt,
            StaticVariables::new(),
            static_context.processor_xslt_version(),
            static_context.processor_xpath_version(),
            base_dir.clone(),
            static_context.static_base_uri().map(ToString::to_string),
            false,
        )
    } else {
        parse_transform_with_static_variables_and_base_dir(
            xslt,
            StaticVariables::new(),
            static_context.processor_xslt_version(),
            static_context.processor_xpath_version(),
            base_dir.clone(),
        )
    };
    // TODO: better error handling
    let (transform, static_variables) = match transform {
        Ok(transform) => transform,
        Err(e) => {
            return Err(map_parse_error(xslt, e));
        }
    };

    let default_mode = transform.default_mode.clone();

    // Process xsl:import and xsl:include directives
    let declarations = process_imports_and_includes(
        transform.declarations,
        base_dir,
        stylesheet_version,
        static_variables,
        static_context.static_base_uri().map(|u| u.to_string()),
    )?;

    // Add namespace bindings from all included/imported modules to the static
    // context so that xs:QName() casts can resolve prefixes from any module.
    let static_context =
        augment_static_context_with_module_namespaces(static_context, &declarations);

    let initial_mode = parse_initial_mode_value(initial_mode)?;
    // When no CLI initial mode is specified (defaults to Unnamed), use the
    // stylesheet's default-mode attribute if present.
    let initial_mode = match initial_mode {
        ast::ApplyTemplatesModeValue::Unnamed => match default_mode {
            ast::DefaultMode::EqName(name) => ast::ApplyTemplatesModeValue::EqName(name),
            ast::DefaultMode::Unnamed => ast::ApplyTemplatesModeValue::Unnamed,
        },
        other => other,
    };
    Ok((declarations, static_context, initial_mode))
}

fn augment_static_context_with_stylesheet_namespaces(
    static_context: StaticContext,
    xslt: &str,
) -> StaticContext {
    let mut namespaces = static_context.namespaces().clone();
    collect_namespaces_from_xslt(xslt, &mut namespaces);
    static_context.clone_with_namespaces(namespaces)
}

/// Collect namespace bindings from all included/imported module files
/// and add them to the static context. This ensures that prefixes declared
/// in any module (e.g. dbe: in errors.xsl) are available for xs:QName() casts.
fn augment_static_context_with_module_namespaces(
    static_context: StaticContext,
    declarations: &[PreprocessedDeclaration],
) -> StaticContext {
    let mut seen_uris = std::collections::HashSet::new();
    let mut namespaces = static_context.namespaces().clone();

    for decl in declarations {
        if let Some(uri) = &decl.stylesheet_uri {
            if !seen_uris.insert(uri.clone()) {
                continue;
            }
            let Some(path) = uri.strip_prefix("file://") else {
                continue;
            };
            let path = path.replace("%20", " ");
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            collect_namespaces_from_xslt(&content, &mut namespaces);
        }
    }

    static_context.clone_with_namespaces(namespaces)
}

fn collect_namespaces_from_xslt(xslt: &str, namespaces: &mut Namespaces) {
    let mut xot = Xot::new();
    let Ok(root) = xot.parse(xslt) else {
        return;
    };
    let Ok(document_element) = xot.document_element(root) else {
        return;
    };

    // Collect from document element: all namespace bindings including default
    for (prefix_id, namespace_id) in xot.namespaces_in_scope(document_element) {
        let prefix = xot.prefix_str(prefix_id);
        let namespace = xot.namespace_str(namespace_id);
        namespaces.add(&[(prefix, namespace)]);
    }

    // Collect from descendant elements: only prefixed bindings. This picks up
    // prefixes declared on child elements (e.g. xmlns:fun="..." on a template)
    // for runtime QName resolution, without leaking default namespace
    // declarations from literal result elements into the global context.
    for node in xot.descendants(document_element) {
        if node == document_element || !xot.is_element(node) {
            continue;
        }
        for (prefix_id, namespace_id) in xot.namespaces_in_scope(node) {
            let prefix = xot.prefix_str(prefix_id);
            if prefix.is_empty() {
                continue;
            }
            let namespace = xot.namespace_str(namespace_id);
            namespaces.add(&[(prefix, namespace)]);
        }
    }
}

pub(super) fn detect_stylesheet_version(xslt: &str) -> Option<u8> {
    let mut xot = Xot::new();
    let root = xot.parse(xslt).ok()?;
    let document_element = xot.document_element(root).ok()?;
    let version_name = xot.add_name("version");
    let version = xot.attributes(document_element).get(version_name)?;
    version.split('.').next()?.parse::<u8>().ok()
}

fn parse_initial_mode_value(
    initial_mode: Option<String>,
) -> error::SpannedResult<ast::ApplyTemplatesModeValue> {
    let Some(initial_mode) = initial_mode else {
        return Ok(ast::ApplyTemplatesModeValue::Unnamed);
    };

    let initial_mode = initial_mode.trim();
    if initial_mode == "#unnamed" {
        return Ok(ast::ApplyTemplatesModeValue::Unnamed);
    }
    if let Some(rest) = initial_mode.strip_prefix("Q{") {
        let Some((namespace, local_name)) = rest.split_once('}') else {
            return Err(error::Error::Unsupported(format!(
                "Unsupported initial mode name: {initial_mode}"
            ))
            .into());
        };
        return Ok(ast::ApplyTemplatesModeValue::EqName(OwnedName::new(
            local_name.to_string(),
            namespace.to_string(),
            "".to_string(),
        )));
    }
    if initial_mode.contains(':') {
        return Err(error::Error::Unsupported(format!(
            "Unsupported prefixed initial mode name: {initial_mode}"
        ))
        .into());
    }

    Ok(ast::ApplyTemplatesModeValue::EqName(OwnedName::new(
        initial_mode.to_string(),
        "".to_string(),
        "".to_string(),
    )))
}

pub(super) fn map_parse_error(_xslt: &str, error: ElementError) -> error::SpannedError {
    match error {
        ElementError::Unsupported(reason) => {
            if reason.starts_with("Could not read stylesheet: ") {
                error::Error::XTSE0165.into()
            } else {
                error::Error::Unsupported(format!("Failed parsing XSLT: Unsupported({reason:?})"))
                    .into()
            }
        }
        ElementError::Attribute(attribute_error) => match attribute_error {
            AttributeError::NotFound { span, .. } => error::SpannedError {
                error: error::Error::XTSE0010,
                span: Some((span.start..span.end).into()),
                detail: None,

                contexts: Vec::new(),
            },
            AttributeError::Unexpected { span, .. } => error::SpannedError {
                error: error::Error::XTSE0090,
                span: Some((span.start..span.end).into()),
                detail: None,

                contexts: Vec::new(),
            },
            AttributeError::Invalid { span, .. } | AttributeError::InvalidEqName { span, .. } => {
                error::SpannedError {
                    error: error::Error::XTSE0020,
                    span: Some((span.start..span.end).into()),
                    detail: None,

                    contexts: Vec::new(),
                }
            }
            AttributeError::XPathParser(parser_error) => parser_error.into(),
            AttributeError::StaticError { code, span } => error::SpannedError {
                error: match code {
                    "XTSE0150" => error::Error::XTSE0150,
                    "XTSE0110" => error::Error::XTSE0110,
                    "XTSE0340" => error::Error::XTSE0340,
                    "XTSE0805" => error::Error::XTSE0805,
                    _ => {
                        error::Error::Unsupported(format!("Unknown XSLT static error code: {code}"))
                    }
                },
                span: Some((span.start..span.end).into()),
                detail: None,

                contexts: Vec::new(),
            },
            other => error::Error::Unsupported(format!("Failed parsing XSLT: {:?}", other)).into(),
        },
        ElementError::Unexpected { span } => error::SpannedError {
            error: error::Error::XTSE0010,
            span: Some((span.start..span.end).into()),
            detail: None,
            contexts: Vec::new(),
        },
        ElementError::UnexpectedEnd => error::Error::XTSE0010.into(),
        ElementError::XPathRunTime(spanned_error) => spanned_error,
        other => error::Error::Unsupported(format!("Failed parsing XSLT: {:?}", other)).into(),
    }
}

fn process_imports_and_includes(
    declarations: ast::Declarations,
    base_dir: Option<std::path::PathBuf>,
    stylesheet_version: Option<u8>,
    module_static_variables: StaticVariables,
    entry_stylesheet_uri: Option<String>,
) -> error::SpannedResult<Vec<PreprocessedDeclaration>> {
    let (modules, _) = process_stylesheet_module(
        declarations,
        base_dir,
        &mut Vec::new(),
        Vec::new(),
        stylesheet_version,
        StaticVariables::new(),
        &module_static_variables,
        entry_stylesheet_uri,
    )?;
    let mut result = Vec::new();
    for (import_precedence, module) in modules.into_iter().enumerate() {
        for (declaration, decl_uri) in module.declarations {
            result.push(PreprocessedDeclaration {
                declaration,
                import_precedence: import_precedence as i64,
                module_path: module.module_path.clone(),
                stylesheet_uri: decl_uri.or(module.stylesheet_uri.clone()),
            });
        }
    }
    Ok(result)
}

fn process_stylesheet_module(
    declarations: ast::Declarations,
    base_dir: Option<std::path::PathBuf>,
    active_paths: &mut Vec<PathBuf>,
    module_path: Vec<usize>,
    _stylesheet_version: Option<u8>,
    initial_static_variables: StaticVariables,
    module_static_variables: &StaticVariables,
    stylesheet_uri: Option<String>,
) -> error::SpannedResult<(Vec<PreprocessedModule>, StaticVariables)> {
    let mut local_declarations: Vec<(ast::Declaration, Option<String>)> = Vec::new();
    let mut imports = Vec::new();
    let mut import_index = 0usize;
    let mut in_scope_static_variables = initial_static_variables;

    for decl in declarations {
        match &decl {
            ast::Declaration::Import(import) => {
                // Load and parse the imported stylesheet
                let (
                    imported_decls,
                    resolved_path,
                    imported_base_dir,
                    imported_version,
                    imported_module_static_variables,
                    imported_stylesheet_uri,
                ) = load_stylesheet(
                    &import.href.to_string(),
                    base_dir.as_ref(),
                    in_scope_static_variables.clone(),
                )?;
                if active_paths.contains(&resolved_path) {
                    return Err(error::Error::XTSE0180
                        .with_ast_span((import.span.start..import.span.end).into()));
                }
                active_paths.push(resolved_path);
                let mut imported_module_path = module_path.clone();
                imported_module_path.push(import_index);
                import_index += 1;
                let (processed, imported_static_variables) = process_stylesheet_module(
                    imported_decls,
                    imported_base_dir,
                    active_paths,
                    imported_module_path,
                    imported_version,
                    in_scope_static_variables.clone(),
                    &imported_module_static_variables,
                    Some(imported_stylesheet_uri),
                )?;
                active_paths.pop();
                merge_static_variables(&mut in_scope_static_variables, &imported_static_variables);
                imports.extend(processed);
            }
            ast::Declaration::Include(include) => {
                // Load and parse the included stylesheet
                let (
                    included_decls,
                    resolved_path,
                    included_base_dir,
                    included_version,
                    included_module_static_variables,
                    included_stylesheet_uri,
                ) = load_stylesheet(
                    &include.href.to_string(),
                    base_dir.as_ref(),
                    in_scope_static_variables.clone(),
                )?;
                if active_paths.contains(&resolved_path) {
                    return Err(error::Error::XTSE0180
                        .with_ast_span((include.span.start..include.span.end).into()));
                }
                active_paths.push(resolved_path);
                let (mut processed, included_static_variables) = process_stylesheet_module(
                    included_decls,
                    included_base_dir,
                    active_paths,
                    module_path.clone(),
                    included_version,
                    in_scope_static_variables.clone(),
                    &included_module_static_variables,
                    Some(included_stylesheet_uri),
                )?;
                active_paths.pop();
                in_scope_static_variables = included_static_variables;
                // Merge included module declarations back into the parent's
                // local list, but preserve each declaration's source URI for
                // correct static-base-uri() resolution.
                if let Some(included_local_module) = processed.pop() {
                    for (decl, decl_uri) in included_local_module.declarations {
                        // Prefer the declaration's own URI (from nested includes),
                        // fall back to the included module's URI.
                        let uri = decl_uri.or(included_local_module.stylesheet_uri.clone());
                        local_declarations.push((decl, uri));
                    }
                }
                imports.extend(processed);
            }
            _ => {
                remember_static_global(
                    &decl,
                    module_static_variables,
                    &mut in_scope_static_variables,
                );
                local_declarations.push((decl, None));
            }
        }
    }

    let mut result = imports;
    result.push(PreprocessedModule {
        declarations: local_declarations,
        module_path,
        stylesheet_uri,
    });
    Ok((result, in_scope_static_variables))
}

fn load_stylesheet(
    href: &str,
    base_dir: Option<&std::path::PathBuf>,
    initial_static_variables: StaticVariables,
) -> error::SpannedResult<(
    ast::Declarations,
    PathBuf,
    Option<PathBuf>,
    Option<u8>,
    StaticVariables,
    String,
)> {
    // Resolve the file path
    let path = if let Some(base_dir) = base_dir {
        base_dir.join(href)
    } else {
        std::path::PathBuf::from(href)
    };

    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
    let next_base_dir = canonical
        .parent()
        .or_else(|| path.parent())
        .map(std::path::Path::to_path_buf);

    // Try to read the file
    let content = std::fs::read_to_string(&path).map_err(|_| error::Error::XTSE0165)?;
    let stylesheet_version = detect_stylesheet_version(&content);
    let stylesheet_uri = format!("file://{}", canonical.display()).replace(' ', "%20");

    // Parse the stylesheet
    let (transform, static_variables) = parse_transform_with_static_variables_and_location(
        &content,
        initial_static_variables,
        Some(3),
        Some(31),
        next_base_dir.clone(),
        Some(stylesheet_uri.clone()),
        true,
    )
    .map_err(|e| {
        let mapped = map_parse_error(&content, e);
        match mapped.error {
            error::Error::Unsupported(_) => error::Error::XTSE0165.into(),
            _ => mapped,
        }
    })?;

    Ok((
        transform.declarations,
        canonical,
        next_base_dir,
        stylesheet_version,
        static_variables,
        stylesheet_uri,
    ))
}

fn merge_static_variables(target: &mut StaticVariables, source: &StaticVariables) {
    for (name, value) in source {
        target.insert(name.clone(), value.clone());
    }
}

fn remember_static_global(
    declaration: &ast::Declaration,
    module_static_variables: &StaticVariables,
    in_scope_static_variables: &mut StaticVariables,
) {
    let static_name = match declaration {
        ast::Declaration::Param(param) if param.static_ => Some(&param.name),
        ast::Declaration::Variable(variable) if variable.static_ => Some(&variable.name),
        _ => None,
    };

    let Some(static_name) = static_name else {
        return;
    };
    let Some(value) = module_static_variables.get(static_name) else {
        return;
    };
    in_scope_static_variables.insert(static_name.clone(), value.clone());
}

